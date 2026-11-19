//! Command-line arguments for `sasha-cli`.

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::model::{DataBits, FlowControl, Parity, SerialConfig, StopBits};

use super::exit::CliError;

#[derive(Parser, Debug)]
#[command(
    name = "sasha-cli",
    version,
    about = "Headless serial port tools from Sasha"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List serial ports
    Ports(PortsArgs),
    /// Send data once, optionally waiting for a response
    Send(SendArgs),
    /// Listen to a port without sending
    Monitor(MonitorArgs),
    /// Run a Lua script against a port
    Run(RunArgs),
}

#[derive(Args, Debug)]
pub struct PortsArgs {
    /// Print a JSON array instead of tab-separated lines
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LineEndingArg {
    None,
    Crlf,
    Lf,
}

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("payload").required(true).args(["text", "hex"])))]
pub struct SendArgs {
    #[command(flatten)]
    pub serial: SerialArgs,
    /// Send this text (UTF-8)
    #[arg(long)]
    pub text: Option<String>,
    /// Send these bytes, as hex pairs ("AA BB 01" or "AABB01")
    #[arg(long)]
    pub hex: Option<String>,
    /// Line ending appended to --text
    #[arg(long, value_enum, default_value_t = LineEndingArg::None)]
    pub line_ending: LineEndingArg,
    /// Append a checksum to --hex: sum8, xor8, crc8, crc16_modbus, crc16_ccitt, crc32
    #[arg(long)]
    pub checksum: Option<String>,
    /// Wait for a response matching this regex after sending
    #[arg(long)]
    pub expect: Option<String>,
    /// How long to wait for --expect, in milliseconds
    #[arg(long, default_value_t = 1000, requires = "expect")]
    pub timeout: u64,
    /// Print the result as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct MonitorArgs {
    #[command(flatten)]
    pub serial: SerialArgs,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    #[command(flatten)]
    pub serial: SerialArgs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ParityArg {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StopBitsArg {
    #[value(name = "1")]
    One,
    #[value(name = "1.5")]
    OnePointFive,
    #[value(name = "2")]
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FlowControlArg {
    None,
    Hardware,
    Software,
}

/// Serial port parameters shared by `send`, `monitor` and `run`.
#[derive(Args, Debug, Clone)]
pub struct SerialArgs {
    /// Port name, e.g. COM3 or /dev/ttyUSB0
    #[arg(long)]
    pub port: String,
    #[arg(long, default_value_t = 115200)]
    pub baud: u32,
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u8).range(5..=8))]
    pub data_bits: u8,
    #[arg(long, value_enum, default_value_t = ParityArg::None)]
    pub parity: ParityArg,
    #[arg(long, value_enum, default_value = "1")]
    pub stop_bits: StopBitsArg,
    #[arg(long, value_enum, default_value_t = FlowControlArg::None)]
    pub flow_control: FlowControlArg,
    /// Drive DTR high when opening (default low)
    #[arg(long)]
    pub dtr: bool,
    /// Drive RTS high when opening (default low)
    #[arg(long)]
    pub rts: bool,
}

impl SerialArgs {
    pub fn to_config(&self) -> Result<SerialConfig, CliError> {
        let port = self.port.trim();
        if port.is_empty() {
            return Err(CliError::usage("--port must not be empty"));
        }
        if self.baud == 0 {
            return Err(CliError::usage("--baud must be greater than 0"));
        }
        let data_bits = match self.data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            _ => DataBits::Eight,
        };
        Ok(SerialConfig {
            port: port.to_string(),
            baud_rate: self.baud,
            data_bits,
            parity: match self.parity {
                ParityArg::None => Parity::None,
                ParityArg::Odd => Parity::Odd,
                ParityArg::Even => Parity::Even,
            },
            stop_bits: match self.stop_bits {
                StopBitsArg::One => StopBits::One,
                StopBitsArg::OnePointFive => StopBits::OnePointFive,
                StopBitsArg::Two => StopBits::Two,
            },
            flow_control: match self.flow_control {
                FlowControlArg::None => FlowControl::None,
                FlowControlArg::Hardware => FlowControl::Hardware,
                FlowControlArg::Software => FlowControl::Software,
            },
            dtr: self.dtr,
            rts: self.rts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_serial(extra: &[&str]) -> Result<SerialArgs, clap::Error> {
        #[derive(Parser)]
        struct Wrapper {
            #[command(flatten)]
            serial: SerialArgs,
        }
        let mut argv = vec!["t"];
        argv.extend_from_slice(extra);
        Wrapper::try_parse_from(argv).map(|w| w.serial)
    }

    #[test]
    fn serial_args_default_to_the_gui_defaults() {
        let config = parse_serial(&["--port", "COM3"])
            .unwrap()
            .to_config()
            .unwrap();
        assert_eq!(config, SerialConfig::default());
    }

    #[test]
    fn serial_args_map_every_option() {
        let config = parse_serial(&[
            "--port",
            "COM7",
            "--baud",
            "9600",
            "--data-bits",
            "7",
            "--parity",
            "even",
            "--stop-bits",
            "2",
            "--flow-control",
            "hardware",
            "--dtr",
            "--rts",
        ])
        .unwrap()
        .to_config()
        .unwrap();
        assert_eq!(config.port, "COM7");
        assert_eq!(config.baud_rate, 9600);
        assert_eq!(config.data_bits, DataBits::Seven);
        assert_eq!(config.parity, Parity::Even);
        assert_eq!(config.stop_bits, StopBits::Two);
        assert_eq!(config.flow_control, FlowControl::Hardware);
        assert!(config.dtr);
        assert!(config.rts);
    }

    #[test]
    fn invalid_serial_values_are_rejected() {
        assert!(parse_serial(&["--port", "COM3", "--data-bits", "9"]).is_err());
        assert!(parse_serial(&["--port", "COM3", "--parity", "bogus"]).is_err());
        let zero = parse_serial(&["--port", "COM3", "--baud", "0"]).unwrap();
        assert!(zero.to_config().is_err());
        let blank = parse_serial(&["--port", "  "]).unwrap();
        assert!(blank.to_config().is_err());
    }
}
