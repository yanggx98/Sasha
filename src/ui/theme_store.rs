//! Theme files: TOML parsing, derivation rules, built-in themes, directory
//! scanning, fallback resolution and the mapping onto gpui_kit's global `Theme`.
//!
//! Pure logic only; applying a theme to the running app lives elsewhere.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use gpui_kit::{Hsla, Rgba, rgb, rgba};
use serde::Deserialize;

use super::theme::NanoTheme;

pub const BUILTIN_DARK_ID: &str = "dark";
pub const BUILTIN_LIGHT_ID: &str = "light";
pub const BUILTIN_CLAUDE_DARK_ID: &str = "claude-dark";
pub const BUILTIN_CLAUDE_LIGHT_ID: &str = "claude-light";
pub const BUILTIN_GEMINI_DARK_ID: &str = "gemini-dark";
pub const BUILTIN_GEMINI_LIGHT_ID: &str = "gemini-light";
pub const BUILTIN_DEEPSEEK_DARK_ID: &str = "deepseek-dark";
pub const BUILTIN_DEEPSEEK_LIGHT_ID: &str = "deepseek-light";
pub const BUILTIN_GROK_DARK_ID: &str = "grok-dark";
pub const BUILTIN_GROK_LIGHT_ID: &str = "grok-light";

const DARK_TOML: &str = include_str!("theme_assets/dark.toml");
const LIGHT_TOML: &str = include_str!("theme_assets/light.toml");
const CLAUDE_DARK_TOML: &str = include_str!("theme_assets/claude-dark.toml");
const CLAUDE_LIGHT_TOML: &str = include_str!("theme_assets/claude-light.toml");
const GEMINI_DARK_TOML: &str = include_str!("theme_assets/gemini-dark.toml");
const GEMINI_LIGHT_TOML: &str = include_str!("theme_assets/gemini-light.toml");
const DEEPSEEK_DARK_TOML: &str = include_str!("theme_assets/deepseek-dark.toml");
const DEEPSEEK_LIGHT_TOML: &str = include_str!("theme_assets/deepseek-light.toml");
const GROK_DARK_TOML: &str = include_str!("theme_assets/grok-dark.toml");
const GROK_LIGHT_TOML: &str = include_str!("theme_assets/grok-light.toml");

/// Light or dark family a theme belongs to (also decides gpui `ThemeMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeBase {
    Dark,
    Light,
}

impl ThemeBase {
    pub fn is_dark(self) -> bool {
        self == ThemeBase::Dark
    }
}

