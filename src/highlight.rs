use std::ops::Range;
use std::sync::OnceLock;

use eframe::egui::Color32;
use regex::Regex;

use crate::theme::Palette;

// Order matters: at the same position the first matching alternative wins.
const PATTERN: &str = r#"(?P<br>\[[^\]\n]*\])|(?P<str>"[^"\n]*")|(?P<url>https?://\S+)|(?P<ip>\b\d{1,3}(?:\.\d{1,3}){3}(?::\d{2,5})?\b)|(?P<time>\b\d{2}:\d{2}:\d{2}(?:[.,]\d+)?\b)|(?P<wpath>\b[A-Za-z]:\\[^\s"'<>|]*)|(?P<upath>\B(?:~|\.{1,2})?/[\w.\-]+(?:/[\w.\-]+)*/?)|(?P<host>\b[\w.\-]+@[\w.\-]+\b)|(?P<lvl>\b(?:INFO|NOTICE|WARN|WARNING|ERROR|FATAL|SEVERE|DEBUG|TRACE)\b)|(?P<good>\b(?i:success|succeeded|passed|started|running|active|enabled|ready)\b|\bOK\b)|(?P<warnw>\b(?i:warning|deprecated|inactive|disabled|timeout|retry)\b)|(?P<bad>\b(?i:error|errors|failed|failure|fatal|denied|refused|exception|panic|critical|dead)\b)|(?P<kw>\b(?:true|false|null|none|True|False|None)\b)|(?P<num>\b\d+(?:\.\d+)?[\p{L}%]*\b)"#;

fn re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(PATTERN).expect("valid highlight regex"))
}

fn level_color(word: &str, pal: &Palette) -> Option<Color32> {
    match word.to_ascii_uppercase().as_str() {
        "INFO" | "NOTICE" => Some(pal.s_info),
        "WARN" | "WARNING" => Some(pal.s_warn),
        "ERROR" | "FATAL" | "SEVERE" => Some(pal.s_error),
        "DEBUG" | "TRACE" => Some(pal.s_debug),
        _ => None,
    }
}

fn bracket_color(group: &str, pal: &Palette) -> Color32 {
    let inner = &group[1..group.len() - 1];
    if let Some(c) = inner
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .find_map(|w| level_color(w, pal))
    {
        return c;
    }
    let looks_like_time = inner.contains(':')
        && inner
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ':' | '.' | ','));
    if looks_like_time {
        pal.s_time
    } else {
        pal.s_bracket
    }
}

/// Finds syntax tokens in a line of terminal output and returns byte ranges with their colors.
pub fn spans(text: &str, pal: &Palette) -> Vec<(Range<usize>, Color32)> {
    let mut out = Vec::new();
    for caps in re().captures_iter(text) {
        let m = caps.get(0).unwrap();
        let s = m.as_str();
        let has = |n: &str| caps.name(n).is_some();
        let color = if has("br") {
            bracket_color(s, pal)
        } else if has("str") {
            pal.s_string
        } else if has("url") || has("ip") {
            pal.s_link
        } else if has("time") {
            pal.s_time
        } else if has("wpath") || has("upath") {
            pal.s_path
        } else if has("host") {
            pal.s_host
        } else if has("lvl") {
            level_color(s, pal).unwrap_or(pal.text)
        } else if has("good") {
            pal.ok
        } else if has("warnw") {
            pal.s_warn
        } else if has("bad") {
            pal.s_error
        } else if has("kw") {
            pal.s_keyword
        } else {
            pal.s_number
        };
        out.push((m.range(), color));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tokens() {
        let pal = Palette::light();
        let text = "[12:00:01] [main/ERROR]: failed to open /etc/nginx/nginx.conf on 10.0.0.5:22 (3 retries)";
        let spans = spans(text, &pal);
        let find = |needle: &str| {
            let start = text.find(needle).unwrap();
            spans
                .iter()
                .find(|(r, _)| r.start == start)
                .map(|(_, c)| *c)
        };
        assert_eq!(find("[12:00:01]"), Some(pal.s_time));
        assert_eq!(find("[main/ERROR]"), Some(pal.s_error));
        assert_eq!(find("failed"), Some(pal.s_error));
        assert_eq!(find("/etc/nginx/nginx.conf"), Some(pal.s_path));
        assert_eq!(find("10.0.0.5:22"), Some(pal.s_link));
    }

    #[test]
    fn plain_words_are_not_paths() {
        let pal = Palette::light();
        let spans = spans("and/or this is plain text", &pal);
        assert!(spans.is_empty(), "{spans:?}");
    }
}
