use eframe::egui::{Key, Modifiers};

/// Bytes a terminal expects for special keys (arrows, F-keys, Ctrl/Alt combos).
/// Plain printable characters arrive as text events and are not handled here.
pub fn key_bytes(key: Key, m: &Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let modp = 1 + m.shift as u8 + 2 * (m.alt as u8) + 4 * (m.ctrl as u8);
    let nav = |c: char| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[1;{modp}{c}")
        } else if app_cursor {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[{c}")
        }
        .into_bytes()
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[{n};{modp}~")
        } else {
            format!("\x1b[{n}~")
        }
        .into_bytes()
    };
    let fkey = |c: char| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[1;{modp}{c}")
        } else {
            format!("\x1bO{c}")
        }
        .into_bytes()
    };
    let bytes = match key {
        Key::Enter => {
            if m.alt {
                b"\x1b\r".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        Key::Tab => {
            if m.shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        Key::Backspace => {
            if m.ctrl {
                vec![0x08]
            } else if m.alt {
                vec![0x1b, 0x7f]
            } else {
                vec![0x7f]
            }
        }
        Key::Escape => vec![0x1b],
        Key::ArrowUp => nav('A'),
        Key::ArrowDown => nav('B'),
        Key::ArrowRight => nav('C'),
        Key::ArrowLeft => nav('D'),
        Key::Home => nav('H'),
        Key::End => nav('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => fkey('P'),
        Key::F2 => fkey('Q'),
        Key::F3 => fkey('R'),
        Key::F4 => fkey('S'),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        _ => {
            let name = key.name();
            let ch = if name.len() == 1 {
                name.chars().next()?
            } else {
                return None;
            };
            if m.ctrl && !m.alt {
                if ch.is_ascii_uppercase() {
                    vec![ch as u8 - b'A' + 1]
                } else {
                    return None;
                }
            } else if m.alt && !m.ctrl && ch.is_ascii_alphanumeric() {
                let c = if m.shift {
                    ch.to_ascii_uppercase()
                } else {
                    ch.to_ascii_lowercase()
                };
                vec![0x1b, c as u8]
            } else {
                return None;
            }
        }
    };
    Some(bytes)
}

fn macro_bytes(name: &str) -> Option<Vec<u8>> {
    Some(match name {
        "enter" => b"\r".to_vec(),
        "tab" => b"\t".to_vec(),
        "esc" => vec![0x1b],
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        n if n.len() == 6 && n.starts_with("ctrl+") => {
            let c = n.as_bytes()[5];
            if c.is_ascii_lowercase() {
                vec![c - b'a' + 1]
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// Replaces macros like `{ctrl+c}` with control bytes. The flag tells whether the line had literal text.
pub fn expand_macros(line: &str) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut had_text = false;
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        if c == '{' {
            if let Some(end) = rest.find('}') {
                let name = rest[1..end].trim().to_lowercase();
                if let Some(b) = macro_bytes(&name) {
                    out.extend(b);
                    rest = &rest[end + 1..];
                    continue;
                }
            }
        }
        let l = c.len_utf8();
        out.extend_from_slice(&rest.as_bytes()[..l]);
        had_text = true;
        rest = &rest[l..];
    }
    (out, had_text)
}

/// Turns a button's command text into the bytes sent to the terminal.
/// Every non-empty line is typed; lines made only of macros are sent without Enter.
pub fn command_bytes(text: &str, press_enter: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (bytes, had_text) = expand_macros(line);
        out.extend(bytes);
        if had_text && press_enter {
            out.push(b'\r');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_keys() {
        let none = Modifiers::NONE;
        assert_eq!(key_bytes(Key::Enter, &none, false).unwrap(), b"\r");
        assert_eq!(key_bytes(Key::ArrowUp, &none, false).unwrap(), b"\x1b[A");
        assert_eq!(key_bytes(Key::ArrowUp, &none, true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes(Key::F5, &none, false).unwrap(), b"\x1b[15~");
        assert_eq!(key_bytes(Key::A, &none, false), None);
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert_eq!(key_bytes(Key::D, &ctrl, false).unwrap(), vec![4]);
        assert_eq!(
            key_bytes(Key::ArrowLeft, &ctrl, false).unwrap(),
            b"\x1b[1;5D"
        );
        let alt = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert_eq!(key_bytes(Key::B, &alt, false).unwrap(), b"\x1bb");
    }

    #[test]
    fn macros() {
        assert_eq!(command_bytes("{ctrl+c}", true), vec![3]);
        assert_eq!(command_bytes("ls -la\n\npwd", true), b"ls -la\rpwd\r");
        assert_eq!(command_bytes("echo {hi}", false), b"echo {hi}");
        assert_eq!(command_bytes("{ctrl+c}\nclear", true), b"\x03clear\r");
    }
}