/// One selectable theme.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeEntry {
    pub id: String,
    pub name: String,
    pub base: ThemeBase,
    pub builtin: bool,
    pub theme: NanoTheme,
    /// The eight base colors the theme was built from (for the editor).
    pub colors: BaseColors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeErrorKind {
    /// File could not be read.
    Io,
    /// TOML or color syntax is invalid; the theme is not loaded.
    Parse,
    /// A file id equals a built-in id; the built-in theme wins.
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    pub kind: ThemeErrorKind,
    /// File name such as `ocean.toml`.
    pub file: String,
    /// Offending field (`canvas`, `overrides.bg_hover`) when known.
    pub field: Option<String>,
    pub message: String,
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(field) => write!(f, "{}: {}: {}", self.file, field, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

impl std::error::Error for ThemeError {}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeScan {
    /// Built-ins first (dark, light), then custom themes sorted by id.
    pub themes: Vec<ThemeEntry>,
    pub errors: Vec<ThemeError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeWarningKind {
    /// No loaded theme has this id.
    Missing,
    /// A file for this id exists but failed to load (message attached).
    Broken(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeWarning {
    pub id: String,
    pub kind: ThemeWarningKind,
}

// ---------------------------------------------------------------- colors

fn parse_hex(text: &str) -> Result<Hsla, String> {
    let bad = || format!("invalid color `{text}`: expected #RRGGBB or #RRGGBBAA");
    let digits = text.trim().strip_prefix('#').ok_or_else(bad)?;
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let value = u32::from_str_radix(digits, 16).map_err(|_| bad())?;
    match digits.len() {
        6 => Ok(rgb(value).into()),
        8 => Ok(rgba(value).into()),
        _ => Err(bad()),
    }
}

/// Mix in sRGB space: `t = 0` gives `a`, `t = 1` gives `b`; alpha kept from `a`.
fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (x, y) = (Rgba::from(a), Rgba::from(b));
    Rgba {
        r: x.r + (y.r - x.r) * t,
        g: x.g + (y.g - x.g) * t,
        b: x.b + (y.b - x.b) * t,
        a: x.a,
    }
    .into()
}

fn with_alpha(c: Hsla, a: f32) -> Hsla {
    c.alpha(a)
}

fn black() -> Hsla {
    rgb(0x000000).into()
}

fn white() -> Hsla {
    rgb(0xFFFFFF).into()
}

fn luminance(c: Hsla) -> f32 {
    let c = Rgba::from(c);
    let lin = |v: f32| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio between two colors (alpha ignored).
pub fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Black or white, whichever reads better on `bg` (white wins ties).
pub fn contrast_foreground(bg: Hsla) -> Hsla {
    if contrast_ratio(bg, white()) >= contrast_ratio(bg, black()) {
        white()
    } else {
        black()
    }
}

/// Send-area shadow `(alpha, y_offset_px, blur_px)`. One soft shadow shared by
/// every theme so dark and light look the same. The offset is negative (cast
/// upward) for the expanded card; the collapsed toolbar mirrors it downward.
pub fn send_card_shadow() -> (f32, f32, f32) {
    (0.12, -2.0, 8.0)
}

/// Background highlight colors for search matches `(normal, active)` over a
/// log canvas. Dark keeps the original yellow/amber; on light canvases the
/// alpha is raised so the highlight stays visible while dark text stays readable.
pub fn search_highlight_colors(canvas: Hsla) -> (Hsla, Hsla) {
    if luminance(canvas) > 0.5 {
        (rgba(0xEAB30873).into(), rgba(0xF59E0BB3).into())
    } else {
        (rgba(0xEAB30840).into(), rgba(0xF59E0B99).into())
    }
}

// ------------------------------------------------------------ derivation

/// The eight base colors a theme file defines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaseColors {
    pub canvas: Hsla,
    pub card: Hsla,
    pub text: Hsla,
    pub accent: Hsla,
    pub rx: Hsla,
    pub tx: Hsla,
    pub error: Hsla,
    pub border: Hsla,
}

/// Derivation constants for one base family.
struct DeriveParams {
    strip_to_black: f32,
    popover_to_text: f32,
    hover_alpha: f32,
    toggle_alpha: f32,
    accent_hover_mix: f32,
    accent_hover_target: fn() -> Hsla,
    secondary_mix: f32,
    muted_mix: f32,
    disconnected_mix: f32,
    badge_text_mix: f32,
    badge_text_target: fn() -> Hsla,
}

fn params(base: ThemeBase) -> DeriveParams {
    match base {
        ThemeBase::Dark => DeriveParams {
            strip_to_black: 0.23,
            popover_to_text: 0.03,
            hover_alpha: 0.07,
            toggle_alpha: 0.14,
            accent_hover_mix: 0.18,
            accent_hover_target: white,
            secondary_mix: 0.35,
            muted_mix: 0.55,
            disconnected_mix: 0.42,
            badge_text_mix: 0.25,
            badge_text_target: white,
        },
        ThemeBase::Light => DeriveParams {
            strip_to_black: 0.04,
            popover_to_text: 0.0,
            hover_alpha: 0.06,
            toggle_alpha: 0.12,
            accent_hover_mix: 0.14,
            accent_hover_target: black,
            secondary_mix: 0.25,
            muted_mix: 0.40,
            disconnected_mix: 0.40,
            badge_text_mix: 0.0,
            badge_text_target: black,
        },
    }
}

/// Derives every `NanoTheme` token from the base colors.
pub fn derive_theme(base: ThemeBase, c: &BaseColors) -> NanoTheme {
    let p = params(base);
    NanoTheme {
        bg_canvas: c.canvas,
        bg_card: c.card,
        bg_strip: mix(c.canvas, black(), p.strip_to_black),
        bg_popover: mix(c.card, c.text, p.popover_to_text),
        bg_hover: with_alpha(c.text, p.hover_alpha),
        bg_toggle_active: with_alpha(c.accent, p.toggle_alpha),
        border_card: c.border,
        border_popover: with_alpha(c.border, (c.border.a * 1.75).min(1.0)),
        accent_blue: c.accent,
        accent_blue_hover: mix(c.accent, (p.accent_hover_target)(), p.accent_hover_mix),
        rx_green: c.rx,
        tx_blue: c.tx,
        text_primary: c.text,
        text_secondary: mix(c.text, c.canvas, p.secondary_mix),
        text_muted: mix(c.text, c.canvas, p.muted_mix),
        error_text: c.error,
        error_bg: with_alpha(c.error, 0.12),
        sim_badge_bg: with_alpha(c.accent, 0.12),
        sim_badge_text: mix(c.tx, (p.badge_text_target)(), p.badge_text_mix),
        status_disconnected: mix(c.text, c.canvas, p.disconnected_mix),
        status_connected: c.rx,
    }
}

/// Mutable slot of a `NanoTheme` token by its field name.
fn token_slot<'a>(theme: &'a mut NanoTheme, key: &str) -> Option<&'a mut Hsla> {
    Some(match key {
        "bg_canvas" => &mut theme.bg_canvas,
        "bg_card" => &mut theme.bg_card,
        "bg_strip" => &mut theme.bg_strip,
        "bg_popover" => &mut theme.bg_popover,
        "bg_hover" => &mut theme.bg_hover,
        "bg_toggle_active" => &mut theme.bg_toggle_active,
        "border_card" => &mut theme.border_card,
        "border_popover" => &mut theme.border_popover,
        "accent_blue" => &mut theme.accent_blue,
        "accent_blue_hover" => &mut theme.accent_blue_hover,
        "rx_green" => &mut theme.rx_green,
        "tx_blue" => &mut theme.tx_blue,
        "text_primary" => &mut theme.text_primary,
        "text_secondary" => &mut theme.text_secondary,
        "text_muted" => &mut theme.text_muted,
        "error_text" => &mut theme.error_text,
        "error_bg" => &mut theme.error_bg,
        "sim_badge_bg" => &mut theme.sim_badge_bg,
        "sim_badge_text" => &mut theme.sim_badge_text,
        "status_disconnected" => &mut theme.status_disconnected,
        "status_connected" => &mut theme.status_connected,
        _ => return None,
    })
}

// --------------------------------------------------------------- parsing

#[derive(Debug, Deserialize, Default)]
struct RawTheme {
    name: Option<String>,
    base: Option<String>,
    canvas: Option<String>,
    card: Option<String>,
    text: Option<String>,
    accent: Option<String>,
    rx: Option<String>,
    tx: Option<String>,
    error: Option<String>,
    border: Option<String>,
    overrides: Option<BTreeMap<String, toml::Value>>,
}

fn perr(file: &str, field: Option<&str>, message: impl Into<String>) -> ThemeError {
    ThemeError {
        kind: ThemeErrorKind::Parse,
        file: file.to_string(),
        field: field.map(str::to_string),
        message: message.into(),
    }
}

/// Parses theme text. `id` is the file stem; errors name `<id>.toml`.
/// Missing base colors come from `fallback(base)`; `None` is for the built-ins,
/// which must be complete.
fn parse_with(
    id: &str,
    text: &str,
    builtin: bool,
    fallback: Option<fn(ThemeBase) -> BaseColors>,
) -> Result<ThemeEntry, ThemeError> {
    let file = format!("{id}.toml");
    let raw: RawTheme = toml::from_str(text).map_err(|e| perr(&file, None, e.message()))?;

    let base = match raw.base.as_deref() {
        None | Some("dark") => ThemeBase::Dark,
        Some("light") => ThemeBase::Light,
        Some(other) => {
            return Err(perr(
                &file,
                Some("base"),
                format!("invalid base `{other}`: expected \"dark\" or \"light\""),
            ));
        }
    };
    let fb = fallback.map(|f| f(base));

    let pick = |field: &str,
                value: &Option<String>,
                from_fb: fn(&BaseColors) -> Hsla|
     -> Result<Hsla, ThemeError> {
        match value {
            Some(s) => parse_hex(s).map_err(|m| perr(&file, Some(field), m)),
            None => fb
                .as_ref()
                .map(from_fb)
                .ok_or_else(|| perr(&file, Some(field), "missing required color")),
        }
    };
    let colors = BaseColors {
        canvas: pick("canvas", &raw.canvas, |b| b.canvas)?,
        card: pick("card", &raw.card, |b| b.card)?,
        text: pick("text", &raw.text, |b| b.text)?,
        accent: pick("accent", &raw.accent, |b| b.accent)?,
        rx: pick("rx", &raw.rx, |b| b.rx)?,
        tx: pick("tx", &raw.tx, |b| b.tx)?,
        error: pick("error", &raw.error, |b| b.error)?,
        border: pick("border", &raw.border, |b| b.border)?,
    };

    let mut theme = derive_theme(base, &colors);
    if let Some(overrides) = &raw.overrides {
        for (key, value) in overrides {
            let field = format!("overrides.{key}");
            let Some(slot) = token_slot(&mut theme, key) else {
                continue; // unknown keys are ignored
            };
            let s = value
                .as_str()
                .ok_or_else(|| perr(&file, Some(&field), "expected a color string"))?;
            *slot = parse_hex(s).map_err(|m| perr(&file, Some(&field), m))?;
        }
    }

    let name = raw
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| id.to_string());
    Ok(ThemeEntry {
        id: id.to_string(),
        name,
        base,
        builtin,
        theme,
        colors,
    })
}

/// Base colors of the embedded built-ins: `[dark, light]`.
fn builtin_base_colors() -> &'static [BaseColors; 2] {
    static CELL: OnceLock<[BaseColors; 2]> = OnceLock::new();
    CELL.get_or_init(|| {
        let load = |text: &str| -> BaseColors {
            let raw: RawTheme = toml::from_str(text).expect("built-in theme TOML is valid");
            let g = |v: &Option<String>| {
                parse_hex(v.as_deref().expect("built-in color present"))
                    .expect("built-in color valid")
            };
            BaseColors {
                canvas: g(&raw.canvas),
                card: g(&raw.card),
                text: g(&raw.text),
                accent: g(&raw.accent),
                rx: g(&raw.rx),
                tx: g(&raw.tx),
                error: g(&raw.error),
                border: g(&raw.border),
            }
        };
        [load(DARK_TOML), load(LIGHT_TOML)]
    })
}

