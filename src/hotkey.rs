use eframe::egui::{Event, Key, Modifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

impl Hotkey {
    pub fn parse(s: &str) -> Option<Self> {
        let mut hk = Hotkey {
            ctrl: false,
            alt: false,
            shift: false,
            key: Key::A,
        };
        let mut key = None;
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => hk.ctrl = true,
                "alt" => hk.alt = true,
                "shift" => hk.shift = true,
                _ => key = Key::from_name(part),
            }
        }
        hk.key = key?;
        Some(hk)
    }

    pub fn from_event(key: Key, m: &Modifiers) -> Self {
        Hotkey {
            ctrl: m.ctrl || m.command || m.mac_cmd,
            alt: m.alt,
            shift: m.shift,
            key,
        }
    }

    pub fn matches(&self, key: Key, m: &Modifiers) -> bool {
        *self == Self::from_event(key, m)
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt
    }

    pub fn is_function_key(&self) -> bool {
        self.key.name().starts_with('F')
            && self.key.name().len() <= 3
            && self.key.name()[1..].chars().all(|c| c.is_ascii_digit())
    }
}

impl std::fmt::Display for Hotkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ctrl {
            write!(f, "Ctrl+")?;
        }
        if self.alt {
            write!(f, "Alt+")?;
        }
        if self.shift {
            write!(f, "Shift+")?;
        }
        write!(f, "{}", self.key.name())
    }
}

/// First key press in this frame's events, if any.
pub fn first_key_press(events: &[Event]) -> Option<Hotkey> {
    events.iter().find_map(|e| match e {
        Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } => Some(Hotkey::from_event(*key, modifiers)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for s in ["Ctrl+S", "F5", "Ctrl+Shift+X", "Alt+1", "Ctrl+Enter"] {
            let hk = Hotkey::parse(s).unwrap_or_else(|| panic!("parse {s}"));
            assert_eq!(hk.to_string(), s);
        }
        assert!(Hotkey::parse("F5").unwrap().is_function_key());
        assert!(!Hotkey::parse("S").unwrap().is_function_key());
    }
}
