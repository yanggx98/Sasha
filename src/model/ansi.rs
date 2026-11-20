//! ANSI escape sequence handling for text-mode log payloads.
//!
//! Escape sequences are removed from the displayed text, so search, copy,
//! save and filtering all see clean text. SGR (`ESC [ ... m`) sequences are
//! turned into style spans the log view paints as colors and font styles.
//! Parsing is per log entry; style does not carry across entries.

use std::ops::Range;

/// A color requested by an SGR sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    /// 0–7 standard, 8–15 bright, 16–255 the xterm 256-color cube and gray ramp.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Text attributes set by SGR sequences. `Default` means "no styling".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnsiStyle {
    pub fg: Option<AnsiColor>,
    pub bg: Option<AnsiColor>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub inverse: bool,
}

/// Style spans over the stripped text, sorted and non-overlapping.
pub type AnsiSpans = Vec<(Range<usize>, AnsiStyle)>;

/// Removes escape sequences from `input` and returns the plain text plus the
/// style spans (byte ranges into the returned text). Unstyled text has no span.
pub fn parse_ansi(input: &str) -> (String, AnsiSpans) {
    if !input.contains('\x1b') {
        return (input.to_string(), Vec::new());
    }

    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut spans: AnsiSpans = Vec::new();
    let mut style = AnsiStyle::default();
    let mut seg_start = 0;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != 0x1b {
            i += 1;
            continue;
        }
        push_segment(&mut out, &mut spans, &input[seg_start..i], style);
        i += 1;
        match bytes.get(i) {
            None => {}
            Some(b'[') => {
                i += 1;
                let params_start = i;
                while i < bytes.len() && (0x20..=0x3f).contains(&bytes[i]) {
                    i += 1;
                }
                match bytes.get(i) {
                    Some(&final_byte) if (0x40..=0x7e).contains(&final_byte) => {
                        if final_byte == b'm' {
                            apply_sgr(&mut style, &input[params_start..i]);
                        }
                        i += 1;
                    }
                    // Malformed or truncated: drop what was consumed, keep the rest.
                    _ => {}
                }
            }
            Some(b']') => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\') {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            Some(_) => {
                // Two-character escape (`ESC c`, `ESC 7`, ...).
                if let Some(ch) = input[i..].chars().next() {
                    i += ch.len_utf8();
                }
            }
        }
        seg_start = i;
    }
    push_segment(&mut out, &mut spans, &input[seg_start..], style);
    (out, spans)
}

/// Removes escape sequences, discarding styles.
pub fn strip_ansi(input: &str) -> String {
    parse_ansi(input).0
}

fn push_segment(out: &mut String, spans: &mut AnsiSpans, text: &str, style: AnsiStyle) {
    if text.is_empty() {
        return;
    }
    let start = out.len();
    out.push_str(text);
    if style == AnsiStyle::default() {
        return;
    }
    let end = out.len();
    match spans.last_mut() {
        Some((range, last)) if range.end == start && *last == style => range.end = end,
        _ => spans.push((start..end, style)),
    }
}

