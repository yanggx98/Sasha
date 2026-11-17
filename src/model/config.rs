use std::borrow::Cow;
use std::fmt;

use crate::i18n::{Locale, tr};
use crate::model::settings::{
    LOG_BUFFER_LIMIT_PRESETS, format_log_buffer_limit, parse_log_buffer_limit_for,
};

/// Preset baud rates for the quick-select overlay (used by TASK-002).
pub const BAUD_RATE_PRESETS: &[u32] = &[9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

/// Preset simulated serial ports (used by TASK-002).
pub const SIMULATED_PORT_PRESETS: &[&str] = &["COM1", "COM3", "COM4", "COM7"];

/// Identifies which bottom-bar parameter picker overlay is active (TASK-002 boundary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePickerKind {
    Port,
    BaudRate,
    DataBits,
    Parity,
    StopBits,
    BufferLimit,
}

impl ActivePickerKind {
    #[allow(dead_code)]
    pub fn title(self) -> Cow<'static, str> {
        self.title_for(Locale::ZhCn)
    }

    pub fn title_for(self, locale: Locale) -> Cow<'static, str> {
        match self {
            Self::Port => tr(locale, "picker.title_port_sim"),
            Self::BaudRate => tr(locale, "picker.title_baud"),
            Self::DataBits => tr(locale, "picker.title_data_bits"),
            Self::Parity => tr(locale, "picker.title_parity"),
            Self::StopBits => tr(locale, "picker.title_stop_bits"),
            Self::BufferLimit => tr(locale, "picker.title_buffer"),
        }
    }

    #[allow(dead_code)]
    pub fn placeholder(self) -> Cow<'static, str> {
        self.placeholder_for(Locale::ZhCn)
    }

    pub fn placeholder_for(self, locale: Locale) -> Cow<'static, str> {
        match self {
            Self::Port => tr(locale, "picker.placeholder_port"),
            Self::BaudRate => tr(locale, "picker.placeholder_baud"),
            Self::DataBits => tr(locale, "picker.placeholder_data_bits"),
            Self::Parity => tr(locale, "picker.placeholder_parity"),
            Self::StopBits => tr(locale, "picker.placeholder_stop_bits"),
            Self::BufferLimit => tr(locale, "picker.placeholder_buffer"),
        }
    }

    #[allow(dead_code)]
    pub fn presets(self) -> Vec<String> {
        self.presets_for(Locale::ZhCn)
    }

    pub fn presets_for(self, locale: Locale) -> Vec<String> {
        match self {
            Self::Port => SIMULATED_PORT_PRESETS
                .iter()
                .map(|p| (*p).to_string())
                .collect(),
            Self::BaudRate => BAUD_RATE_PRESETS.iter().map(|b| b.to_string()).collect(),
            Self::DataBits => DataBits::ALL
                .iter()
                .map(|d| d.label().to_string())
                .collect(),
            Self::Parity => Parity::ALL
                .iter()
                .map(|p| p.label_for(locale).to_string())
                .collect(),
            Self::StopBits => StopBits::ALL
                .iter()
                .map(|s| s.label().to_string())
                .collect(),
            Self::BufferLimit => LOG_BUFFER_LIMIT_PRESETS
                .iter()
                .copied()
                .map(format_log_buffer_limit)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

impl DataBits {
    pub const ALL: [Self; 4] = [Self::Five, Self::Six, Self::Seven, Self::Eight];

    pub fn label(self) -> &'static str {
        match self {
            Self::Five => "5",
            Self::Six => "6",
            Self::Seven => "7",
            Self::Eight => "8",
        }
    }

    #[allow(dead_code)]
    pub fn parse(raw: &str) -> Result<Self, Cow<'static, str>> {
        Self::parse_for(raw, Locale::ZhCn)
    }

    pub fn parse_for(raw: &str, locale: Locale) -> Result<Self, Cow<'static, str>> {
        match raw.trim() {
            "5" => Ok(Self::Five),
            "6" => Ok(Self::Six),
            "7" => Ok(Self::Seven),
            "8" => Ok(Self::Eight),
            _ => Err(tr(locale, "err.data_bits")),
        }
    }
}

