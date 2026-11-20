use eframe::egui::{
    self, Color32, CursorIcon, FontId, Frame, Margin, Response, RichText, Sense, Stroke, Ui,
};

use crate::theme::Palette;

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn to_rgb(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// Linear mix of two colors, t = 0 gives `a`, t = 1 gives `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Dark or white text, whichever reads better on `bg`.
pub fn contrast(bg: [u8; 3]) -> [u8; 3] {
    let lum = 0.299 * bg[0] as f32 + 0.587 * bg[1] as f32 + 0.114 * bg[2] as f32;
    if lum > 160.0 {
        [31, 35, 40]
    } else {
        [255, 255, 255]
    }
}

/// A rounded button with its own fill and text color, hover and press feedback.
pub fn color_button(
    ui: &mut Ui,
    label: &str,
    bg: Color32,
    fg: Color32,
    fill_width: bool,
    dark: bool,
) -> Response {
    let text = if label.trim().is_empty() {
        "(без названия)"
    } else {
        label
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(14.5), fg);
    let mut size = galley.size() + egui::vec2(32.0, 18.0);
    size.y = size.y.max(34.0);
    if fill_width {
        size.x = ui.available_width().max(size.x.min(60.0));
    }
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let down = resp.is_pointer_button_down_on();
        let fill = if down {
            mix(bg, Color32::BLACK, 0.18)
        } else if resp.hovered() {
            mix(bg, Color32::WHITE, 0.14)
        } else {
            bg
        };
        let p = ui.painter();
        if !down {
            let a = if dark { 70 } else { 30 };
            p.rect_filled(
                rect.translate(egui::vec2(0.0, 1.5)),
                9.0,
                Color32::from_black_alpha(a),
            );
        }
        p.rect_filled(rect, 9.0, fill);
        p.rect_stroke(
            rect,
            9.0,
            Stroke::new(1.0_f32, mix(bg, Color32::BLACK, 0.18)),
        );
        let pos = rect.center() - galley.size() / 2.0;
        p.galley(pos, galley, fg);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// A white (or dark) rounded card with a thin border.
pub fn card<R>(ui: &mut Ui, pal: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::none()
        .fill(pal.panel)
        .stroke(Stroke::new(1.0_f32, pal.border))
        .rounding(12.0)
        .inner_margin(Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn section_title(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.label(RichText::new(text).size(16.0).strong().color(pal.text));
    ui.add_space(2.0);
}
