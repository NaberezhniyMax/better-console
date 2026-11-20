use std::time::Duration;

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, vec2, Align2, Color32, Event, EventFilter, FontId, Id, PointerButton, Pos2, Rect, Sense,
    Stroke, Ui,
};

use crate::highlight;
use crate::hotkey::Hotkey;
use crate::keys::key_bytes;
use crate::session::{Sel, Session};
use crate::theme::Palette;
use crate::widgets::mix;

pub struct Opts<'a> {
    pub pal: &'a Palette,
    pub font_size: f32,
    pub highlight: bool,
    /// Hotkeys that fired this frame (the terminal must not receive them).
    pub fired: &'a [Hotkey],
    pub skip_text: bool,
}

fn ansi256(i: u8, pal: &Palette) -> Color32 {
    match i {
        0..=15 => pal.ansi[i as usize],
        16..=231 => {
            let n = i - 16;
            let lv = [0u8, 95, 135, 175, 215, 255];
            Color32::from_rgb(
                lv[(n / 36) as usize],
                lv[((n / 6) % 6) as usize],
                lv[(n % 6) as usize],
            )
        }
        _ => Color32::from_gray(8 + 10 * (i - 232)),
    }
}

fn resolve(c: vt100::Color, bold: bool, pal: &Palette) -> Option<Color32> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(ansi256(if bold && i < 8 { i + 8 } else { i }, pal)),
        vt100::Color::Rgb(r, g, b) => Some(Color32::from_rgb(r, g, b)),
    }
}

fn lum(c: Color32) -> f32 {
    0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32
}

/// Keeps colors chosen by programs readable on the current background.
fn legible(fg: Color32, pal: &Palette) -> Color32 {
    if (lum(fg) - lum(pal.card)).abs() < 55.0 {
        mix(fg, pal.text, 0.6)
    } else {
        fg
    }
}

struct Cv<'a> {
    text: &'a str,
    fg: Option<Color32>,
    bg: Option<Color32>,
    /// default foreground, eligible for syntax highlighting
    plain: bool,
    italic: bool,
    underline: bool,
}

fn row_job(
    screen: &vt100::Screen,
    r: u16,
    cols: u16,
    pal: &Palette,
    font: &FontId,
    highlight: bool,
) -> LayoutJob {
    let mut cells: Vec<Cv> = Vec::with_capacity(cols as usize);
    for c in 0..cols {
        let Some(cell) = screen.cell(r, c) else { break };
        if cell.is_wide_continuation() {
            continue;
        }
        let fg_r = resolve(cell.fgcolor(), cell.bold(), pal);
        let bg_r = resolve(cell.bgcolor(), false, pal);
        let (fg, bg, plain) = if cell.inverse() {
            (
                Some(bg_r.unwrap_or(pal.card)),
                Some(fg_r.unwrap_or(pal.text)),
                false,
            )
        } else if cell.dim() {
            (
                Some(mix(fg_r.unwrap_or(pal.text), pal.card, 0.45)),
                bg_r,
                false,
            )
        } else {
            let fg = fg_r.map(|f| if bg_r.is_none() { legible(f, pal) } else { f });
            (fg, bg_r, fg_r.is_none())
        };
        cells.push(Cv {
            text: cell.contents(),
            fg,
            bg,
            plain,
            italic: cell.italic(),
            underline: cell.underline(),
        });
    }
    while cells
        .last()
        .is_some_and(|c| (c.text.is_empty() || c.text == " ") && c.bg.is_none())
    {
        cells.pop();
    }

    let mut line = String::new();
    let mut starts = Vec::with_capacity(cells.len());
    for c in &cells {
        starts.push(line.len());
        line.push_str(if c.text.is_empty() { " " } else { c.text });
    }
    let mut over: Vec<Option<Color32>> = vec![None; cells.len()];
    if highlight {
        for (range, color) in highlight::spans(&line, pal) {
            let mut i = starts.partition_point(|&x| x < range.start);
            while i < cells.len() && starts[i] < range.end {
                if cells[i].plain {
                    over[i] = Some(color);
                }
                i += 1;
            }
        }
    }

    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let flush = |job: &mut LayoutJob,
                 run: &mut String,
                 fmt: Option<(Color32, Option<Color32>, bool, bool)>| {
        if let (false, Some((fg, bg, italic, ul))) = (run.is_empty(), fmt) {
            job.append(
                run,
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color: fg,
                    background: bg.unwrap_or(Color32::TRANSPARENT),
                    italics: italic,
                    underline: if ul {
                        Stroke::new(1.0_f32, fg)
                    } else {
                        Stroke::NONE
                    },
                    ..Default::default()
                },
            );
        }
        run.clear();
    };
    let mut run = String::new();
    let mut cur = None;
    for (i, c) in cells.iter().enumerate() {
        let fg = c.fg.or(over[i]).unwrap_or(pal.text);
        let fmt = Some((fg, c.bg, c.italic, c.underline));
        if fmt != cur {
            flush(&mut job, &mut run, cur);
            cur = fmt;
        }
        run.push_str(if c.text.is_empty() { " " } else { c.text });
    }
    flush(&mut job, &mut run, cur);
    job
}