fn apply_sgr(style: &mut AnsiStyle, params: &str) {
    // Only plain digit/`;`/`:` parameter strings are SGR; `?`, `>` etc. are not.
    if !params
        .bytes()
        .all(|b| b.is_ascii_digit() || b == b';' || b == b':')
    {
        return;
    }
    let codes: Vec<Option<u16>> = params
        .split([';', ':'])
        .map(|p| {
            if p.is_empty() {
                Some(0)
            } else {
                p.parse().ok()
            }
        })
        .collect();
    let codes = if codes.is_empty() {
        vec![Some(0)]
    } else {
        codes
    };

    let mut idx = 0;
    while idx < codes.len() {
        let Some(code) = codes[idx] else {
            idx += 1;
            continue;
        };
        idx += 1;
        match code {
            0 => *style = AnsiStyle::default(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            7 => style.inverse = true,
            9 => style.strikethrough = true,
            22 => {
                style.bold = false;
                style.dim = false;
            }
            23 => style.italic = false,
            24 => style.underline = false,
            27 => style.inverse = false,
            29 => style.strikethrough = false,
            30..=37 => style.fg = Some(AnsiColor::Indexed((code - 30) as u8)),
            38 => {
                if let Some(color) = extended_color(&codes, &mut idx) {
                    style.fg = Some(color);
                }
            }
            39 => style.fg = None,
            40..=47 => style.bg = Some(AnsiColor::Indexed((code - 40) as u8)),
            48 => {
                if let Some(color) = extended_color(&codes, &mut idx) {
                    style.bg = Some(color);
                }
            }
            49 => style.bg = None,
            90..=97 => style.fg = Some(AnsiColor::Indexed((code - 90 + 8) as u8)),
            100..=107 => style.bg = Some(AnsiColor::Indexed((code - 100 + 8) as u8)),
            _ => {}
        }
    }
}

/// Reads the arguments after `38`/`48`: `5;n` or `2;r;g;b`.
fn extended_color(codes: &[Option<u16>], idx: &mut usize) -> Option<AnsiColor> {
    let byte = |value: Option<u16>| value.and_then(|v| u8::try_from(v).ok());
    match codes.get(*idx).copied().flatten()? {
        5 => {
            let n = byte(codes.get(*idx + 1).copied().flatten());
            *idx += 2;
            n.map(AnsiColor::Indexed)
        }
        2 => {
            let rgb = (1..=3)
                .map(|k| byte(codes.get(*idx + k).copied().flatten()))
                .collect::<Option<Vec<u8>>>();
            *idx += 4;
            rgb.map(|c| AnsiColor::Rgb(c[0], c[1], c[2]))
        }
        _ => {
            *idx += 1;
            None
        }
    }
}

/// RGB for a 256-color palette index. Indices 0–15 use `base16`.
pub fn indexed_rgb(index: u8, base16: &[(u8, u8, u8); 16]) -> (u8, u8, u8) {
    match index {
        0..=15 => base16[index as usize],
        16..=231 => {
            let n = index - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        232..=255 => {
            let v = 8 + (index - 232) * 10;
            (v, v, v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_untouched() {
        let (text, spans) = parse_ansi("hello 数据");
        assert_eq!(text, "hello 数据");
        assert!(spans.is_empty());
    }

    #[test]
    fn sgr_colors_become_spans_and_are_stripped() {
        let (text, spans) = parse_ansi("a\x1b[31mred\x1b[0m b");
        assert_eq!(text, "ared b");
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].0.clone()], "red");
        assert_eq!(spans[0].1.fg, Some(AnsiColor::Indexed(1)));
    }

    #[test]
    fn combined_and_extended_codes() {
        let (text, spans) = parse_ansi("\x1b[1;4;92mX\x1b[38;5;200mY\x1b[48;2;1;2;3mZ");
        assert_eq!(text, "XYZ");
        assert_eq!(spans.len(), 3);
        assert!(spans[0].1.bold && spans[0].1.underline);
        assert_eq!(spans[0].1.fg, Some(AnsiColor::Indexed(10)));
        assert_eq!(spans[1].1.fg, Some(AnsiColor::Indexed(200)));
        assert_eq!(spans[2].1.bg, Some(AnsiColor::Rgb(1, 2, 3)));
        assert_eq!(spans[2].1.fg, Some(AnsiColor::Indexed(200)));
    }

    #[test]
    fn adjacent_equal_styles_merge_and_empty_param_resets() {
        let (text, spans) = parse_ansi("\x1b[32mab\x1b[32mcd\x1b[mef");
        assert_eq!(text, "abcdef");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0, 0..4);
    }

    #[test]
    fn non_sgr_and_osc_sequences_are_dropped() {
        assert_eq!(strip_ansi("a\x1b[2Kb\x1b[1;1Hc"), "abc");
        assert_eq!(strip_ansi("a\x1b]0;title\x07b"), "ab");
        assert_eq!(strip_ansi("a\x1b]0;title\x1b\\b"), "ab");
        assert_eq!(strip_ansi("a\x1bcb"), "ab");
        assert_eq!(strip_ansi("a\x1b[?25lb"), "ab");
    }

    #[test]
    fn truncated_sequences_do_not_panic() {
        assert_eq!(strip_ansi("a\x1b"), "a");
        assert_eq!(strip_ansi("a\x1b["), "a");
        assert_eq!(strip_ansi("a\x1b[31"), "a");
        assert_eq!(strip_ansi("a\x1b[3数"), "a数");
        assert_eq!(strip_ansi("a\x1b数b"), "ab");
    }

    #[test]
    fn palette_covers_cube_and_grays() {
        let base = [(0, 0, 0); 16];
        assert_eq!(indexed_rgb(16, &base), (0, 0, 0));
        assert_eq!(indexed_rgb(231, &base), (255, 255, 255));
        assert_eq!(indexed_rgb(232, &base), (8, 8, 8));
        assert_eq!(indexed_rgb(255, &base), (238, 238, 238));
    }
}