fn fallback_colors(base: ThemeBase) -> BaseColors {
    let b = builtin_base_colors();
    match base {
        ThemeBase::Dark => b[0],
        ThemeBase::Light => b[1],
    }
}

/// Parses a user theme (`id` = file stem). Missing colors fall back to the
/// built-in theme of the same `base`.
pub fn parse_theme(id: &str, text: &str) -> Result<ThemeEntry, ThemeError> {
    parse_with(id, text, false, Some(fallback_colors))
}

/// The built-in themes: dark, light, then Claude, Gemini, DeepSeek and Grok pairs.
pub fn builtin_themes() -> Vec<ThemeEntry> {
    [
        (BUILTIN_DARK_ID, DARK_TOML),
        (BUILTIN_LIGHT_ID, LIGHT_TOML),
        (BUILTIN_CLAUDE_DARK_ID, CLAUDE_DARK_TOML),
        (BUILTIN_CLAUDE_LIGHT_ID, CLAUDE_LIGHT_TOML),
        (BUILTIN_GEMINI_DARK_ID, GEMINI_DARK_TOML),
        (BUILTIN_GEMINI_LIGHT_ID, GEMINI_LIGHT_TOML),
        (BUILTIN_DEEPSEEK_DARK_ID, DEEPSEEK_DARK_TOML),
        (BUILTIN_DEEPSEEK_LIGHT_ID, DEEPSEEK_LIGHT_TOML),
        (BUILTIN_GROK_DARK_ID, GROK_DARK_TOML),
        (BUILTIN_GROK_LIGHT_ID, GROK_LIGHT_TOML),
    ]
    .iter()
    .map(|(id, text)| parse_with(id, text, true, None).expect("built-in theme TOML is valid"))
    .collect()
}

pub fn builtin_dark() -> ThemeEntry {
    builtin_themes().remove(0)
}

fn is_builtin_id(id: &str) -> bool {
    [
        BUILTIN_DARK_ID,
        BUILTIN_LIGHT_ID,
        BUILTIN_CLAUDE_DARK_ID,
        BUILTIN_CLAUDE_LIGHT_ID,
        BUILTIN_GEMINI_DARK_ID,
        BUILTIN_GEMINI_LIGHT_ID,
        BUILTIN_DEEPSEEK_DARK_ID,
        BUILTIN_DEEPSEEK_LIGHT_ID,
        BUILTIN_GROK_DARK_ID,
        BUILTIN_GROK_LIGHT_ID,
    ]
    .contains(&id)
}

// --------------------------------------------------------------- writing

/// `#RRGGBB`, or `#RRGGBBAA` when the color is translucent (uppercase).
pub fn format_hex(c: Hsla) -> String {
    let c = Rgba::from(c);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b, a) = (byte(c.r), byte(c.g), byte(c.b), byte(c.a));
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// Public color validator shared with the editor UI.
pub fn parse_color(text: &str) -> Result<Hsla, String> {
    parse_hex(text)
}

/// Theme file text. The `[overrides]` table of `overrides_from` (an earlier
/// version of the same file) is carried over when it parses.
pub fn serialize_theme(
    name: &str,
    base: ThemeBase,
    colors: &BaseColors,
    overrides_from: Option<&str>,
) -> String {
    let base_text = if base.is_dark() { "dark" } else { "light" };
    let mut out = format!(
        "name = {}\nbase = \"{base_text}\"\n\n",
        toml::Value::String(name.to_string())
    );
    for (key, value) in [
        ("canvas", colors.canvas),
        ("card", colors.card),
        ("text", colors.text),
        ("accent", colors.accent),
        ("rx", colors.rx),
        ("tx", colors.tx),
        ("error", colors.error),
        ("border", colors.border),
    ] {
        out.push_str(&format!("{key} = \"{}\"\n", format_hex(value)));
    }
    let overrides = overrides_from
        .and_then(|text| toml::from_str::<toml::Table>(text).ok())
        .and_then(|mut table| table.remove("overrides"))
        .filter(|value| value.as_table().is_some_and(|t| !t.is_empty()));
    if let Some(overrides) = overrides {
        let mut wrapper = toml::Table::new();
        wrapper.insert("overrides".to_string(), overrides);
        if let Ok(text) = toml::to_string(&wrapper) {
            out.push('\n');
            out.push_str(&text);
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeIdError {
    /// The name has no usable characters.
    Empty,
    /// Built-in id or Windows device name.
    Reserved,
}

/// Lowercase file-safe id from a display name: alphanumerics (any script) are
/// kept, every other run becomes one `-`.
pub fn theme_id_from_name(name: &str) -> Result<String, ThemeIdError> {
    let mut id = String::new();
    let mut pending_dash = false;
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !id.is_empty() {
                id.push('-');
            }
            pending_dash = false;
            id.extend(ch.to_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if id.is_empty() {
        return Err(ThemeIdError::Empty);
    }
    let device = matches!(id.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|p| {
            id.strip_prefix(p)
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        });
    if is_builtin_id(&id) || device {
        return Err(ThemeIdError::Reserved);
    }
    Ok(id)
}

fn builtin_id_error(id: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("`{id}` is a built-in theme id"),
    )
}

/// Creates `<id>.toml`; fails with `AlreadyExists` instead of overwriting.
pub fn write_new_theme_file(dir: &Path, id: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if is_builtin_id(id) {
        return Err(builtin_id_error(id));
    }
    fs::create_dir_all(dir)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(format!("{id}.toml")))?;
    file.write_all(text.as_bytes())
}

/// Replaces the content of an existing custom theme file.
pub fn overwrite_theme_file(dir: &Path, id: &str, text: &str) -> std::io::Result<()> {
    if is_builtin_id(id) {
        return Err(builtin_id_error(id));
    }
    fs::write(dir.join(format!("{id}.toml")), text)
}

pub fn delete_theme_file(dir: &Path, id: &str) -> std::io::Result<()> {
    if is_builtin_id(id) {
        return Err(builtin_id_error(id));
    }
    fs::remove_file(dir.join(format!("{id}.toml")))
}

// ------------------------------------------------------------------ scan

/// Loads built-ins plus every `*.toml` in `dir`. A missing directory is empty.
pub fn scan_dir(dir: &Path) -> ThemeScan {
    let mut scan = ThemeScan {
        themes: builtin_themes(),
        errors: Vec::new(),
    };
    let Ok(read) = fs::read_dir(dir) else {
        return scan;
    };
    let mut files: Vec<(String, std::path::PathBuf)> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x.eq_ignore_ascii_case("toml"))
        })
        .filter_map(|p| {
            let stem = p.file_stem()?.to_str()?.to_string();
            Some((stem, p))
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut custom: Vec<ThemeEntry> = Vec::new();
    for (id, path) in files {
        let file = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&id)
            .to_string();
        if is_builtin_id(&id) {
            scan.errors.push(ThemeError {
                kind: ThemeErrorKind::Conflict,
                file,
                field: None,
                message: format!("id `{id}` is reserved by a built-in theme; the built-in is used"),
            });
            continue;
        }
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                scan.errors.push(ThemeError {
                    kind: ThemeErrorKind::Io,
                    file,
                    field: None,
                    message: e.to_string(),
                });
                continue;
            }
        };
        match parse_theme(&id, &text) {
            Ok(entry) => custom.push(entry),
            Err(mut e) => {
                e.file = file;
                scan.errors.push(e);
            }
        }
    }
    custom.sort_by(|a, b| a.id.cmp(&b.id));
    scan.themes.extend(custom);
    scan
}

