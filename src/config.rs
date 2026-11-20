use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeMode {
    Light,
    Dark,
}

/// Where a button lives in the interface.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dock {
    Top,
    Left,
    Right,
    Bottom,
}

impl Dock {
    pub const ALL: [Dock; 4] = [Dock::Top, Dock::Left, Dock::Right, Dock::Bottom];

    pub fn title(self) -> &'static str {
        match self {
            Dock::Top => "Сверху",
            Dock::Left => "Слева",
            Dock::Right => "Справа",
            Dock::Bottom => "Снизу",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProfileKind {
    PowerShell,
    Pwsh,
    Cmd,
    Shell,
    Ssh,
    Custom,
}

impl ProfileKind {
    pub const ALL: [ProfileKind; 6] = [
        ProfileKind::Ssh,
        ProfileKind::PowerShell,
        ProfileKind::Pwsh,
        ProfileKind::Cmd,
        ProfileKind::Shell,
        ProfileKind::Custom,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ProfileKind::Custom => "Своя команда",
            ProfileKind::Ssh => "SSH",
            ProfileKind::PowerShell => "Windows PowerShell",
            ProfileKind::Pwsh => "PowerShell 7 (pwsh)",
            ProfileKind::Cmd => "cmd",
            ProfileKind::Shell => "Системная оболочка (bash/zsh)",
        }
    }
}

fn yes() -> bool {
    true
}

fn default_port() -> u16 {
    22
}

/// A saved connection: local shell or SSH host.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Profile {
    pub id: u64,
    pub name: String,
    pub kind: ProfileKind,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    /// Path to a private key (optional).
    pub identity: String,
    /// Extra ssh arguments, e.g. `-J jump.example.com`.
    pub extra_args: String,
    /// Full command line for `ProfileKind::Custom`.
    pub custom_cmd: String,
    pub work_dir: String,
    /// Accent color of the session tab.
    pub color: [u8; 3],
}

