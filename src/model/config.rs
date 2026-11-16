use std::fmt;

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
}

impl ActivePickerKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Port => "选择或输入端口（模拟）",
            Self::BaudRate => "选择或输入波特率",
            Self::DataBits => "选择或输入数据位",
            Self::Parity => "选择或输入校验位",
            Self::StopBits => "选择或输入停止位",
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Port => "输入端口名（如 COM3）或选择预设",
            Self::BaudRate => "输入正整数波特率（如 230400）或选择预设",
            Self::DataBits => "输入 5 / 6 / 7 / 8 或选择预设",
            Self::Parity => "输入 无校验 / 奇校验 / 偶校验 或选择预设",
            Self::StopBits => "输入 1 / 1.5 / 2 或选择预设",
        }
    }

    pub fn presets(self) -> Vec<String> {
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
            Self::Parity => Parity::ALL.iter().map(|p| p.label().to_string()).collect(),
            Self::StopBits => StopBits::ALL
                .iter()
                .map(|s| s.label().to_string())
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

    pub fn parse(raw: &str) -> Result<Self, &'static str> {
        match raw.trim() {
            "5" => Ok(Self::Five),
            "6" => Ok(Self::Six),
            "7" => Ok(Self::Seven),
            "8" => Ok(Self::Eight),
            _ => Err("数据位仅支持 5、6、7、8"),
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

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "无校验",
            Self::Odd => "奇校验",
            Self::Even => "偶校验",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, &'static str> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "无校验" | "none" | "n" => Ok(Self::None),
            "奇校验" | "odd" | "o" => Ok(Self::Odd),
            "偶校验" | "even" | "e" => Ok(Self::Even),
            _ => Err("校验位仅支持 无校验、奇校验、偶校验"),
        }
    }
}

impl fmt::Display for Parity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
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

    pub fn parse(raw: &str) -> Result<Self, &'static str> {
        match raw.trim() {
            "1" => Ok(Self::One),
            "1.5" => Ok(Self::OnePointFive),
            "2" => Ok(Self::Two),
            _ => Err("停止位仅支持 1、1.5、2"),
        }
    }
}

impl fmt::Display for StopBits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Validates a simulated port name input.
pub fn parse_port(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("端口名称不能为空（如 COM3）");
    }
    Ok(trimmed.to_string())
}

/// Validates a baud rate input as a positive integer (`>= 1`).
pub fn parse_baud_rate(raw: &str) -> Result<u32, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("请输入正整数波特率（如 115200）");
    }
    match trimmed.parse::<u32>() {
        Ok(val) if val > 0 => Ok(val),
        _ => Err("波特率必须为正整数（如 115200 或 230400）"),
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
    pub fn current_value_str(&self, kind: ActivePickerKind) -> String {
        match kind {
            ActivePickerKind::Port => self.port.clone(),
            ActivePickerKind::BaudRate => self.baud_rate.to_string(),
            ActivePickerKind::DataBits => self.data_bits.label().to_string(),
            ActivePickerKind::Parity => self.parity.label().to_string(),
            ActivePickerKind::StopBits => self.stop_bits.label().to_string(),
        }
    }

    /// Validates and applies a parameter value. Leaves `self` unchanged on error.
    pub fn apply_picker_value(
        &mut self,
        kind: ActivePickerKind,
        raw: &str,
    ) -> Result<(), &'static str> {
        match kind {
            ActivePickerKind::Port => {
                self.port = parse_port(raw)?;
            }
            ActivePickerKind::BaudRate => {
                self.baud_rate = parse_baud_rate(raw)?;
            }
            ActivePickerKind::DataBits => {
                self.data_bits = DataBits::parse(raw)?;
            }
            ActivePickerKind::Parity => {
                self.parity = Parity::parse(raw)?;
            }
            ActivePickerKind::StopBits => {
                self.stop_bits = StopBits::parse(raw)?;
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