// --------------------------------------------------------------- resolve

/// Finds `id` in `themes`; otherwise falls back to built-in dark with a warning.
#[allow(dead_code)]
pub fn resolve(id: &str, themes: &[ThemeEntry]) -> (NanoTheme, Option<ThemeWarning>) {
    let (entry, warning) = resolve_entry(id, themes);
    (entry.theme, warning)
}

/// Like [`resolve`] but returns the whole entry (callers need `base`).
pub fn resolve_entry(id: &str, themes: &[ThemeEntry]) -> (ThemeEntry, Option<ThemeWarning>) {
    match themes.iter().find(|t| t.id == id) {
        Some(t) => (t.clone(), None),
        None => (
            builtin_dark(),
            Some(ThemeWarning {
                id: id.to_string(),
                kind: ThemeWarningKind::Missing,
            }),
        ),
    }
}

/// Like [`resolve_entry`] but distinguishes a broken file from a missing one
/// using the scan's error list.
pub fn resolve_scan(id: &str, scan: &ThemeScan) -> (ThemeEntry, Option<ThemeWarning>) {
    let (entry, warning) = resolve_entry(id, &scan.themes);
    match warning {
        Some(mut w) => {
            let file = format!("{id}.toml");
            if let Some(err) = scan
                .errors
                .iter()
                .find(|e| e.kind != ThemeErrorKind::Conflict && e.file == file)
            {
                w.kind = ThemeWarningKind::Broken(err.to_string());
            }
            (entry, Some(w))
        }
        None => (entry, None),
    }
}

// ------------------------------------------------------- gpui Theme map

/// Colors to write into gpui_kit's global `Theme` (`ThemeColor` fields of the
/// same names), plus the mode. Apply with `Theme::change(mode)` first, then
/// `Theme::update` with these (THEME-B).
#[derive(Debug, Clone, Copy)]
pub struct GpuiThemeColors {
    #[allow(dead_code)]
    pub base: ThemeBase,
    pub background: Hsla,
    pub foreground: Hsla,
    pub input: Hsla,
    pub border: Hsla,
    pub primary: Hsla,
    pub primary_hover: Hsla,
    pub primary_active: Hsla,
    pub primary_foreground: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub popover: Hsla,
    pub popover_foreground: Hsla,
    pub caret: Hsla,
    pub ring: Hsla,
    pub selection: Hsla,
    pub accent: Hsla,
    pub accent_foreground: Hsla,
    pub link: Hsla,
    pub scrollbar: Hsla,
    pub scrollbar_thumb: Hsla,
    pub scrollbar_thumb_hover: Hsla,
    pub danger: Hsla,
    pub danger_hover: Hsla,
    pub danger_active: Hsla,
    pub danger_foreground: Hsla,
}

/// Maps a Sasha theme onto the gpui global theme colors. Window-close danger
/// colors stay the native Windows deep red in both modes (as in `main.rs`).
pub fn gpui_theme_colors(theme: &NanoTheme, base: ThemeBase) -> GpuiThemeColors {
    let primary_foreground = contrast_foreground(theme.accent_blue);
    GpuiThemeColors {
        base,
        background: theme.bg_canvas,
        foreground: theme.text_primary,
        input: theme.border_card,
        border: theme.border_card,
        primary: theme.accent_blue,
        primary_hover: theme.accent_blue_hover,
        primary_active: theme.accent_blue,
        primary_foreground,
        muted: theme.bg_strip,
        muted_foreground: theme.text_muted,
        popover: theme.bg_popover,
        popover_foreground: theme.text_primary,
        caret: theme.text_primary,
        ring: theme.accent_blue,
        selection: with_alpha(theme.accent_blue, 0.35),
        accent: theme.bg_hover,
        accent_foreground: theme.text_primary,
        link: theme.accent_blue,
        scrollbar: with_alpha(theme.bg_canvas, 0.0),
        scrollbar_thumb: with_alpha(theme.text_muted, 0.5),
        scrollbar_thumb_hover: with_alpha(theme.text_secondary, 0.7),
        danger: rgb(0xC42B1C).into(),
        danger_hover: rgb(0xB22A1B).into(),
        danger_active: rgb(0xA52619).into(),
        danger_foreground: white(),
    }
}

// ------------------------------------------------------------ app state