impl fmt::Display for DataBits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
}

impl Parity {
    pub const ALL: [Self; 3] = [Self::None, Self::Odd, Self::Even];

    pub fn label(self) -> Cow<'static, str> {
        self.label_for(Locale::ZhCn)
    }

    pub fn label_for(self, locale: Locale) -> Cow<'static, str> {
        match self {
            Self::None => tr(locale, "parity.none"),
            Self::Odd => tr(locale, "parity.odd"),
            Self::Even => tr(locale, "parity.even"),
        }
    }

    #[allow(dead_code)]
    pub fn parse(raw: &str) -> Result<Self, Cow<'static, str>> {
        Self::parse_for(raw, Locale::ZhCn)
    }

    pub fn parse_for(raw: &str, locale: Locale) -> Result<Self, Cow<'static, str>> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "无校验" | "none" | "n" => Ok(Self::None),
            "奇校验" | "odd" | "o" => Ok(Self::Odd),
            "偶校验" | "even" | "e" => Ok(Self::Even),
            _ => Err(tr(locale, "err.parity")),
        }
    }
}

impl fmt::Display for Parity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    OnePointFive,
    Two,
}

impl StopBits {
    pub const ALL: [Self; 3] = [Self::One, Self::OnePointFive, Self::Two];

    pub fn label(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::OnePointFive => "1.5",
            Self::Two => "2",
        }
    }

    #[allow(dead_code)]
    pub fn parse(raw: &str) -> Result<Self, Cow<'static, str>> {
        Self::parse_for(raw, Locale::ZhCn)
    }

    pub fn parse_for(raw: &str, locale: Locale) -> Result<Self, Cow<'static, str>> {
        match raw.trim() {
            "1" => Ok(Self::One),
            "1.5" => Ok(Self::OnePointFive),
            "2" => Ok(Self::Two),
            _ => Err(tr(locale, "err.stop_bits")),
        }
    }
}

impl fmt::Display for StopBits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Validates a simulated port name input.
#[allow(dead_code)]
pub fn parse_port(raw: &str) -> Result<String, Cow<'static, str>> {
    parse_port_for(raw, Locale::ZhCn)
}

pub fn parse_port_for(raw: &str, locale: Locale) -> Result<String, Cow<'static, str>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(tr(locale, "err.port_empty"));
    }
    Ok(trimmed.to_string())
}

/// Validates a baud rate input as a positive integer (`>= 1`).
#[allow(dead_code)]
pub fn parse_baud_rate(raw: &str) -> Result<u32, Cow<'static, str>> {
    parse_baud_rate_for(raw, Locale::ZhCn)
}

pub fn parse_baud_rate_for(raw: &str, locale: Locale) -> Result<u32, Cow<'static, str>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(tr(locale, "err.baud_empty"));
    }
    match trimmed.parse::<u32>() {
        Ok(val) if val > 0 => Ok(val),
        _ => Err(tr(locale, "err.baud_invalid")),
    }
}

/// UI-level serial configuration model (does not imply real hardware support).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialConfig {
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

impl SerialConfig {
    #[allow(dead_code)]
    pub fn current_value_str(&self, kind: ActivePickerKind) -> String {
        self.current_value_str_for(kind, Locale::ZhCn)
    }

    pub fn current_value_str_for(&self, kind: ActivePickerKind, locale: Locale) -> String {
        match kind {
            ActivePickerKind::Port => self.port.clone(),
            ActivePickerKind::BaudRate => self.baud_rate.to_string(),
            ActivePickerKind::DataBits => self.data_bits.label().to_string(),
            ActivePickerKind::Parity => self.parity.label_for(locale).to_string(),
            ActivePickerKind::StopBits => self.stop_bits.label().to_string(),
            ActivePickerKind::BufferLimit => String::new(),
        }
    }

