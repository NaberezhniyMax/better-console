use eframe::egui::{self, Color32, Context, FontFamily, FontId, Rounding, Stroke, TextStyle};

use crate::config::ThemeMode;

const fn hex(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    // syntax colors
    pub s_time: Color32,
    pub s_info: Color32,
    pub s_warn: Color32,
    pub s_error: Color32,
    pub s_debug: Color32,
    pub s_string: Color32,
    pub s_number: Color32,
    pub s_bracket: Color32,
    pub s_keyword: Color32,
    pub s_link: Color32,
    pub s_path: Color32,
    pub s_host: Color32,
    pub ansi: [Color32; 16],
}

impl Palette {
    pub fn new(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            bg: hex(0xF3F5F9),
            panel: hex(0xFFFFFF),
            card: hex(0xFCFCFE),
            border: hex(0xE1E5EC),
            text: hex(0x1F2430),
            muted: hex(0x8A93A5),
            accent: hex(0x4F6BED),
            ok: hex(0x2E9E5B),
            warn: hex(0xD98A00),
            err: hex(0xD92D3A),
            s_time: hex(0x9AA3B5),
            s_info: hex(0x0B7FAB),
            s_warn: hex(0xC27A00),
            s_error: hex(0xD92D3A),
            s_debug: hex(0x8250DF),
            s_string: hex(0x1A8D4F),
            s_number: hex(0xD9640E),
            s_bracket: hex(0x6B7488),
            s_keyword: hex(0xC1388F),
            s_link: hex(0x1B6EF3),
            s_path: hex(0x0E7C7B),
            s_host: hex(0x8250DF),
            ansi: [
                hex(0x1F2430),
                hex(0xD92D3A),
                hex(0x1A8D4F),
                hex(0xB7791F),
                hex(0x2F5BEA),
                hex(0xA23FB8),
                hex(0x0B7FAB),
                hex(0x6B7488),
                hex(0x5A6275),
                hex(0xE5484D),
                hex(0x2EA862),
                hex(0xC98A1B),
                hex(0x4F6BED),
                hex(0xB95BD0),
                hex(0x1393C2),
                hex(0x3A4156),
            ],
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: hex(0x14161B),
            panel: hex(0x1B1E25),
            card: hex(0x101217),
            border: hex(0x2A2F3A),
            text: hex(0xD6DCE8),
            muted: hex(0x6F788A),
            accent: hex(0x7C93FF),
            ok: hex(0x3DAE7A),
            warn: hex(0xE8B14A),
            err: hex(0xE5484D),
            s_time: hex(0x5C6578),
            s_info: hex(0x5CC8E8),
            s_warn: hex(0xE8B14A),
            s_error: hex(0xF26D78),
            s_debug: hex(0xC49BFF),
            s_string: hex(0x8FD694),
            s_number: hex(0xF0A36B),
            s_bracket: hex(0x7F8AA0),
            s_keyword: hex(0xF08BC8),
            s_link: hex(0x6EA8FF),
            s_path: hex(0x7FD1C7),
            s_host: hex(0xC49BFF),
            ansi: [
                hex(0x3B4252),
                hex(0xF26D78),
                hex(0x8FD694),
                hex(0xE8B14A),
                hex(0x6EA8FF),
                hex(0xC49BFF),
                hex(0x5CC8E8),
                hex(0xD6DCE8),
                hex(0x5C6578),
                hex(0xFF8A93),
                hex(0xA6E3AB),
                hex(0xF5C96B),
                hex(0x8FBBFF),
                hex(0xD7B6FF),
                hex(0x7FDCF2),
                hex(0xFFFFFF),
            ],
        }
    }
}

pub fn apply(ctx: &Context, pal: &Palette, mono_size: f32) {
    let mut v = if pal.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = pal.bg;
    v.window_fill = pal.panel;
    v.extreme_bg_color = pal.card;
    v.faint_bg_color = pal.bg;
    v.window_stroke = Stroke::new(1.0_f32, pal.border);
    v.window_rounding = Rounding::same(14.0);
    v.menu_rounding = Rounding::same(10.0);
    v.selection.bg_fill = pal.accent.linear_multiply(0.30);
    v.selection.stroke = Stroke::new(1.0_f32, pal.accent);
    v.hyperlink_color = pal.s_link;

    let (idle, hover, press) = if pal.dark {
        (hex(0x232733), hex(0x2B3040), hex(0x343A4D))
    } else {
        (hex(0xF1F3F8), hex(0xE7EBFA), hex(0xDAE1FA))
    };
    let r = Rounding::same(9.0);
    v.widgets.noninteractive.rounding = r;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, pal.text);
    v.widgets.inactive.rounding = r;
    v.widgets.inactive.weak_bg_fill = idle;
    v.widgets.inactive.bg_fill = idle;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, pal.text);
    v.widgets.hovered.rounding = r;
    v.widgets.hovered.weak_bg_fill = hover;
    v.widgets.hovered.bg_fill = hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, pal.accent.linear_multiply(0.6));
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, pal.text);
    v.widgets.active.rounding = r;
    v.widgets.active.weak_bg_fill = press;
    v.widgets.active.bg_fill = press;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, pal.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.5_f32, pal.text);
    v.widgets.open.rounding = r;
    v.widgets.open.weak_bg_fill = hover;
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(22.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(mono_size, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}