impl Profile {
    pub fn new(id: u64, name: &str, kind: ProfileKind, color: [u8; 3]) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            host: String::new(),
            port: 22,
            user: String::new(),
            identity: String::new(),
            extra_args: String::new(),
            custom_cmd: String::new(),
            work_dir: String::new(),
            color,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct CmdButton {
    pub id: u64,
    /// Connection profile this button belongs to. `None` keeps legacy/global buttons available everywhere.
    #[serde(default)]
    pub profile_id: Option<u64>,
    pub label: String,
    /// One command per line. Lines may contain macros like `{ctrl+c}`, `{enter}`, `{tab}`, `{esc}`.
    pub command: String,
    pub bg: [u8; 3],
    pub fg: [u8; 3],
    pub dock: Dock,
    /// Hotkey as text, e.g. "Alt+1" or "F5". Empty means none.
    pub hotkey: String,
    /// Ask for confirmation before sending.
    pub confirm: bool,
    /// Press Enter after every line.
    #[serde(default = "yes")]
    pub press_enter: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Config {
    pub theme: ThemeMode,
    pub font_size: f32,
    pub scrollback: usize,
    /// Extra syntax highlighting of plain (uncolored) terminal output.
    pub highlight: bool,
    /// Path to the ssh client; empty means `ssh` from PATH.
    pub ssh_path: String,
    pub profiles: Vec<Profile>,
    /// Profile opened on startup.
    pub open_on_start: Option<u64>,
    pub buttons: Vec<CmdButton>,
}

fn button(
    id: u64,
    profile_id: Option<u64>,
    label: &str,
    command: &str,
    bg: [u8; 3],
    hotkey: &str,
) -> CmdButton {
    CmdButton {
        id,
        profile_id,
        label: label.into(),
        command: command.into(),
        bg,
        fg: [255, 255, 255],
        dock: Dock::Top,
        hotkey: hotkey.into(),
        confirm: false,
        press_enter: true,
    }
}

fn default_buttons() -> Vec<CmdButton> {
    vec![
        button(
            1,
            Some(1),
            "Прервать (Ctrl+C)",
            "{ctrl+c}",
            [229, 72, 77],
            "F9",
        ),
        button(
            2,
            Some(1),
            "Очистить",
            if cfg!(windows) { "Clear-Host" } else { "clear" },
            [100, 116, 139],
            "F2",
        ),
        button(3, Some(1), "Кто я", "whoami", [79, 107, 237], "F3"),
        button(4, Some(1), "Имя хоста", "hostname", [46, 158, 91], "F4"),
        button(
            5,
            Some(2),
            "Прервать (Ctrl+C)",
            "{ctrl+c}",
            [229, 72, 77],
            "F9",
        ),
        button(6, Some(2), "Очистить", "clear", [100, 116, 139], "F2"),
        button(7, Some(2), "Кто я", "whoami", [79, 107, 237], "F3"),
        button(8, Some(2), "Имя хоста", "hostname", [46, 158, 91], "F4"),
    ]
}

fn legacy_default_buttons() -> Vec<CmdButton> {
    vec![
        button(
            1,
            None,
            "Прервать (Ctrl+C)",
            "{ctrl+c}",
            [229, 72, 77],
            "F9",
        ),
        button(2, None, "Очистить", "clear", [100, 116, 139], "F2"),
        button(3, None, "Кто я", "whoami", [79, 107, 237], "F3"),
        button(4, None, "Имя хоста", "hostname", [46, 158, 91], "F4"),
    ]
}

fn migrate_legacy_buttons(config: &mut Config) -> bool {
    if config.buttons == legacy_default_buttons() {
        config.buttons = default_buttons();
        true
    } else {
        false
    }
}

impl Default for Config {
    fn default() -> Self {
        let local = if cfg!(windows) {
            Profile::new(1, "PowerShell", ProfileKind::PowerShell, [79, 107, 237])
        } else {
            Profile::new(1, "Терминал", ProfileKind::Shell, [79, 107, 237])
        };
        let mut ssh = Profile::new(2, "Мой сервер (SSH)", ProfileKind::Ssh, [46, 158, 91]);
        ssh.host = "example.com".into();
        ssh.user = "root".into();
        Self {
            theme: ThemeMode::Light,
            font_size: 14.0,
            scrollback: 10_000,
            highlight: true,
            ssh_path: String::new(),
            profiles: vec![local, ssh],
            open_on_start: Some(1),
            buttons: default_buttons(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("server-console")
            .join("config.json")
    }

    pub fn load() -> Self {
        let mut config = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if migrate_legacy_buttons(&mut config) {
            config.save();
        }
        config
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }

    pub fn alloc_button_id(&self) -> u64 {
        self.buttons.iter().map(|b| b.id + 1).max().unwrap_or(1)
    }

    pub fn alloc_profile_id(&self) -> u64 {
        self.profiles.iter().map(|p| p.id + 1).max().unwrap_or(1)
    }

    pub fn reset_buttons(&mut self) {
        self.buttons = default_buttons();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_buttons_are_scoped_to_connection_profiles() {
        let config = Config::default();
        assert_eq!(config.buttons.len(), 8);
        assert!(config.buttons[..4]
            .iter()
            .all(|button| button.profile_id == Some(1)));
        assert!(config.buttons[4..]
            .iter()
            .all(|button| button.profile_id == Some(2)));
        assert_eq!(
            config.buttons[1].command,
            if cfg!(windows) { "Clear-Host" } else { "clear" }
        );
        assert_eq!(config.buttons[5].command, "clear");
    }

    #[test]
    fn migrates_the_previous_unscoped_defaults() {
        let mut config = Config::default();
        config.buttons = legacy_default_buttons();

        assert!(migrate_legacy_buttons(&mut config));
        assert!(config.buttons == default_buttons());
    }

    #[test]
    fn old_button_json_defaults_to_a_global_scope() {
        let mut value = serde_json::to_value(button(1, None, "test", "echo", [1, 2, 3], ""))
            .expect("serialize button");
        value
            .as_object_mut()
            .expect("button object")
            .remove("profile_id");

        let button: CmdButton = serde_json::from_value(value).expect("deserialize legacy button");
        assert_eq!(button.profile_id, None);
    }
}
