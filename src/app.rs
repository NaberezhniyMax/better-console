use std::time::Duration;

use eframe::egui::{
    self, vec2, Align, Align2, Color32, Context, Event, Frame, Id, Key, Layout, Margin, RichText,
    ScrollArea, Sense, Stroke, TextEdit, TextStyle, Ui,
};

use crate::config::{CmdButton, Config, Dock, Profile, ProfileKind, ThemeMode};
use crate::hotkey::{self, Hotkey};
use crate::keys::command_bytes;
use crate::session::{build_command, Session};
use crate::terminal;
use crate::theme::{self, Palette};
use crate::widgets::{card, color_button, contrast, rgb, section_title, to_rgb};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Console,
    Buttons,
    Profiles,
    Settings,
}

/// Deferred changes produced while drawing (so we never mutate lists mid-iteration).
enum Action {
    Run(u64),
    Edit(u64),
    Delete(u64),
    Shift(u64, i32),
    Move(u64, Dock),
    Open(u64),
    Close(usize),
    Reconnect(usize),
}

const SWATCHES: [[u8; 3]; 8] = [
    [79, 107, 237],
    [46, 158, 91],
    [229, 72, 77],
    [245, 165, 36],
    [14, 165, 165],
    [142, 78, 198],
    [100, 116, 139],
    [233, 61, 130],
];

fn is_physical_paste_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Key {
            physical_key: Some(Key::V),
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } if modifiers.command && !modifiers.alt
    )
}

fn replace_physical_paste_events(events: &mut Vec<Event>, clipboard_text: Option<&str>) -> bool {
    if !events.iter().any(is_physical_paste_event) {
        return false;
    }

    events.retain(|event| !is_physical_paste_event(event));
    if let Some(text) = clipboard_text.filter(|text| !text.is_empty()) {
        events.push(Event::Paste(text.replace("\r\n", "\n")));
    }
    true
}