    /// Validates and applies a parameter value. Leaves `self` unchanged on error.
    #[allow(dead_code)]
    pub fn apply_picker_value(
        &mut self,
        kind: ActivePickerKind,
        raw: &str,
    ) -> Result<(), Cow<'static, str>> {
        self.apply_picker_value_for(kind, raw, Locale::ZhCn)
    }

    pub fn apply_picker_value_for(
        &mut self,
        kind: ActivePickerKind,
        raw: &str,
        locale: Locale,
    ) -> Result<(), Cow<'static, str>> {
        match kind {
            ActivePickerKind::Port => {
                self.port = parse_port_for(raw, locale)?;
            }
            ActivePickerKind::BaudRate => {
                self.baud_rate = parse_baud_rate_for(raw, locale)?;
            }
            ActivePickerKind::DataBits => {
                self.data_bits = DataBits::parse_for(raw, locale)?;
            }
            ActivePickerKind::Parity => {
                self.parity = Parity::parse_for(raw, locale)?;
            }
            ActivePickerKind::StopBits => {
                self.stop_bits = StopBits::parse_for(raw, locale)?;
            }
            ActivePickerKind::BufferLimit => {
                parse_log_buffer_limit_for(raw, locale)?;
            }
        }
        Ok(())
    }
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: "COM3".to_string(),
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baud_rate_accepts_positive_integers_and_rejects_invalid() {
        assert_eq!(parse_baud_rate("115200"), Ok(115200));
        assert_eq!(parse_baud_rate(" 230400 "), Ok(230400));
        assert_eq!(parse_baud_rate("460800"), Ok(460800));
        assert_eq!(parse_baud_rate("921600"), Ok(921600));
        assert_eq!(parse_baud_rate("1"), Ok(1));

        for &preset in BAUD_RATE_PRESETS {
            assert_eq!(parse_baud_rate(&preset.to_string()), Ok(preset));
        }
        assert!(BAUD_RATE_PRESETS.contains(&115200));
        assert!(BAUD_RATE_PRESETS.contains(&230400));
        assert!(BAUD_RATE_PRESETS.contains(&460800));
        assert!(BAUD_RATE_PRESETS.contains(&921600));

        assert!(parse_baud_rate("").is_err());
        assert!(parse_baud_rate("   ").is_err());
        assert!(parse_baud_rate("0").is_err());
        assert!(parse_baud_rate("-9600").is_err());
        assert!(parse_baud_rate("115200.5").is_err());
        assert!(parse_baud_rate("abc").is_err());
    }

    #[test]
    fn apply_picker_value_preserves_original_on_invalid_input() {
        let mut cfg = SerialConfig::default();
        assert!(
            cfg.apply_picker_value(ActivePickerKind::BaudRate, "0")
                .is_err()
        );
        assert_eq!(cfg.baud_rate, 115200);

        assert!(
            cfg.apply_picker_value(ActivePickerKind::BaudRate, "230400")
                .is_ok()
        );
        assert_eq!(cfg.baud_rate, 230400);

        assert!(
            cfg.apply_picker_value(ActivePickerKind::Port, "  ")
                .is_err()
        );
        assert_eq!(cfg.port, "COM3");

        assert!(
            cfg.apply_picker_value(ActivePickerKind::Port, "COM8")
                .is_ok()
        );
        assert_eq!(cfg.port, "COM8");

        assert!(
            cfg.apply_picker_value(ActivePickerKind::DataBits, "9")
                .is_err()
        );
        assert_eq!(cfg.data_bits, DataBits::Eight);
        assert!(
            cfg.apply_picker_value(ActivePickerKind::DataBits, "7")
                .is_ok()
        );
        assert_eq!(cfg.data_bits, DataBits::Seven);

        assert!(
            cfg.apply_picker_value(ActivePickerKind::Parity, "invalid")
                .is_err()
        );
        assert_eq!(cfg.parity, Parity::None);
        assert!(
            cfg.apply_picker_value(ActivePickerKind::Parity, "偶校验")
                .is_ok()
        );
        assert_eq!(cfg.parity, Parity::Even);

        assert!(
            cfg.apply_picker_value(ActivePickerKind::StopBits, "3")
                .is_err()
        );
        assert_eq!(cfg.stop_bits, StopBits::One);
        assert!(
            cfg.apply_picker_value(ActivePickerKind::StopBits, "1.5")
                .is_ok()
        );
        assert_eq!(cfg.stop_bits, StopBits::OnePointFive);
    }
}