/// User-facing theme problem, rendered by the UI layer in the active locale.
/// File names, field names and parser details stay verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeMessage {
    /// Configured theme not found; built-in dark is in effect.
    FallbackMissing { id: String },
    /// Configured theme file failed to load; built-in dark is in effect.
    FallbackBroken { id: String, detail: String },
    /// A theme file was skipped (parse error, unreadable, or id conflict).
    File(ThemeError),
    /// Persisting the selected theme failed.
    SaveFailed(String),
    /// The themes directory could not be created.
    DirCreateFailed { path: String, error: String },
    /// Directory watching is unavailable (themes still load at startup).
    WatchUnavailable(String),
}

/// Surface alpha used over a workspace background image. Dark keeps the
/// original 0.84; light is more opaque so dark text stays readable even over
/// a dark photo (a mostly-white surface at 0.84 turns grey and washes out).
pub fn surface_opacity(base: ThemeBase) -> f32 {
    match base {
        ThemeBase::Dark => 0.84,
        ThemeBase::Light => 0.90,
    }
}

/// True for paths a theme watcher should react to (`*.toml`, any case).
pub fn is_toml_path(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| x.eq_ignore_ascii_case("toml"))
}

/// Trailing-edge debouncer: fires once, `quiet` after the last event.
#[derive(Debug, Clone)]
pub struct Debouncer {
    quiet: std::time::Duration,
    last_event: Option<std::time::Instant>,
}

impl Debouncer {
    pub fn new(quiet: std::time::Duration) -> Self {
        Self {
            quiet,
            last_event: None,
        }
    }

    pub fn note(&mut self, now: std::time::Instant) {
        self.last_event = Some(now);
    }

    /// Returns true (and clears the pending state) once the quiet period elapsed.
    pub fn poll(&mut self, now: std::time::Instant) -> bool {
        match self.last_event {
            Some(t) if now.saturating_duration_since(t) >= self.quiet => {
                self.last_event = None;
                true
            }
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn is_pending(&self) -> bool {
        self.last_event.is_some()
    }
}

/// Observed state of the themes directory and its watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchHealth {
    pub dir_exists: bool,
    pub watching: bool,
    /// An event concerning the directory itself (removed / renamed / created).
    pub dir_event: bool,
    /// The debounce window for file events has elapsed.
    pub events_due: bool,
}

/// What to do about the watcher and the theme list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WatchPlan {
    pub drop_watcher: bool,
    pub rebuild_watcher: bool,
    pub rescan: bool,
}

/// Pure decision logic for keeping the directory watcher alive.
///
/// - Directory gone: drop the stale watcher and rescan (list falls back to built-ins).
/// - Directory present but not watched, or a directory-level event arrived
///   (it may have been recreated, which invalidates the old handle): rebuild and rescan.
/// - Otherwise rescan only when debounced file events are due.
pub fn plan_watch(h: WatchHealth) -> WatchPlan {
    let drop_watcher = !h.dir_exists && (h.watching || h.dir_event);
    let rebuild_watcher = h.dir_exists && (!h.watching || h.dir_event);
    WatchPlan {
        drop_watcher,
        rebuild_watcher,
        rescan: h.events_due || drop_watcher || rebuild_watcher,
    }
}

/// Loaded themes plus the selected one. Pure state; no gpui, no file watching.
///
/// `selected` is the configured id and is never rewritten on fallback, so a
/// repaired or restored file takes effect again on the next rescan.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeState {
    dir: std::path::PathBuf,
    scan: ThemeScan,
    selected: String,
    current: ThemeEntry,
    warning: Option<ThemeWarning>,
}

impl ThemeState {
    pub fn load(dir: &Path, selected: &str) -> Self {
        let scan = scan_dir(dir);
        let (current, warning) = resolve_scan(selected, &scan);
        Self {
            dir: dir.to_path_buf(),
            scan,
            selected: selected.to_string(),
            current,
            warning,
        }
    }

    /// Rescans the directory; returns true if anything observable changed.
    pub fn rescan(&mut self) -> bool {
        let fresh = Self::load(&self.dir, &self.selected);
        let changed = fresh != *self;
        *self = fresh;
        changed
    }

    /// Selects a loaded theme by id. Returns false (state unchanged) if unknown.
    pub fn select(&mut self, id: &str) -> bool {
        if !self.scan.themes.iter().any(|t| t.id == id) {
            return false;
        }
        self.selected = id.to_string();
        let (current, warning) = resolve_scan(id, &self.scan);
        self.current = current;
        self.warning = warning;
        true
    }

