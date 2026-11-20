#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use eframe::egui::IconData;

mod app;
mod config;
mod highlight;
mod hotkey;
mod keys;
mod session;
mod terminal;
mod theme;
mod widgets;

fn generated_icon() -> IconData {
    const SIZE: u32 = 128;
    let mut rgba = vec![0; (SIZE * SIZE * 4) as usize];

    fn rounded_rect(
        rgba: &mut [u8],
        bounds: (f32, f32, f32, f32),
        radius: f32,
        color: impl Fn(f32) -> [u8; 4],
    ) {
        let (left, top, right, bottom) = bounds;
        for y in top as u32..bottom as u32 {
            for x in left as u32..right as u32 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let near_x = px.clamp(left + radius, right - radius);
                let near_y = py.clamp(top + radius, bottom - radius);
                if (px - near_x).powi(2) + (py - near_y).powi(2) > radius.powi(2) {
                    continue;
                }
                let rgba_index = ((y * SIZE + x) * 4) as usize;
                rgba[rgba_index..rgba_index + 4]
                    .copy_from_slice(&color((py - top) / (bottom - top)));
            }
        }
    }

    fn circle(rgba: &mut [u8], center: (f32, f32), radius: f32, color: [u8; 4]) {
        for y in (center.1 - radius) as u32..=(center.1 + radius) as u32 {
            for x in (center.0 - radius) as u32..=(center.0 + radius) as u32 {
                if (x as f32 + 0.5 - center.0).powi(2) + (y as f32 + 0.5 - center.1).powi(2)
                    <= radius.powi(2)
                {
                    let index = ((y * SIZE + x) * 4) as usize;
                    rgba[index..index + 4].copy_from_slice(&color);
                }
            }
        }
    }

    fn line(rgba: &mut [u8], start: (f32, f32), end: (f32, f32), width: f32, color: [u8; 4]) {
        let dx = end.0 - start.0;
        let dy = end.1 - start.1;
        let length_sq = dx * dx + dy * dy;
        for y in (start.1.min(end.1) - width) as u32..=(start.1.max(end.1) + width) as u32 {
            for x in (start.0.min(end.0) - width) as u32..=(start.0.max(end.0) + width) as u32 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let t = (((px - start.0) * dx + (py - start.1) * dy) / length_sq).clamp(0.0, 1.0);
                if (px - (start.0 + t * dx)).powi(2) + (py - (start.1 + t * dy)).powi(2)
                    <= (width / 2.0).powi(2)
                {
                    let index = ((y * SIZE + x) * 4) as usize;
                    rgba[index..index + 4].copy_from_slice(&color);
                }
            }
        }
    }

    rounded_rect(&mut rgba, (0.0, 0.0, SIZE as f32, SIZE as f32), 27.0, |t| {
        [
            (42.0 + 9.0 * t) as u8,
            (57.0 + 11.0 * t) as u8,
            (83.0 + 16.0 * t) as u8,
            255,
        ]
    });
    rounded_rect(&mut rgba, (18.0, 24.0, 110.0, 104.0), 13.0, |_| {
        [235, 238, 245, 255]
    });
    rounded_rect(&mut rgba, (22.0, 28.0, 106.0, 51.0), 9.0, |_| {
        [226, 230, 239, 255]
    });
    rounded_rect(&mut rgba, (22.0, 45.0, 106.0, 100.0), 8.0, |t| {
        [
            (13.0 + 7.0 * t) as u8,
            (21.0 + 11.0 * t) as u8,
            (35.0 + 17.0 * t) as u8,
            255,
        ]
    });
    circle(&mut rgba, (34.0, 39.0), 3.2, [255, 95, 86, 255]);
    circle(&mut rgba, (44.0, 39.0), 3.2, [255, 189, 46, 255]);
    circle(&mut rgba, (54.0, 39.0), 3.2, [39, 201, 63, 255]);
    line(
        &mut rgba,
        (39.0, 64.0),
        (49.0, 72.0),
        4.0,
        [105, 229, 184, 255],
    );
    line(
        &mut rgba,
        (49.0, 72.0),
        (39.0, 80.0),
        4.0,
        [105, 229, 184, 255],
    );
    line(
        &mut rgba,
        (58.0, 80.0),
        (74.0, 80.0),
        4.0,
        [112, 165, 255, 255],
    );

    IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}

pub(crate) fn app_icon() -> IconData {
    match image::load_from_memory_with_format(include_bytes!("../app.ico"), image::ImageFormat::Ico)
    {
        Ok(image) => {
            let image = image.into_rgba8();
            let (width, height) = image.dimensions();
            IconData {
                rgba: image.into_raw(),
                width,
                height,
            }
        }
        Err(error) => {
            eprintln!("Некорректная встроенная ICO-иконка: {error}");
            generated_icon()
        }
    }
}

#[cfg(windows)]
pub(crate) fn create_tray_icon() -> Result<tray_icon::TrayIcon, String> {
    let icon = app_icon();
    let icon = tray_icon::Icon::from_rgba(icon.rgba, icon.width, icon.height)
        .map_err(|error| error.to_string())?;
    tray_icon::TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("Better Console")
        .build()
        .map_err(|error| error.to_string())
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Better Console")
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([780.0, 480.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Better Console",
        options,
        Box::new(|cc| Box::new(app::App::new(cc))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_uses_the_embedded_ico() {
        let expected = image::load_from_memory_with_format(
            include_bytes!("../app.ico"),
            image::ImageFormat::Ico,
        )
        .expect("decode app.ico")
        .into_rgba8();
        let icon = app_icon();

        assert_eq!(icon.width, expected.width());
        assert_eq!(icon.height, expected.height());
        assert_eq!(icon.rgba, expected.into_raw());
    }
}