/// Draws the terminal and feeds keyboard input to the session. Returns the terminal's rectangle.
pub fn show(ui: &mut Ui, s: &mut Session, o: &Opts) -> (Rect, Option<String>) {
    let pal = o.pal;
    let font = FontId::monospace(o.font_size);
    let (cw, ch) = ui.fonts(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
    let avail = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(avail, Sense::hover());
    let id = Id::new(("terminal", s.id));
    let resp = ui.interact(rect, id, Sense::click_and_drag());

    // Terminal size follows the window.
    let cols = ((rect.width() / cw).floor() as u16).clamp(8, 500);
    let rows = ((rect.height() / ch).floor() as u16).clamp(2, 300);
    s.resize(rows, cols);

    // Keep keyboard focus on the terminal unless a text field wants it.
    if !ui.ctx().wants_keyboard_input() || resp.clicked() || resp.drag_started() {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let focused = ui.memory(|m| m.has_focus(id));
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
    }

    // Scrollback position.
    let old_scroll = s.scroll;
    s.parser.screen_mut().set_scrollback(s.scroll);
    s.scroll = s.parser.screen().scrollback();
    if resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            s.scroll_acc += dy / ch;
            let n = s.scroll_acc.trunc();
            s.scroll_acc -= n;
            if s.parser.screen().alternate_screen() {
                let app = s.parser.screen().application_cursor();
                let (up, down) = if app {
                    ("\x1bOA", "\x1bOB")
                } else {
                    ("\x1b[A", "\x1b[B")
                };
                let seq = if n > 0.0 { up } else { down };
                for _ in 0..(n.abs() as usize) {
                    s.send_text(seq);
                }
            } else if n != 0.0 {
                let want = (s.scroll as i64 + n as i64).max(0) as usize;
                s.parser.screen_mut().set_scrollback(want);
                s.scroll = s.parser.screen().scrollback();
            }
        }
    }

    // Keyboard.
    let mut sent = false;
    let mut paste_error = None;
    if resp.secondary_clicked() {
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) => {
                s.paste_text(&text);
                sent = !text.is_empty();
            }
            Err(error) => paste_error = Some(error.to_string()),
        }
    }
    if focused {
        let (events, mods) = ui.input(|i| (i.events.clone(), i.modifiers));
        for ev in &events {
            match ev {
                Event::Text(t) => {
                    if o.skip_text || (mods.alt && !mods.ctrl) {
                        continue;
                    }
                    s.send_text(t);
                    sent = true;
                }
                Event::Paste(t) => {
                    s.paste_text(t);
                    sent = true;
                }
                Event::Copy => {
                    if let Some(text) = s.selection_text() {
                        ui.ctx().output_mut(|out| out.copied_text = text);
                        s.selection = None;
                    } else {
                        s.write_bytes(&[3]); // Ctrl+C: interrupt
                        sent = true;
                    }
                }
                Event::Cut => {
                    s.write_bytes(&[0x18]);
                    sent = true;
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if o.fired.contains(&Hotkey::from_event(*key, modifiers)) {
                        continue;
                    }
                    let app = s.parser.screen().application_cursor();
                    if let Some(b) = key_bytes(*key, modifiers, app) {
                        s.write_bytes(&b);
                        sent = true;
                    }
                }
                _ => {}
            }
        }
    }
    if sent && s.scroll != 0 {
        s.scroll = 0;
        s.parser.screen_mut().set_scrollback(0);
    }
    if sent || s.scroll != old_scroll {
        s.selection = None;
    }

    // Mouse selection.
    let cell_at = |p: Pos2| -> (u16, u16) {
        let c = ((p.x - rect.left()) / cw).floor().max(0.0) as u16;
        let r = ((p.y - rect.top()) / ch).floor().max(0.0) as u16;
        (r.min(rows - 1), c.min(cols - 1))
    };
    if resp.drag_started_by(PointerButton::Primary) {
        if let Some(p) = resp.interact_pointer_pos() {
            let c = cell_at(p);
            s.selection = Some(Sel { anchor: c, head: c });
        }
    } else if resp.dragged_by(PointerButton::Primary) {
        if let (Some(p), Some(sel)) = (resp.interact_pointer_pos(), s.selection.as_mut()) {
            sel.head = cell_at(p);
        }
    } else if resp.double_clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let (r, c) = cell_at(p);
            let screen = s.parser.screen();
            let is_word = |col: u16| {
                screen.cell(r, col).is_some_and(|x| {
                    let t = x.contents();
                    !t.is_empty() && !t.chars().all(char::is_whitespace)
                })
            };
            if is_word(c) {
                let (mut a, mut b) = (c, c);
                while a > 0 && is_word(a - 1) {
                    a -= 1;
                }
                while b + 1 < cols && is_word(b + 1) {
                    b += 1;
                }
                s.selection = Some(Sel {
                    anchor: (r, a),
                    head: (r, b),
                });
            }
        }
    } else if resp.clicked() {
        s.selection = None;
    }

    // Paint.
    let painter = ui.painter_at(rect);
    let highlight_on = o.highlight && !s.parser.screen().alternate_screen();
    for r in 0..rows {
        let job = row_job(s.parser.screen(), r, cols, pal, &font, highlight_on);
        let galley = ui.fonts(|f| f.layout_job(job));
        painter.galley(rect.min + vec2(0.0, r as f32 * ch), galley, pal.text);
    }

    if let Some(sel) = s.selection {
        let (mut a, mut b) = (sel.anchor, sel.head);
        if (b.0, b.1) < (a.0, a.1) {
            std::mem::swap(&mut a, &mut b);
        }
        let tint =
            Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 70);
        for r in a.0..=b.0 {
            let (c0, c1) = if a.0 == b.0 {
                (a.1, b.1)
            } else if r == a.0 {
                (a.1, cols - 1)
            } else if r == b.0 {
                (0, b.1)
            } else {
                (0, cols - 1)
            };
            let min = rect.min + vec2(c0 as f32 * cw, r as f32 * ch);
            painter.rect_filled(
                Rect::from_min_size(min, vec2((c1 - c0 + 1) as f32 * cw, ch)),
                2.0,
                tint,
            );
        }
    }

    let screen = s.parser.screen();
    if s.scroll == 0 && !screen.hide_cursor() && s.finished.is_none() {
        let (cr, cc) = screen.cursor_position();
        let cur = Rect::from_min_size(
            rect.min + vec2(cc as f32 * cw, cr as f32 * ch),
            vec2(cw, ch),
        );
        if focused {
            let on = ui.input(|i| (i.time * 1.8) as i64 % 2 == 0);
            if on {
                painter.rect_filled(
                    cur,
                    1.5,
                    Color32::from_rgba_unmultiplied(
                        pal.accent.r(),
                        pal.accent.g(),
                        pal.accent.b(),
                        150,
                    ),
                );
            }
            ui.ctx().request_repaint_after(Duration::from_millis(280));
        } else {
            painter.rect_stroke(cur, 1.5, Stroke::new(1.0_f32, pal.accent));
        }
    }

    if s.scroll > 0 {
        let pill = Rect::from_min_size(
            Pos2::new(rect.right() - 126.0, rect.bottom() - 36.0),
            vec2(116.0, 28.0),
        );
        let r2 = ui.interact(pill, Id::new(("tail", s.id)), Sense::click());
        let fill = if r2.hovered() {
            mix(pal.accent, Color32::WHITE, 0.15)
        } else {
            pal.accent
        };
        painter.rect_filled(pill, 14.0, fill);
        painter.text(
            pill.center(),
            Align2::CENTER_CENTER,
            "К концу",
            FontId::proportional(13.5),
            Color32::WHITE,
        );
        if r2.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            s.scroll = 0;
            s.parser.screen_mut().set_scrollback(0);
        }
    }
    (rect, paste_error)
}
