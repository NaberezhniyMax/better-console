use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::Context;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::config::{Profile, ProfileKind};

/// Splits a command line into arguments, honoring "double" and 'single' quotes.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut in_token = false;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    in_token = true;
                }
                c if c.is_whitespace() => {
                    if in_token {
                        out.push(std::mem::take(&mut cur));
                        in_token = false;
                    }
                }
                c => {
                    cur.push(c);
                    in_token = true;
                }
            },
        }
    }
    if in_token {
        out.push(cur);
    }
    out
}

/// Program and arguments that a profile starts.
pub fn build_command(p: &Profile, ssh_path: &str) -> Result<(String, Vec<String>), String> {
    let win = cfg!(windows);
    let s = |x: &str| x.to_string();
    Ok(match p.kind {
        ProfileKind::PowerShell => {
            if win {
                (s("powershell.exe"), vec![s("-NoLogo")])
            } else {
                (s("pwsh"), vec![s("-NoLogo")])
            }
        }
        ProfileKind::Pwsh => (s("pwsh"), vec![s("-NoLogo")]),
        ProfileKind::Cmd => {
            if win {
                (s("cmd.exe"), vec![])
            } else {
                (s("sh"), vec![])
            }
        }
        ProfileKind::Shell => {
            if win {
                (s("cmd.exe"), vec![])
            } else {
                (
                    std::env::var("SHELL").unwrap_or_else(|_| s("/bin/bash")),
                    vec![],
                )
            }
        }
        ProfileKind::Ssh => {
            if p.host.trim().is_empty() {
                return Err("не указан адрес сервера (Подключения)".into());
            }
            let mut args = Vec::new();
            if p.port != 22 {
                args.push(s("-p"));
                args.push(p.port.to_string());
            }
            if !p.identity.trim().is_empty() {
                args.push(s("-i"));
                args.push(p.identity.trim().to_string());
            }
            args.push(s("-o"));
            args.push(s("ServerAliveInterval=30"));
            args.extend(split_args(&p.extra_args));
            let target = if p.user.trim().is_empty() {
                p.host.trim().to_string()
            } else {
                format!("{}@{}", p.user.trim(), p.host.trim())
            };
            args.push(target);
            let prog = if ssh_path.trim().is_empty() {
                s("ssh")
            } else {
                ssh_path.trim().to_string()
            };
            (prog, args)
        }
        ProfileKind::Custom => {
            let parts = split_args(&p.custom_cmd);
            let Some((prog, args)) = parts.split_first() else {
                return Err("команда не задана (Подключения)".into());
            };
            (prog.clone(), args.to_vec())
        }
    })
}

fn nz(v: u16) -> NonZeroU16 {
    NonZeroU16::new(v.max(1)).unwrap()
}

#[derive(Clone, Copy, Debug)]
pub struct Sel {
    pub anchor: (u16, u16),
    pub head: (u16, u16),
}