    /// A loaded theme by id.
    pub fn entry(&self, id: &str) -> Option<&ThemeEntry> {
        self.scan.themes.iter().find(|t| t.id == id)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn themes(&self) -> &[ThemeEntry] {
        &self.scan.themes
    }

    /// Configured id (may name a theme that is currently missing).
    pub fn selected_id(&self) -> &str {
        &self.selected
    }

    /// Theme actually in effect (built-in dark after a fallback).
    pub fn current(&self) -> &ThemeEntry {
        &self.current
    }

    #[allow(dead_code)]
    pub fn fallback_warning(&self) -> Option<&ThemeWarning> {
        self.warning.as_ref()
    }

    /// Structured messages: fallback warning first, then per-file errors.
    pub fn messages(&self) -> Vec<ThemeMessage> {
        let mut out = Vec::new();
        if let Some(w) = &self.warning {
            out.push(match &w.kind {
                ThemeWarningKind::Missing => ThemeMessage::FallbackMissing { id: w.id.clone() },
                ThemeWarningKind::Broken(detail) => ThemeMessage::FallbackBroken {
                    id: w.id.clone(),
                    detail: detail.clone(),
                },
            });
        }
        out.extend(self.scan.errors.iter().cloned().map(ThemeMessage::File));
        out
    }
}

// ----------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "sasha_theme_test_{label}_{}_{}",
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&path).expect("create temp test dir");
            Self { path }
        }

        fn write(&self, name: &str, text: &str) {
            fs::write(self.path.join(name), text).expect("write theme file");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn all_tokens(t: &NanoTheme) -> Vec<Hsla> {
        vec![
            t.bg_canvas,
            t.bg_card,
            t.bg_strip,
            t.bg_popover,
            t.bg_hover,
            t.bg_toggle_active,
            t.border_card,
            t.border_popover,
            t.accent_blue,
            t.accent_blue_hover,
            t.rx_green,
            t.tx_blue,
            t.text_primary,
            t.text_secondary,
            t.text_muted,
            t.error_text,
            t.error_bg,
            t.sim_badge_bg,
            t.sim_badge_text,
            t.status_disconnected,
            t.status_connected,
        ]
    }

    #[test]
    fn builtin_dark_equals_legacy_default() {
        let dark = builtin_dark();
        assert_eq!(dark.id, "dark");
        assert!(dark.builtin && dark.base == ThemeBase::Dark);
        assert_eq!(all_tokens(&dark.theme), all_tokens(&NanoTheme::default()));
    }

    #[test]
    fn builtins_are_dark_light_then_brand_pairs() {
        let ids: Vec<_> = builtin_themes().into_iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            [
                "dark",
                "light",
                "claude-dark",
                "claude-light",
                "gemini-dark",
                "gemini-light",
                "deepseek-dark",
                "deepseek-light",
                "grok-dark",
                "grok-light"
            ]
        );
    }

    #[test]
    fn text_contrast_is_readable_on_both_builtins() {
        for entry in builtin_themes() {
            let t = &entry.theme;
            let id = &entry.id;
            assert!(contrast_ratio(t.text_primary, t.bg_canvas) >= 4.5, "{id}");
            assert!(contrast_ratio(t.text_primary, t.bg_card) >= 4.5, "{id}");
            assert!(contrast_ratio(t.text_secondary, t.bg_canvas) >= 4.5, "{id}");
            assert!(contrast_ratio(t.text_muted, t.bg_canvas) >= 3.0, "{id}");
            assert!(contrast_ratio(t.rx_green, t.bg_canvas) >= 3.0, "{id}");
            assert!(contrast_ratio(t.tx_blue, t.bg_canvas) >= 3.0, "{id}");
        }
    }

    #[test]
    fn light_theme_is_light_and_tx_independent_of_accent() {
        let light = &builtin_themes()[1];
        assert_eq!(light.base, ThemeBase::Light);
        assert!(luminance(light.theme.bg_canvas) > 0.8);
        assert_ne!(light.theme.tx_blue, light.theme.accent_blue);
    }

    #[test]
    fn changing_accent_updates_derived_tokens_but_not_tx() {
        let a = parse_theme("a", "base = \"dark\"\naccent = \"#FF0000\"\n").unwrap();
        let b = parse_theme("b", "base = \"dark\"\naccent = \"#00AA00\"\n").unwrap();
        assert_ne!(a.theme.accent_blue_hover, b.theme.accent_blue_hover);
        assert_ne!(a.theme.bg_toggle_active, b.theme.bg_toggle_active);
        assert_ne!(a.theme.sim_badge_bg, b.theme.sim_badge_bg);
        assert_eq!(a.theme.tx_blue, b.theme.tx_blue);
        assert_eq!(a.theme.accent_blue, rgb(0xFF0000).into());
    }

    #[test]
    fn overrides_win_over_derivation() {
        let t = parse_theme(
            "o",
            "accent = \"#FF0000\"\n[overrides]\nbg_toggle_active = \"#12345678\"\nunknown_key = 5\n",
        )
        .unwrap();
        assert_eq!(t.theme.bg_toggle_active, rgba(0x12345678).into());
        assert_eq!(t.theme.accent_blue, rgb(0xFF0000).into());
    }

    #[test]
    fn missing_fields_fall_back_to_builtin_of_same_base() {
        let d = parse_theme("d", "").unwrap();
        assert_eq!(d.name, "d");
        assert_eq!(d.base, ThemeBase::Dark);
        assert_eq!(d.theme.bg_canvas, rgb(0x0D1117).into());
        let l = parse_theme(
            "l",
            "name = \"My Light\"\nbase = \"light\"\naccent = \"#8250DF\"\n",
        )
        .unwrap();
        assert_eq!(l.name, "My Light");
        assert_eq!(l.theme.bg_canvas, rgb(0xF6F8FA).into());
        assert_eq!(l.theme.accent_blue, rgb(0x8250DF).into());
        assert_eq!(l.theme.text_primary, rgb(0x1F2328).into());
    }

    #[test]
    fn rgba_and_unknown_fields_are_accepted() {
        let t = parse_theme("x", "border = \"#11223344\"\nfuture = \"x\"\n").unwrap();
        assert_eq!(t.theme.border_card, rgba(0x11223344).into());
    }

    #[test]
    fn invalid_color_reports_file_and_field() {
        let e = parse_theme("bad", "canvas = \"red\"\n").unwrap_err();
        assert_eq!(e.file, "bad.toml");
        assert_eq!(e.field.as_deref(), Some("canvas"));
        assert!(e.to_string().starts_with("bad.toml: canvas:"));
        for bad in ["#12345", "#GGGGGG", "112233", "#1122334455"] {
            let e = parse_theme("bad", &format!("text = \"{bad}\"\n")).unwrap_err();
            assert_eq!(e.field.as_deref(), Some("text"), "{bad}");
        }
        let e = parse_theme("bad", "[overrides]\nbg_hover = \"nope\"\n").unwrap_err();
        assert_eq!(e.field.as_deref(), Some("overrides.bg_hover"));
        let e = parse_theme("bad", "base = \"neon\"\n").unwrap_err();
        assert_eq!(e.field.as_deref(), Some("base"));
        let e = parse_theme("bad", "canvas = [\n").unwrap_err();
        assert_eq!(e.file, "bad.toml");
    }

    #[test]
    fn scan_handles_missing_and_empty_dir() {
        let missing = std::env::temp_dir().join("sasha_theme_test_definitely_missing_dir");
        let scan = scan_dir(&missing);
        assert_eq!(scan.themes.len(), 10);
        assert!(scan.errors.is_empty());
        let dir = TempDir::new("empty");
        let scan = scan_dir(&dir.path);
        assert_eq!(scan.themes.len(), 10);
        assert!(scan.errors.is_empty());
    }

    #[test]
    fn scan_sorts_isolates_bad_files_and_reports_conflicts() {
        let dir = TempDir::new("scan");
        dir.write("zeta.toml", "name = \"Zeta\"\n");
        dir.write("alpha.toml", "base = \"light\"\n");
        dir.write("broken.toml", "canvas = \"oops\"\n");
        dir.write("dark.toml", "canvas = \"#FFFFFF\"\n");
        dir.write("notes.txt", "ignored");
        let scan = scan_dir(&dir.path);
        let ids: Vec<_> = scan.themes.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "dark",
                "light",
                "claude-dark",
                "claude-light",
                "gemini-dark",
                "gemini-light",
                "deepseek-dark",
                "deepseek-light",
                "grok-dark",
                "grok-light",
                "alpha",
                "zeta"
            ]
        );
        assert!(scan.themes[0].builtin);
        assert_eq!(scan.themes[0].theme.bg_canvas, rgb(0x0D1117).into());
        assert_eq!(scan.themes[11].name, "Zeta");
        assert_eq!(scan.errors.len(), 2);
        let broken = scan
            .errors
            .iter()
            .find(|e| e.file == "broken.toml")
            .unwrap();
        assert_eq!(broken.kind, ThemeErrorKind::Parse);
        let conflict = scan.errors.iter().find(|e| e.file == "dark.toml").unwrap();
        assert_eq!(conflict.kind, ThemeErrorKind::Conflict);
    }

    #[test]
    fn resolve_finds_or_falls_back_to_dark() {
        let dir = TempDir::new("resolve");
        dir.write("mine.toml", "base = \"light\"\n");
        dir.write("broken.toml", "text = \"x\"\n");
        let scan = scan_dir(&dir.path);

        let (t, w) = resolve("light", &scan.themes);
        assert!(w.is_none());
        assert_eq!(t.bg_canvas, rgb(0xF6F8FA).into());

        let (t, w) = resolve("gone", &scan.themes);
        assert_eq!(t.bg_canvas, NanoTheme::default().bg_canvas);
        assert_eq!(w.unwrap().kind, ThemeWarningKind::Missing);

        let (e, w) = resolve_scan("broken", &scan);
        assert_eq!(e.id, "dark");
        assert!(matches!(w.unwrap().kind, ThemeWarningKind::Broken(_)));

        let (e, w) = resolve_scan("mine", &scan);
        assert_eq!(e.base, ThemeBase::Light);
        assert!(w.is_none());
    }

    #[test]
    fn contrast_foreground_keeps_white_on_builtin_dark_accent() {
        assert_eq!(contrast_foreground(rgb(0x2563EB).into()), white());
        assert_eq!(contrast_foreground(rgb(0xFACC15).into()), black());
        assert_eq!(contrast_foreground(rgb(0xFFFFFF).into()), black());
        assert_eq!(contrast_foreground(rgb(0x000000).into()), white());
        let light = &builtin_themes()[1];
        assert_eq!(contrast_foreground(light.theme.accent_blue), white());
    }

    #[test]
    fn search_highlights_keep_text_readable_on_both_bases() {
        let (n, a) = search_highlight_colors(rgb(0x0D1117).into());
        assert_eq!((n, a), (rgba(0xEAB30840).into(), rgba(0xF59E0B99).into()));
        for entry in builtin_themes() {
            let t = &entry.theme;
            let (n, a) = search_highlight_colors(t.bg_canvas);
            if entry.base.is_dark() {
                continue; // dark colors are the pre-existing look
            }
            for h in [n, a] {
                let blended = mix(t.bg_canvas, h.alpha(1.0), h.a);
                assert!(
                    contrast_ratio(t.text_primary, blended) >= 4.5,
                    "{}: {h:?}",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn hover_over_keeps_the_surface_opacity() {
        for entry in builtin_themes() {
            let t = entry.theme.with_surface_opacity(0.84);
            let hovered = t.hover_over(t.bg_card);
            assert!(hovered.a >= t.bg_card.a - 0.01, "{}", entry.id);
            assert!(hovered.a > t.bg_hover.a, "{}", entry.id);
        }
    }

    #[test]
    fn send_card_shadow_is_soft_and_theme_independent() {
        let (alpha, _, blur) = send_card_shadow();
        assert!((0.10..=0.14).contains(&alpha));
        assert!(blur <= 8.0);
    }

    #[test]
    fn watch_plan_covers_directory_lifecycle() {
        let h = |dir_exists, watching, dir_event, events_due| WatchHealth {
            dir_exists,
            watching,
            dir_event,
            events_due,
        };
        // Healthy and idle: nothing to do.
        assert_eq!(
            plan_watch(h(true, true, false, false)),
            WatchPlan::default()
        );
        // Debounced file events: rescan only.
        let p = plan_watch(h(true, true, false, true));
        assert!(p.rescan && !p.rebuild_watcher && !p.drop_watcher);
        // Directory deleted while watched: drop watcher, rescan.
        let p = plan_watch(h(false, true, true, false));
        assert!(p.drop_watcher && p.rescan && !p.rebuild_watcher);
        // Still missing and unwatched: idle (periodic check does nothing).
        assert_eq!(
            plan_watch(h(false, false, false, false)),
            WatchPlan::default()
        );
        // Recreated (no event seen) or failed earlier: rebuild + rescan.
        let p = plan_watch(h(true, false, false, false));
        assert!(p.rebuild_watcher && p.rescan && !p.drop_watcher);
        // Deleted and recreated between checks: event forces a rebuild.
        let p = plan_watch(h(true, true, true, false));
        assert!(p.rebuild_watcher && p.rescan);
    }

    #[test]
    fn deleted_themes_dir_returns_list_to_builtins_and_keeps_selected_id() {
        let dir = TempDir::new("deldir");
        dir.write("mine.toml", "accent = \"#FF0000\"\n");
        let mut st = ThemeState::load(&dir.path, "mine");
        assert_eq!(st.current().id, "mine");
        fs::remove_dir_all(&dir.path).unwrap();
        assert!(st.rescan());
        assert_eq!(st.themes().len(), 10);
        assert_eq!(st.current().id, "dark");
        assert_eq!(st.selected_id(), "mine");
        assert!(!st.messages().is_empty());
        fs::create_dir_all(&dir.path).unwrap();
        dir.write("mine.toml", "accent = \"#FF0000\"\n");
        assert!(st.rescan());
        assert_eq!(st.current().id, "mine");
    }

    #[test]
    fn documented_example_theme_parses() {
        let text = include_str!("../../docs/themes/example.toml");
        let entry = parse_theme("example", text).expect("example theme parses");
        assert_eq!(entry.name, "Ocean");
        assert_eq!(entry.base, ThemeBase::Dark);
        assert_eq!(entry.theme.accent_blue, rgb(0x14B8A6).into());
        assert_eq!(entry.theme.bg_strip, rgb(0x08111A).into());
        assert!(contrast_ratio(entry.theme.text_primary, entry.theme.bg_canvas) >= 4.5);
    }

    #[test]
    fn gpui_mapping_follows_theme_and_base() {
        let dark = builtin_dark();
        let m = gpui_theme_colors(&dark.theme, dark.base);
        assert_eq!(m.base, ThemeBase::Dark);
        assert_eq!(m.background, dark.theme.bg_canvas);
        assert_eq!(m.foreground, dark.theme.text_primary);
        assert_eq!(m.primary, dark.theme.accent_blue);
        assert_eq!(m.popover, dark.theme.bg_popover);
        assert_eq!(m.danger, rgb(0xC42B1C).into());

        let light = &builtin_themes()[1];
        let m = gpui_theme_colors(&light.theme, light.base);
        assert_eq!(m.base, ThemeBase::Light);
        assert_eq!(m.background, rgb(0xF6F8FA).into());
        assert!(contrast_ratio(m.primary, m.primary_foreground) >= 4.5);
    }

    #[test]
    fn debouncer_fires_once_after_quiet_period() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_millis(250));
        assert!(!d.poll(t0));
        d.note(t0);
        assert!(!d.poll(t0 + Duration::from_millis(100)));
        d.note(t0 + Duration::from_millis(200)); // burst extends the window
        assert!(!d.poll(t0 + Duration::from_millis(400)));
        assert!(d.poll(t0 + Duration::from_millis(450)));
        assert!(!d.is_pending());
        assert!(!d.poll(t0 + Duration::from_millis(900)));
    }

    #[test]
    fn only_toml_paths_trigger_rescans() {
        assert!(is_toml_path(Path::new("themes/a.toml")));
        assert!(is_toml_path(Path::new("themes/A.TOML")));
        assert!(!is_toml_path(Path::new("themes/a.toml.tmp")));
        assert!(!is_toml_path(Path::new("themes/.a.toml.swp")));
        assert!(!is_toml_path(Path::new("themes/a")));
    }

    #[test]
    fn light_surfaces_are_more_opaque_than_dark() {
        assert_eq!(surface_opacity(ThemeBase::Dark), 0.84);
        assert!(surface_opacity(ThemeBase::Light) > surface_opacity(ThemeBase::Dark));
    }

    #[test]
    fn state_rescan_applies_edits_and_falls_back_then_recovers() {
        let dir = TempDir::new("state");
        dir.write("mine.toml", "accent = \"#FF0000\"\n");
        let mut st = ThemeState::load(&dir.path, "mine");
        assert_eq!(st.current().id, "mine");
        assert!(st.fallback_warning().is_none());
        assert!(!st.rescan(), "no change, no notification");

        dir.write("mine.toml", "accent = \"#00FF00\"\n");
        assert!(st.rescan());
        assert_eq!(st.current().theme.accent_blue, rgb(0x00FF00).into());

        dir.write("mine.toml", "accent = \"bogus\"\n");
        assert!(st.rescan());
        assert_eq!(st.current().id, "dark");
        assert_eq!(st.selected_id(), "mine", "config id is never rewritten");
        assert!(matches!(
            st.fallback_warning().unwrap().kind,
            ThemeWarningKind::Broken(_)
        ));
        assert!(st.messages().len() >= 2);

        fs::remove_file(dir.path.join("mine.toml")).unwrap();
        assert!(st.rescan());
        assert_eq!(
            st.fallback_warning().unwrap().kind,
            ThemeWarningKind::Missing
        );

        dir.write("mine.toml", "accent = \"#0000FF\"\n");
        assert!(st.rescan());
        assert_eq!(st.current().id, "mine");
        assert!(st.fallback_warning().is_none());
        assert!(st.messages().is_empty());
    }

    #[test]
    fn state_select_switches_and_rejects_unknown() {
        let dir = TempDir::new("select");
        dir.write("mine.toml", "base = \"light\"\n");
        let mut st = ThemeState::load(&dir.path, "dark");
        assert!(st.select("light"));
        assert_eq!(st.current().base, ThemeBase::Light);
        assert!(st.select("mine"));
        assert_eq!(st.selected_id(), "mine");
        assert!(!st.select("nope"));
        assert_eq!(st.selected_id(), "mine");
        let ids: Vec<_> = st.themes().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "dark",
                "light",
                "claude-dark",
                "claude-light",
                "gemini-dark",
                "gemini-light",
                "deepseek-dark",
                "deepseek-light",
                "grok-dark",
                "grok-light",
                "mine"
            ]
        );
    }

    #[test]
    fn format_hex_roundtrips_parse_hex() {
        for text in ["#0B1620", "#FFFFFF1C", "#000000"] {
            assert_eq!(format_hex(parse_hex(text).unwrap()), text);
        }
    }

    #[test]
    fn serialize_then_parse_is_identity() {
        let dark = builtin_dark();
        let text = serialize_theme("X Name", ThemeBase::Dark, &dark.colors, None);
        let parsed = parse_theme("x", &text).unwrap();
        assert_eq!(parsed.name, "X Name");
        assert_eq!(parsed.base, ThemeBase::Dark);
        assert_eq!(parsed.colors, dark.colors);
        assert_eq!(parsed.theme, derive_theme(ThemeBase::Dark, &dark.colors));
    }

    #[test]
    fn serialize_keeps_overrides() {
        let dark = builtin_dark();
        let source = "base = \"dark\"\n[overrides]\nbg_strip = \"#08111A\"\n";
        let text = serialize_theme("X", ThemeBase::Dark, &dark.colors, Some(source));
        let parsed = parse_theme("x", &text).unwrap();
        assert_eq!(parsed.theme.bg_strip, parse_hex("#08111A").unwrap());
    }

    #[test]
    fn id_from_name() {
        assert_eq!(theme_id_from_name("Ocean Blue").unwrap(), "ocean-blue");
        assert_eq!(theme_id_from_name("我的主题").unwrap(), "我的主题");
        assert_eq!(theme_id_from_name("  a//b ").unwrap(), "a-b");
        for bad in ["///", "   ", ""] {
            assert_eq!(theme_id_from_name(bad), Err(ThemeIdError::Empty));
        }
        for bad in ["Dark", "light", "CON", "nul.", "Com1"] {
            assert_eq!(
                theme_id_from_name(bad),
                Err(ThemeIdError::Reserved),
                "{bad}"
            );
        }
    }

    #[test]
    fn write_new_refuses_existing_and_builtin() {
        let dir = TempDir::new("write_new");
        write_new_theme_file(&dir.path, "mine", "a = 1").unwrap();
        let err = write_new_theme_file(&dir.path, "mine", "b = 2").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(dir.path.join("mine.toml")).unwrap(),
            "a = 1"
        );
        assert!(write_new_theme_file(&dir.path, "dark", "x").is_err());
        assert!(overwrite_theme_file(&dir.path, "light", "x").is_err());
    }

    #[test]
    fn rescan_after_write_lists_new_theme_with_saved_colors() {
        let dir = TempDir::new("rescan_write");
        let mut st = ThemeState::load(&dir.path, "dark");
        assert!(st.entry("mine").is_none());
        let mut colors = builtin_dark().colors;
        colors.accent = parse_hex("#14B8A6").unwrap();
        let text = serialize_theme("Mine", ThemeBase::Dark, &colors, None);
        write_new_theme_file(&dir.path, "mine", &text).unwrap();
        assert!(st.rescan());
        let entry = st.entry("mine").expect("new theme listed");
        assert_eq!(entry.colors.accent, colors.accent);
        assert_eq!(entry.name, "Mine");
        assert!(!entry.builtin);
    }

    #[test]
    fn delete_removes_file_and_rejects_builtin() {
        let dir = TempDir::new("delete");
        dir.write("mine.toml", "a = 1");
        delete_theme_file(&dir.path, "mine").unwrap();
        assert!(!dir.path.join("mine.toml").exists());
        assert!(delete_theme_file(&dir.path, "dark").is_err());
    }
}