pub struct App {
    cfg: Config,
    last_saved: String,
    pal: Palette,
    applied: (ThemeMode, u32),
    tab: Tab,
    sessions: Vec<Session>,
    active: usize,
    next_session_id: u64,
    capturing: Option<u64>,
    confirm: Option<(u64, u64)>,
    pending: Vec<Action>,
    focus_btn: Option<u64>,
    focus_profile: Option<u64>,
    error: Option<String>,
    fired: Vec<Hotkey>,
    skip_text: bool,
    #[cfg(windows)]
    _tray_icon: Option<tray_icon::TrayIcon>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(windows)]
        let (tray_icon, tray_error) = match crate::create_tray_icon() {
            Ok(icon) => (Some(icon), None),
            Err(error) => (
                None,
                Some(format!("Не удалось создать значок в трее: {error}")),
            ),
        };
        #[cfg(not(windows))]
        let tray_error = None;
        let cfg = Config::load();
        let pal = Palette::new(cfg.theme);
        theme::apply(&cc.egui_ctx, &pal, cfg.font_size);
        let last_saved = serde_json::to_string(&cfg).unwrap_or_default();
        let mut app = Self {
            applied: (cfg.theme, cfg.font_size.to_bits()),
            cfg,
            last_saved,
            pal,
            tab: Tab::Console,
            sessions: Vec::new(),
            active: 0,
            next_session_id: 0,
            capturing: None,
            confirm: None,
            pending: Vec::new(),
            focus_btn: None,
            focus_profile: None,
            error: tray_error,
            fired: Vec::new(),
            skip_text: false,
            #[cfg(windows)]
            _tray_icon: tray_icon,
        };
        if let Some(id) = app.cfg.open_on_start {
            app.open_profile(&cc.egui_ctx, id);
        }
        app
    }

    // ---------- sessions ----------

    fn open_profile(&mut self, ctx: &Context, pid: u64) {
        let Some(p) = self.cfg.profiles.iter().find(|p| p.id == pid).cloned() else {
            return;
        };
        self.next_session_id += 1;
        match Session::spawn(
            self.next_session_id,
            &p,
            &self.cfg.ssh_path,
            self.cfg.scrollback,
            ctx,
        ) {
            Ok(s) => {
                self.sessions.push(s);
                self.active = self.sessions.len() - 1;
                self.tab = Tab::Console;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Не удалось открыть \"{}\": {e}", p.name)),
        }
    }

    fn reconnect(&mut self, ctx: &Context, idx: usize) {
        let Some(pid) = self.sessions.get(idx).map(|s| s.profile_id) else {
            return;
        };
        let Some(p) = self.cfg.profiles.iter().find(|p| p.id == pid).cloned() else {
            self.error = Some("Профиль этого сеанса удален".into());
            return;
        };
        self.next_session_id += 1;
        match Session::spawn(
            self.next_session_id,
            &p,
            &self.cfg.ssh_path,
            self.cfg.scrollback,
            ctx,
        ) {
            Ok(s) => {
                self.sessions[idx] = s;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Не удалось переподключить: {e}")),
        }
    }

    fn close_session(&mut self, idx: usize) {
        if idx < self.sessions.len() {
            self.sessions.remove(idx);
            if self.active >= self.sessions.len() {
                self.active = self.sessions.len().saturating_sub(1);
            }
        }
    }

    fn send_to_active(&mut self, text: &str, press_enter: bool) {
        let Some(session_id) = self.sessions.get(self.active).map(|session| session.id) else {
            self.error =
                Some("Нет активного сеанса. Откройте подключение кнопкой \"+ Сеанс\".".into());
            return;
        };
        self.send_to_session(session_id, text, press_enter);
    }

    fn send_to_session(&mut self, session_id: u64, text: &str, press_enter: bool) {
        let bytes = command_bytes(text, press_enter);
        match self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            Some(s) if s.finished.is_none() => {
                s.write_bytes(&bytes);
                s.scroll = 0;
                s.selection = None;
            }
            Some(_) => self.error = Some("Сеанс уже завершен.".into()),
            None => self.error = Some("Сеанс для этой команды больше не открыт.".into()),
        }
    }

    fn pump(&mut self, ctx: &Context) {
        let mut alive = false;
        for s in &mut self.sessions {
            s.poll();
            alive |= s.finished.is_none();
        }
        if alive {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    // ---------- buttons ----------

    fn run_button(&mut self, id: u64) {
        let Some(b) = self.cfg.buttons.iter().find(|b| b.id == id) else {
            return;
        };
        if !self.button_matches_active_profile(b) {
            return;
        }
        if b.confirm {
            if let Some(session_id) = self.sessions.get(self.active).map(|session| session.id) {
                self.confirm = Some((id, session_id));
            } else {
                self.error =
                    Some("Нет активного сеанса. Откройте подключение кнопкой \"+ Сеанс\".".into());
            }
        } else {
            let (cmd, enter) = (b.command.clone(), b.press_enter);
            self.send_to_active(&cmd, enter);
        }
    }

    fn add_button(&mut self) -> u64 {
        let id = self.cfg.alloc_button_id();
        let profile_id = self.sessions.get(self.active).map(|s| s.profile_id);
        self.cfg.buttons.push(CmdButton {
            id,
            profile_id,
            label: "Новая кнопка".into(),
            command: String::new(),
            bg: SWATCHES[0],
            fg: [255, 255, 255],
            dock: Dock::Top,
            hotkey: String::new(),
            confirm: false,
            press_enter: true,
        });
        id
    }

    fn button_matches_active_profile(&self, button: &CmdButton) -> bool {
        button.profile_id.is_none()
            || self
                .sessions
                .get(self.active)
                .map(|session| session.profile_id)
                == button.profile_id
    }

    fn shift(&mut self, id: u64, dir: i32) {
        let Some(pos) = self.cfg.buttons.iter().position(|b| b.id == id) else {
            return;
        };
        let dock = self.cfg.buttons[pos].dock;
        let profile_id = self.cfg.buttons[pos].profile_id;
        let mut j = pos as i32 + dir;
        while j >= 0 && (j as usize) < self.cfg.buttons.len() {
            if self.cfg.buttons[j as usize].dock == dock
                && self.cfg.buttons[j as usize].profile_id == profile_id
            {
                self.cfg.buttons.swap(pos, j as usize);
                return;
            }
            j += dir;
        }
    }

    fn apply_pending(&mut self, ctx: &Context) {
        for a in std::mem::take(&mut self.pending) {
            match a {
                Action::Run(id) => self.run_button(id),
                Action::Edit(id) => {
                    self.tab = Tab::Buttons;
                    self.focus_btn = Some(id);
                }
                Action::Delete(id) => self.cfg.buttons.retain(|b| b.id != id),
                Action::Shift(id, d) => self.shift(id, d),
                Action::Move(id, dock) => {
                    if let Some(b) = self.cfg.buttons.iter_mut().find(|b| b.id == id) {
                        b.dock = dock;
                    }
                }
                Action::Open(pid) => self.open_profile(ctx, pid),
                Action::Close(i) => self.close_session(i),
                Action::Reconnect(i) => self.reconnect(ctx, i),
            }
        }
    }

    // ---------- input handling ----------

    fn handle_capture(&mut self, ctx: &Context) {
        let Some(id) = self.capturing else { return };
        let events = ctx.input(|i| i.events.clone());
        if events.iter().any(|e| {
            matches!(
                e,
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                }
            )
        }) {
            self.capturing = None;
            return;
        }
        if let Some(hk) = hotkey::first_key_press(&events) {
            if let Some(b) = self.cfg.buttons.iter_mut().find(|b| b.id == id) {
                b.hotkey = hk.to_string();
            }
            self.capturing = None;
        }
    }

    fn handle_keyboard_paste(&mut self, ctx: &Context) {
        if !ctx.input(|input| input.events.iter().any(is_physical_paste_event)) {
            return;
        }

        let clipboard = arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.get_text())
            .map_err(|error| error.to_string());
        ctx.input_mut(|input| {
            replace_physical_paste_events(&mut input.events, clipboard.as_deref().ok());
        });
        if let Err(error) = clipboard {
            self.error = Some(format!("Не удалось прочитать буфер обмена: {error}"));
        }
    }

    fn handle_hotkeys(&mut self, ctx: &Context) {
        self.fired.clear();
        self.skip_text = false;
        if self.capturing.is_some() || self.confirm.is_some() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let mut to_run = Vec::new();
        for ev in &events {
            if let Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = ev
            {
                for b in &self.cfg.buttons {
                    if !self.button_matches_active_profile(b) {
                        continue;
                    }
                    let Some(hk) = Hotkey::parse(&b.hotkey) else {
                        continue;
                    };
                    if !hk.matches(*key, modifiers) {
                        continue;
                    }
                    // In the console a bare letter is just typing.
                    if self.tab == Tab::Console && !hk.has_modifier() && !hk.is_function_key() {
                        continue;
                    }
                    to_run.push(b.id);
                    self.fired.push(hk);
                }
            }
        }
        if !to_run.is_empty() {
            self.skip_text = true;
        }
        for id in to_run {
            self.run_button(id);
        }
    }

    fn sync_theme(&mut self, ctx: &Context) {
        let key = (self.cfg.theme, self.cfg.font_size.to_bits());
        if key != self.applied {
            self.pal = Palette::new(self.cfg.theme);
            theme::apply(ctx, &self.pal, self.cfg.font_size);
            self.applied = key;
        }
    }

    fn autosave(&mut self, ctx: &Context) {
        let active = ctx.input(|i| !i.events.is_empty() || i.pointer.any_released());
        if !active {
            return;
        }
        if let Ok(s) = serde_json::to_string(&self.cfg) {
            if s != self.last_saved {
                self.cfg.save();
                self.last_saved = s;
            }
        }
    }

    // ---------- header ----------

    fn header(&mut self, ctx: &Context) {
        let pal = self.pal;
        egui::TopBottomPanel::top("header")
            .frame(
                Frame::none()
                    .fill(pal.panel)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .inner_margin(Margin::symmetric(16.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Better Console")
                            .size(19.0)
                            .strong()
                            .color(pal.accent),
                    );
                    ui.add_space(10.0);
                    if let Some(s) = self.sessions.get(self.active) {
                        let (text, color) = match s.finished {
                            Some(code) => (format!("{}: завершен ({code})", s.title), pal.muted),
                            None => (format!("{}: подключено", s.title), pal.ok),
                        };
                        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 5.0, color);
                        ui.label(RichText::new(text).color(pal.muted));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let icon = if pal.dark { "☀" } else { "🌙" };
                        let btn = egui::Button::new(RichText::new(icon).size(17.0))
                            .min_size(vec2(36.0, 32.0));
                        if ui.add(btn).on_hover_text("Светлая / темная тема").clicked()
                        {
                            self.cfg.theme = match self.cfg.theme {
                                ThemeMode::Light => ThemeMode::Dark,
                                ThemeMode::Dark => ThemeMode::Light,
                            };
                        }
                        ui.add_space(8.0);
                        for (tab, title) in [
                            (Tab::Settings, "Настройки"),
                            (Tab::Profiles, "Подключения"),
                            (Tab::Buttons, "Кнопки"),
                            (Tab::Console, "Консоль"),
                        ] {
                            ui.selectable_value(
                                &mut self.tab,
                                tab,
                                RichText::new(title).size(14.5),
                            );
                        }
                    });
                });
            });
    }

    // ---------- docks and buttons ----------

    fn draw_buttons(&mut self, ui: &mut Ui, dock: Dock, fill: bool) {
        let dark = self.pal.dark;
        for b in &self.cfg.buttons {
            if b.dock != dock || !self.button_matches_active_profile(b) {
                continue;
            }
            let mut tip = if b.command.trim().is_empty() {
                "(команда не задана)".to_string()
            } else {
                b.command.clone()
            };
            if !b.hotkey.is_empty() {
                tip.push_str(&format!("\nКлавиша: {}", b.hotkey));
            }
            let resp =
                color_button(ui, &b.label, rgb(b.bg), rgb(b.fg), fill, dark).on_hover_text(tip);
            let id = b.id;
            if resp.clicked() {
                self.pending.push(Action::Run(id));
            }
            let pending = &mut self.pending;
            resp.context_menu(|ui| {
                if ui.button("Выполнить").clicked() {
                    pending.push(Action::Run(id));
                    ui.close_menu();
                }
                if ui.button("Изменить").clicked() {
                    pending.push(Action::Edit(id));
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Сдвинуть назад").clicked() {
                    pending.push(Action::Shift(id, -1));
                    ui.close_menu();
                }
                if ui.button("Сдвинуть вперед").clicked() {
                    pending.push(Action::Shift(id, 1));
                    ui.close_menu();
                }
                ui.menu_button("Переместить в", |ui| {
                    for d in Dock::ALL {
                        if ui.button(d.title()).clicked() {
                            pending.push(Action::Move(id, d));
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                if ui
                    .button(RichText::new("Удалить").color(Color32::from_rgb(217, 45, 58)))
                    .clicked()
                {
                    pending.push(Action::Delete(id));
                    ui.close_menu();
                }
            });
        }
    }

    fn has_buttons(&self, dock: Dock) -> bool {
        self.cfg
            .buttons
            .iter()
            .any(|b| b.dock == dock && self.button_matches_active_profile(b))
    }

    fn dock_panels(&mut self, ctx: &Context) {
        let pal = self.pal;
        egui::TopBottomPanel::top("dock_top")
            .frame(Frame::none().fill(pal.bg).inner_margin(Margin {
                left: 16.0,
                right: 16.0,
                top: 12.0,
                bottom: 0.0,
            }))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    self.draw_buttons(ui, Dock::Top, false);
                    if ui
                        .add(
                            egui::Button::new(RichText::new("+ Кнопка").color(pal.muted))
                                .min_size(vec2(0.0, 34.0)),
                        )
                        .on_hover_text("Добавить кнопку")
                        .clicked()
                    {
                        let id = self.add_button();
                        self.focus_btn = Some(id);
                        self.tab = Tab::Buttons;
                    }
                });
            });

        if self.has_buttons(Dock::Bottom) {
            egui::TopBottomPanel::bottom("dock_bottom")
                .frame(
                    Frame::none()
                        .fill(pal.panel)
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .inner_margin(Margin::symmetric(16.0, 10.0)),
                )
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| self.draw_buttons(ui, Dock::Bottom, false));
                });
        }

        for (dock, id) in [(Dock::Left, "dock_left"), (Dock::Right, "dock_right")] {
            if !self.has_buttons(dock) {
                continue;
            }
            let panel = if dock == Dock::Left {
                egui::SidePanel::left(id)
            } else {
                egui::SidePanel::right(id)
            };
            panel
                .resizable(true)
                .default_width(170.0)
                .width_range(110.0..=360.0)
                .frame(
                    Frame::none()
                        .fill(pal.bg)
                        .inner_margin(Margin::symmetric(14.0, 14.0)),
                )
                .show(ctx, |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.draw_buttons(ui, dock, true);
                        });
                });
        }
    }

    // ---------- console page ----------

    fn session_tabs(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        ui.horizontal_wrapped(|ui| {
            for i in 0..self.sessions.len() {
                let (title, accent, done, cmdline) = {
                    let s = &self.sessions[i];
                    (
                        s.title.clone(),
                        s.accent,
                        s.finished.is_some(),
                        s.cmdline.clone(),
                    )
                };
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(
                    r.center(),
                    5.0,
                    if done { pal.muted } else { rgb(accent) },
                );
                let resp = ui
                    .selectable_label(i == self.active, RichText::new(&title).size(14.5))
                    .on_hover_text(cmdline);
                if resp.clicked() {
                    self.active = i;
                }
                if resp.middle_clicked() {
                    self.pending.push(Action::Close(i));
                }
                if ui
                    .small_button("x")
                    .on_hover_text("Закрыть сеанс")
                    .clicked()
                {
                    self.pending.push(Action::Close(i));
                }
                ui.add_space(8.0);
            }
            ui.menu_button("+ Сеанс", |ui| {
                for p in &self.cfg.profiles {
                    if ui.button(&p.name).clicked() {
                        self.pending.push(Action::Open(p.id));
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button("Настроить подключения...").clicked() {
                    self.tab = Tab::Profiles;
                    ui.close_menu();
                }
            });
        });
    }

    fn console_page(&mut self, ctx: &Context) {
        let pal = self.pal;
        egui::CentralPanel::default()
            .frame(Frame::none().fill(pal.bg).inner_margin(Margin::same(16.0)))
            .show(ctx, |ui| {
                self.session_tabs(ui);
                if let Some(err) = self.error.clone() {
                    ui.add_space(4.0);
                    Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(
                            pal.warn.r(),
                            pal.warn.g(),
                            pal.warn.b(),
                            40,
                        ))
                        .stroke(Stroke::new(1.0_f32, pal.warn))
                        .rounding(10.0)
                        .inner_margin(Margin::symmetric(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(err).color(pal.text));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.small_button("x").clicked() {
                                        self.error = None;
                                    }
                                });
                            });
                        });
                }
                ui.add_space(6.0);

                if self.sessions.is_empty() {
                    self.empty_state(ui);
                    return;
                }
                let active = self.active.min(self.sessions.len() - 1);
                self.active = active;
                let mut term_rect = None;
                let mut terminal_error = None;
                Frame::none()
                    .fill(pal.card)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(14.0)
                    .inner_margin(Margin::same(12.0))
                    .shadow(egui::epaint::Shadow {
                        offset: vec2(0.0, 3.0),
                        blur: 14.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(if pal.dark { 60 } else { 16 }),
                    })
                    .show(ui, |ui| {
                        let o = terminal::Opts {
                            pal: &pal,
                            font_size: self.cfg.font_size,
                            highlight: self.cfg.highlight,
                            fired: &self.fired,
                            skip_text: self.skip_text,
                        };
                        let (rect, error) = terminal::show(ui, &mut self.sessions[active], &o);
                        term_rect = Some(rect);
                        terminal_error = error;
                    });
                if let Some(error) = terminal_error {
                    self.error = Some(format!("Не удалось прочитать буфер обмена: {error}"));
                }

                if let (Some(rect), Some(code)) = (term_rect, self.sessions[active].finished) {
                    let pos = rect.center_bottom() - vec2(150.0, 90.0);
                    egui::Area::new(Id::new("session_done"))
                        .fixed_pos(pos)
                        .order(egui::Order::Foreground)
                        .show(ctx, |ui| {
                            Frame::popup(ui.style()).show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("Сеанс завершен (код {code})")).strong(),
                                );
                                ui.horizontal(|ui| {
                                    if color_button(
                                        ui,
                                        "Переподключить",
                                        pal.accent,
                                        Color32::WHITE,
                                        false,
                                        pal.dark,
                                    )
                                    .clicked()
                                    {
                                        self.pending.push(Action::Reconnect(active));
                                    }
                                    if ui.button("Закрыть").clicked() {
                                        self.pending.push(Action::Close(active));
                                    }
                                });
                            });
                        });
                }
            });
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Нет открытых сеансов")
                    .size(20.0)
                    .strong()
                    .color(pal.text),
            );
            ui.add_space(4.0);
            ui.label(RichText::new("Выберите подключение:").color(pal.muted));
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                for p in &self.cfg.profiles {
                    if color_button(
                        ui,
                        &p.name,
                        rgb(p.color),
                        rgb(contrast(p.color)),
                        false,
                        pal.dark,
                    )
                    .clicked()
                    {
                        self.pending.push(Action::Open(p.id));
                    }
                }
            });
            ui.add_space(10.0);
            if ui.button("Настроить подключения").clicked() {
                self.tab = Tab::Profiles;
            }
        });
    }

    // ---------- profiles ----------

    fn profiles_page(&mut self, ctx: &Context) {
        let pal = self.pal;
        let ssh_path = self.cfg.ssh_path.clone();
        let focus = self.focus_profile.take();
        let mut add = false;
        let mut remove = None;
        let mut open = None;

        egui::CentralPanel::default()
            .frame(Frame::none().fill(pal.bg).inner_margin(Margin::same(18.0)))
            .show(ctx, |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(860.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Подключения").size(22.0).strong().color(pal.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if color_button(ui, "+ Добавить подключение", pal.accent, Color32::WHITE, false, pal.dark).clicked() {
                                add = true;
                            }
                        });
                    });
                    ui.label(
                        RichText::new(
                            "SSH работает через системный ssh: ключи, ssh-agent и ~/.ssh/config действуют как обычно. \
                             Пароль вводится прямо в терминале и нигде не сохраняется.",
                        )
                        .color(pal.muted),
                    );
                    ui.add_space(8.0);

                    let n = self.cfg.profiles.len();
                    for i in 0..n {
                        let p = &mut self.cfg.profiles[i];
                        let id = p.id;
                        card(ui, &pal, |ui| {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                                ui.painter().circle_filled(r.center(), 6.0, rgb(p.color));
                                ui.label(RichText::new(&p.name).size(16.0).strong());
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(RichText::new("Удалить").color(pal.err)).clicked() {
                                        remove = Some(id);
                                    }
                                    if color_button(ui, "Открыть", pal.ok, Color32::WHITE, false, pal.dark).clicked() {
                                        open = Some(id);
                                    }
                                });
                            });
                            ui.add_space(4.0);
                            egui::Grid::new(("profile_grid", id)).num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                                ui.label("Название");
                                ui.add(TextEdit::singleline(&mut p.name).desired_width(f32::INFINITY));
                                ui.end_row();

                                ui.label("Тип");
                                egui::ComboBox::from_id_source(("kind", id)).selected_text(p.kind.title()).show_ui(ui, |ui| {
                                    for k in ProfileKind::ALL {
                                        ui.selectable_value(&mut p.kind, k, k.title());
                                    }
                                });
                                ui.end_row();

                                if p.kind == ProfileKind::Ssh {
                                    ui.label("Сервер");
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            TextEdit::singleline(&mut p.host).hint_text("example.com или 192.168.1.10").desired_width(300.0),
                                        );
                                        ui.label("порт");
                                        ui.add(egui::DragValue::new(&mut p.port).clamp_range(1..=65535));
                                    });
                                    ui.end_row();

                                    ui.label("Пользователь");
                                    ui.add(TextEdit::singleline(&mut p.user).hint_text("root").desired_width(300.0));
                                    ui.end_row();

                                    ui.label("Файл ключа");
                                    ui.add(
                                        TextEdit::singleline(&mut p.identity)
                                            .hint_text("необязательно, например ~/.ssh/id_ed25519")
                                            .desired_width(f32::INFINITY),
                                    );
                                    ui.end_row();

                                    ui.label("Доп. параметры ssh");
                                    ui.add(
                                        TextEdit::singleline(&mut p.extra_args)
                                            .hint_text("например: -J bastion.example.com -L 8080:localhost:80")
                                            .desired_width(f32::INFINITY),
                                    );
                                    ui.end_row();
                                }
                                if p.kind == ProfileKind::Custom {
                                    ui.label("Команда");
                                    ui.add(
                                        TextEdit::singleline(&mut p.custom_cmd)
                                            .font(TextStyle::Monospace)
                                            .hint_text("например: wsl.exe -d Ubuntu")
                                            .desired_width(f32::INFINITY),
                                    );
                                    ui.end_row();
                                }

                                ui.label("Рабочая папка");
                                ui.add(
                                    TextEdit::singleline(&mut p.work_dir)
                                        .hint_text("пусто = домашняя")
                                        .desired_width(f32::INFINITY),
                                );
                                ui.end_row();

                                ui.label("Цвет вкладки");
                                ui.horizontal(|ui| {
                                    let mut c = p.color;
                                    if ui.color_edit_button_srgb(&mut c).changed() {
                                        p.color = c;
                                    }
                                    for sw in SWATCHES {
                                        let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                                        ui.painter().circle_filled(r.center(), 10.0, rgb(sw));
                                        if p.color == sw {
                                            ui.painter().circle_stroke(r.center(), 11.5, Stroke::new(2.0_f32, pal.text));
                                        }
                                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                            p.color = sw;
                                        }
                                    }
                                });
                                ui.end_row();

                                ui.label("Открывать при запуске");
                                let mut on = self.cfg.open_on_start == Some(id);
                                if ui.checkbox(&mut on, "автоматически подключаться при старте").changed() {
                                    self.cfg.open_on_start = if on { Some(id) } else { None };
                                }
                                ui.end_row();

                                ui.label("Будет запущено");
                                let preview = match build_command(p, &ssh_path) {
                                    Ok((prog, args)) => format!("{prog} {}", args.join(" ")),
                                    Err(e) => format!("({e})"),
                                };
                                ui.label(RichText::new(preview).monospace().color(pal.muted));
                                ui.end_row();
                            });
                        });
                        if focus == Some(id) {
                            ui.scroll_to_cursor(Some(Align::Center));
                        }
                        ui.add_space(8.0);
                    }
                });
            });

        if add {
            let id = self.cfg.alloc_profile_id();
            let mut p = Profile::new(id, "Новое подключение", ProfileKind::Ssh, SWATCHES[4]);
            p.user = "root".into();
            self.cfg.profiles.push(p);
            self.focus_profile = Some(id);
        }
        if let Some(id) = remove {
            self.cfg.profiles.retain(|p| p.id != id);
            for button in &mut self.cfg.buttons {
                if button.profile_id == Some(id) {
                    button.profile_id = None;
                }
            }
            if self.cfg.open_on_start == Some(id) {
                self.cfg.open_on_start = None;
            }
        }
        if let Some(id) = open {
            self.pending.push(Action::Open(id));
        }
    }

    // ---------- buttons editor ----------

    fn buttons_page(&mut self, ctx: &Context) {
        let pal = self.pal;
        let hotkeys: Vec<(u64, Option<u64>, String)> = self
            .cfg
            .buttons
            .iter()
            .map(|b| (b.id, b.profile_id, b.hotkey.clone()))
            .collect();
        let button_profiles: Vec<(u64, String)> = self
            .cfg
            .profiles
            .iter()
            .map(|profile| (profile.id, profile.name.clone()))
            .collect();
        let mut remove = None;
        let mut shift = None;
        let mut add = false;
        let focus = self.focus_btn.take();

        egui::CentralPanel::default()
            .frame(Frame::none().fill(pal.bg).inner_margin(Margin::same(18.0)))
            .show(ctx, |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(860.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Кнопки").size(22.0).strong().color(pal.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if color_button(ui, "+ Добавить кнопку", pal.accent, Color32::WHITE, false, pal.dark).clicked() {
                                add = true;
                            }
                        });
                    });
                    ui.label(
                        RichText::new(
                            "Каждая строка команды набирается в активном сеансе и отправляется по Enter. \
                             Выберите подключение для каждой кнопки или оставьте ее общей. Ctrl+V вставляет текст в поля. \
                             Макросы: {ctrl+c}, {ctrl+d}, {enter}, {tab}, {esc}, {up}, {down}.",
                        )
                        .color(pal.muted),
                    );
                    ui.add_space(8.0);

                    if self.cfg.buttons.is_empty() {
                        ui.add_space(20.0);
                        ui.label(RichText::new("Кнопок пока нет. Нажмите \"Добавить кнопку\".").color(pal.muted));
                    }

                    let capturing = self.capturing;
                    let mut start_capture = None;
                    let n = self.cfg.buttons.len();
                    for i in 0..n {
                        let b = &mut self.cfg.buttons[i];
                        let id = b.id;
                        let dup = !b.hotkey.is_empty()
                            && hotkeys.iter().any(|(other_id, other_profile, hotkey)| {
                                *other_id != id
                                    && *hotkey == b.hotkey
                                    && (other_profile.is_none()
                                        || b.profile_id.is_none()
                                        || *other_profile == b.profile_id)
                            });
                        card(ui, &pal, |ui| {
                            ui.horizontal(|ui| {
                                color_button(ui, &b.label, rgb(b.bg), rgb(b.fg), false, pal.dark);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button(RichText::new("Удалить").color(pal.err)).clicked() {
                                        remove = Some(id);
                                    }
                                    if ui.button("Ниже").clicked() {
                                        shift = Some((id, 1));
                                    }
                                    if ui.button("Выше").clicked() {
                                        shift = Some((id, -1));
                                    }
                                });
                            });
                            ui.add_space(4.0);
                            egui::Grid::new(("btn_grid", id)).num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                                ui.label("Название");
                                ui.add(TextEdit::singleline(&mut b.label).desired_width(f32::INFINITY));
                                ui.end_row();

                                ui.label("Команда");
                                ui.add(
                                    TextEdit::multiline(&mut b.command)
                                        .font(TextStyle::Monospace)
                                        .desired_rows(2)
                                        .hint_text("например: systemctl status nginx")
                                        .desired_width(f32::INFINITY),
                                );
                                ui.end_row();

                                ui.label("Профиль");
                                let profile_name = b
                                    .profile_id
                                    .and_then(|profile_id| {
                                        button_profiles
                                            .iter()
                                            .find(|(id, _)| *id == profile_id)
                                            .map(|(_, name)| name.as_str())
                                    })
                                    .unwrap_or("Все Профили");
                                egui::ComboBox::from_id_source(("button_profile", id))
                                    .selected_text(profile_name)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut b.profile_id,
                                            None,
                                            "Все профили",
                                        );
                                        for (profile_id, name) in &button_profiles {
                                            ui.selectable_value(
                                                &mut b.profile_id,
                                                Some(*profile_id),
                                                name,
                                            );
                                        }
                                    });
                                ui.end_row();

                                ui.label("Цвет кнопки");
                                ui.horizontal(|ui| {
                                    let mut c = b.bg;
                                    if ui.color_edit_button_srgb(&mut c).changed() {
                                        b.bg = c;
                                    }
                                    for sw in SWATCHES {
                                        let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                                        ui.painter().circle_filled(r.center(), 10.0, rgb(sw));
                                        if b.bg == sw {
                                            ui.painter().circle_stroke(r.center(), 11.5, Stroke::new(2.0_f32, pal.text));
                                        }
                                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                            b.bg = sw;
                                            b.fg = contrast(sw);
                                        }
                                    }
                                });
                                ui.end_row();

                                ui.label("Цвет текста");
                                ui.horizontal(|ui| {
                                    let mut c = b.fg;
                                    if ui.color_edit_button_srgb(&mut c).changed() {
                                        b.fg = c;
                                    }
                                    if ui.button("Авто").on_hover_text("Подобрать читаемый цвет").clicked() {
                                        b.fg = contrast(b.bg);
                                    }
                                    if ui.button("Белый").clicked() {
                                        b.fg = to_rgb(Color32::WHITE);
                                    }
                                    if ui.button("Темный").clicked() {
                                        b.fg = [31, 35, 40];
                                    }
                                });
                                ui.end_row();

                                ui.label("Место в интерфейсе");
                                egui::ComboBox::from_id_source(("dock", id)).selected_text(b.dock.title()).show_ui(ui, |ui| {
                                    for d in Dock::ALL {
                                        ui.selectable_value(&mut b.dock, d, d.title());
                                    }
                                });
                                ui.end_row();

                                ui.label("Горячая клавиша");
                                ui.horizontal(|ui| {
                                    let waiting = capturing == Some(id);
                                    let text = if waiting {
                                        "Нажмите сочетание (Esc - отмена)".to_string()
                                    } else if b.hotkey.is_empty() {
                                        "Назначить".to_string()
                                    } else {
                                        b.hotkey.clone()
                                    };
                                    if ui.add(egui::Button::new(text).selected(waiting)).clicked() {
                                        start_capture = Some(id);
                                    }
                                    if !b.hotkey.is_empty() && ui.button("Сбросить").clicked() {
                                        b.hotkey.clear();
                                    }
                                    if dup {
                                        ui.label(RichText::new("уже занято другой кнопкой").color(pal.warn));
                                    }
                                });
                                ui.end_row();

                                ui.label("Параметры");
                                ui.vertical(|ui| {
                                    ui.checkbox(&mut b.press_enter, "нажимать Enter после каждой строки");
                                    ui.checkbox(&mut b.confirm, "спрашивать подтверждение перед отправкой");
                                });
                                ui.end_row();
                            });
                            ui.label(
                                RichText::new("Клавиши без Ctrl/Alt (кроме F1-F12) в консоли не срабатывают: это обычный ввод.")
                                    .size(12.0)
                                    .color(pal.muted),
                            );
                        });
                        if focus == Some(id) {
                            ui.scroll_to_cursor(Some(Align::Center));
                        }
                        ui.add_space(8.0);
                    }
                    if let Some(id) = start_capture {
                        self.capturing = Some(id);
                    }
                });
            });

        if add {
            let id = self.add_button();
            self.focus_btn = Some(id);
        }
        if let Some((id, d)) = shift {
            self.shift(id, d);
        }
        if let Some(id) = remove {
            self.cfg.buttons.retain(|b| b.id != id);
        }
    }

    // ---------- settings ----------

    fn settings_page(&mut self, ctx: &Context) {
        let pal = self.pal;
        let mut reset = false;
        egui::CentralPanel::default()
            .frame(Frame::none().fill(pal.bg).inner_margin(Margin::same(18.0)))
            .show(ctx, |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(860.0);
                    ui.label(RichText::new("Настройки").size(22.0).strong().color(pal.text));
                    ui.add_space(8.0);

                    card(ui, &pal, |ui| {
                        section_title(ui, &pal, "Оформление");
                        ui.horizontal(|ui| {
                            ui.label("Тема");
                            ui.selectable_value(&mut self.cfg.theme, ThemeMode::Light, "Светлая");
                            ui.selectable_value(&mut self.cfg.theme, ThemeMode::Dark, "Темная");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Размер шрифта терминала");
                            ui.add(egui::Slider::new(&mut self.cfg.font_size, 11.0..=24.0).step_by(0.5));
                        });
                        ui.checkbox(&mut self.cfg.highlight, "Подсвечивать вывод (пути, IP, ссылки, ошибки, числа, уровни логов)");
                        ui.label(
                            RichText::new("Подсветка добавляется только к тексту без собственных цветов и не трогает программы на весь экран (vim, htop).")
                                .size(12.5)
                                .color(pal.muted),
                        );
                    });
                    ui.add_space(10.0);

                    card(ui, &pal, |ui| {
                        section_title(ui, &pal, "Терминал");
                        ui.horizontal(|ui| {
                            ui.label("Строк истории");
                            ui.add(egui::Slider::new(&mut self.cfg.scrollback, 1_000..=100_000).logarithmic(true));
                        });
                        ui.label(RichText::new("Применяется к новым сеансам.").size(12.5).color(pal.muted));
                        ui.horizontal(|ui| {
                            ui.label("Путь к ssh");
                            ui.add(
                                TextEdit::singleline(&mut self.cfg.ssh_path)
                                    .hint_text("пусто = ssh из PATH (в Windows: клиент OpenSSH)")
                                    .desired_width(f32::INFINITY),
                            );
                        });
                    });
                    ui.add_space(10.0);

                    card(ui, &pal, |ui| {
                        section_title(ui, &pal, "Прочее");
                        if ui.button("Сбросить кнопки по умолчанию").clicked() {
                            reset = true;
                        }
                        ui.label(
                            RichText::new(format!("Файл настроек: {}", Config::path().display())).color(pal.muted).size(12.5),
                        );
                    });
                });
            });
        if reset {
            self.cfg.reset_buttons();
        }
    }

    fn confirm_modal(&mut self, ctx: &Context) {
        let Some((id, session_id)) = self.confirm else {
            return;
        };
        let Some(b) = self.cfg.buttons.iter().find(|b| b.id == id) else {
            self.confirm = None;
            return;
        };
        let (label, command, enter) = (b.label.clone(), b.command.clone(), b.press_enter);
        let pal = self.pal;
        let mut decision = None;
        egui::Window::new("Подтвердите действие")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.label(
                    RichText::new(format!("Выполнить \"{label}\"?"))
                        .size(16.0)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(RichText::new(command.clone()).monospace().color(pal.muted));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if color_button(ui, "Выполнить", pal.err, Color32::WHITE, false, pal.dark)
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button("Отмена").clicked() {
                        decision = Some(false);
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            decision = Some(false);
        }
        match decision {
            Some(true) => {
                self.confirm = None;
                self.send_to_session(session_id, &command, enter);
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.pump(ctx);
        self.handle_keyboard_paste(ctx);
        self.handle_capture(ctx);
        self.handle_hotkeys(ctx);
        self.sync_theme(ctx);

        self.header(ctx);
        match self.tab {
            Tab::Console => {
                self.dock_panels(ctx);
                self.console_page(ctx);
            }
            Tab::Buttons => self.buttons_page(ctx),
            Tab::Profiles => self.profiles_page(ctx),
            Tab::Settings => self.settings_page(ctx),
        }
        self.confirm_modal(ctx);
        self.apply_pending(ctx);
        self.autosave(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for s in &mut self.sessions {
            s.kill();
        }
        self.cfg.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Modifiers;

    #[test]
    fn ctrl_v_uses_the_physical_key_across_keyboard_layouts() {
        let event = Event::Key {
            key: Key::M,
            physical_key: Some(Key::V),
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                command: true,
                ..Modifiers::NONE
            },
        };
        assert!(is_physical_paste_event(&event));
    }

    #[test]
    fn ctrl_alt_v_is_not_treated_as_paste() {
        let event = Event::Key {
            key: Key::V,
            physical_key: Some(Key::V),
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                command: true,
                alt: true,
                ..Modifiers::NONE
            },
        };
        assert!(!is_physical_paste_event(&event));
    }

    #[test]
    fn physical_paste_becomes_a_standard_paste_event() {
        let mut events = vec![Event::Key {
            key: Key::M,
            physical_key: Some(Key::V),
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                command: true,
                ..Modifiers::NONE
            },
        }];

        assert!(replace_physical_paste_events(
            &mut events,
            Some("first\r\nsecond")
        ));
        assert_eq!(events, vec![Event::Paste("first\nsecond".into())]);
    }
}