/// One terminal tab: a process on a pseudo terminal plus the screen emulator.
pub struct Session {
    pub id: u64,
    pub profile_id: u64,
    pub title: String,
    pub accent: [u8; 3],
    pub cmdline: String,
    pub parser: vt100::Parser,
    pub rows: u16,
    pub cols: u16,
    pub scroll: usize,
    pub scroll_acc: f32,
    pub selection: Option<Sel>,
    /// Exit code once the process is gone and its output is drained.
    pub finished: Option<u32>,
    exit_seen: Option<(u32, Instant)>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    rx: Receiver<Vec<u8>>,
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

impl Session {
    pub fn spawn(
        id: u64,
        p: &Profile,
        ssh_path: &str,
        scrollback: usize,
        ctx: &Context,
    ) -> Result<Self, String> {
        let (prog, args) = build_command(p, ssh_path)?;
        let (rows, cols) = (30u16, 110u16);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&prog);
        cmd.args(&args);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let wd = p.work_dir.trim();
        if !wd.is_empty() && Path::new(wd).is_dir() {
            cmd.cwd(wd);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("{prog}: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        repaint.request_repaint();
                    }
                }
            }
            repaint.request_repaint();
        });

        let mut cmdline = prog.clone();
        for a in &args {
            cmdline.push(' ');
            cmdline.push_str(a);
        }
        Ok(Self {
            id,
            profile_id: p.id,
            title: p.name.clone(),
            accent: p.color,
            cmdline,
            parser: vt100::Parser::new(nz(rows), nz(cols), scrollback),
            rows,
            cols,
            scroll: 0,
            scroll_acc: 0.0,
            selection: None,
            finished: None,
            exit_seen: None,
            master: pair.master,
            writer,
            child,
            rx,
        })
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    pub fn send_text(&mut self, text: &str) {
        self.write_bytes(text.as_bytes());
    }

    pub fn paste_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        if self.parser.screen().bracketed_paste() {
            self.send_text(&format!("\x1b[200~{text}\x1b[201~"));
        } else {
            self.send_text(&text);
        }
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == (self.rows, self.cols) || rows == 0 || cols == 0 {
            return;
        }
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser.set_size(nz(rows), nz(cols));
        self.rows = rows;
        self.cols = cols;
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        // Some programs (and ConPTY itself) ask the terminal where the cursor is, or who it is.
        if contains(bytes, b"\x1b[6n") {
            let (r, c) = self.parser.screen().cursor_position();
            self.send_text(&format!("\x1b[{};{}R", r + 1, c + 1));
        }
        if contains(bytes, b"\x1b[c") || contains(bytes, b"\x1b[0c") {
            self.send_text("\x1b[?1;2c");
        }
    }

    /// Pulls output from the process into the emulator. Returns true if anything arrived.
    pub fn poll(&mut self) -> bool {
        let mut budget: usize = 512 * 1024;
        let mut got = false;
        let mut closed = false;
        while budget > 0 {
            match self.rx.try_recv() {
                Ok(b) => {
                    budget = budget.saturating_sub(b.len());
                    self.feed(&b);
                    got = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        if self.finished.is_none() {
            if self.exit_seen.is_none() {
                if let Ok(Some(st)) = self.child.try_wait() {
                    self.exit_seen = Some((st.exit_code(), Instant::now()));
                }
            }
            if let Some((code, at)) = self.exit_seen {
                if closed || at.elapsed() > Duration::from_millis(800) {
                    self.finished = Some(code);
                }
            }
        }
        got
    }

    /// Text of the current selection, if any.
    pub fn selection_text(&self) -> Option<String> {
        let sel = self.selection?;
        let (mut a, mut b) = (sel.anchor, sel.head);
        if a == b {
            return None;
        }
        if (b.0, b.1) < (a.0, a.1) {
            std::mem::swap(&mut a, &mut b);
        }
        let screen = self.parser.screen();
        let last_col = self.cols.saturating_sub(1);
        let mut lines = Vec::new();
        for r in a.0..=b.0 {
            let (c0, c1) = if a.0 == b.0 {
                (a.1, b.1)
            } else if r == a.0 {
                (a.1, last_col)
            } else if r == b.0 {
                (0, b.1)
            } else {
                (0, last_col)
            };
            let mut line = String::new();
            for c in c0..=c1 {
                if let Some(cell) = screen.cell(r, c) {
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let t = cell.contents();
                    if t.is_empty() {
                        line.push(' ')
                    } else {
                        line.push_str(t)
                    }
                }
            }
            lines.push(line.trim_end().to_string());
        }
        Some(lines.join("\n"))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quotes() {
        assert_eq!(
            split_args(r#"ssh -i "my key" -J jump host"#),
            vec!["ssh", "-i", "my key", "-J", "jump", "host"]
        );
        assert_eq!(split_args("  a   'b c' "), vec!["a", "b c"]);
    }

    #[cfg(unix)]
    #[test]
    fn pty_roundtrip() {
        let mut p = Profile::new(1, "t", ProfileKind::Custom, [0, 0, 0]);
        p.custom_cmd = r#"sh -c "printf 'hello\n'; printf '\033[31mred\033[0m\n'; cat""#.into();
        let ctx = Context::default();
        let mut s = Session::spawn(1, &p, "", 100, &ctx).expect("spawn");
        let wait = |s: &mut Session, needle: &str| {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(5) {
                s.poll();
                if s.parser.screen().contents().contains(needle) {
                    return true;
                }
                thread::sleep(Duration::from_millis(20));
            }
            false
        };
        assert!(
            wait(&mut s, "hello"),
            "no hello: {:?}",
            s.parser.screen().contents()
        );
        assert!(wait(&mut s, "red"));
        // the word "red" is printed with ANSI color 1
        let cell = s.parser.screen().cell(1, 0).unwrap();
        assert_eq!(cell.contents(), "r");
        assert_eq!(cell.fgcolor(), vt100::Color::Idx(1));
        // input goes to the process (cat echoes it back)
        s.send_text("ping-pong\n");
        assert!(wait(&mut s, "ping-pong"));
        s.selection = Some(Sel {
            anchor: (0, 0),
            head: (0, 4),
        });
        assert_eq!(s.selection_text().as_deref(), Some("hello"));
        // Ctrl+D ends cat and the session reports the exit
        s.write_bytes(&[4]);
        let start = Instant::now();
        while s.finished.is_none() && start.elapsed() < Duration::from_secs(5) {
            s.poll();
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(s.finished, Some(0));
    }

    #[test]
    fn ssh_command_line() {
        let mut p = Profile::new(1, "x", ProfileKind::Ssh, [0, 0, 0]);
        p.host = "example.com".into();
        p.user = "root".into();
        p.port = 2222;
        p.identity = "/k/id".into();
        p.extra_args = "-J jump".into();
        let (prog, args) = build_command(&p, "").unwrap();
        assert_eq!(prog, "ssh");
        assert_eq!(
            args,
            vec![
                "-p",
                "2222",
                "-i",
                "/k/id",
                "-o",
                "ServerAliveInterval=30",
                "-J",
                "jump",
                "root@example.com"
            ]
        );
        p.host.clear();
        assert!(build_command(&p, "").is_err());
    }
}
