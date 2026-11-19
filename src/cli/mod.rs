//! `sasha-cli`: headless serial tools for automation.

pub mod args;
pub mod exit;
pub mod matcher;
pub mod monitor;
pub mod run;
pub mod send;
pub mod session;

use std::ffi::OsString;
use std::io::Write;

use crate::serial::DetectedPort;

/// Entry point shared by the binary and the tests. Returns the process exit code.
pub fn run<I, T>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    use clap::Parser;
    use clap::error::ErrorKind;

    let cli = match args::Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let text = error.to_string();
            return if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = out.write_all(text.as_bytes());
                exit::CliExit::Ok.code()
            } else {
                let _ = err.write_all(text.as_bytes());
                exit::CliExit::Usage.code()
            };
        }
    };

    let result = match cli.command {
        args::Command::Ports(ports) => {
            let detected = crate::serial::enumerate_serial_ports_for(crate::i18n::Locale::EnUs);
            let _ = out.write_all(format_ports(&detected, ports.json).as_bytes());
            exit::CliExit::Ok
        }
        args::Command::Send(send) => send::run_send(send, out, err),
        args::Command::Monitor(monitor) => monitor::run_monitor(monitor, out, err),
        args::Command::Run(run) => run::run_script(run, out, err),
    };
    result.code()
}

/// Formats the `ports` output: a JSON array, or one `name<TAB>description` line per port.
pub fn format_ports(ports: &[DetectedPort], json: bool) -> String {
    if json {
        let items: Vec<serde_json::Value> = ports
            .iter()
            .map(|p| serde_json::json!({ "name": p.name, "description": p.description }))
            .collect();
        let mut text = serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string());
        text.push('\n');
        text
    } else {
        ports
            .iter()
            .map(|p| format!("{}\t{}\n", p.name, p.description))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_capture(args: &[&str]) -> (u8, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(args.iter().copied(), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn run_reports_usage_error_with_code_2() {
        for argv in [&["sasha-cli"][..], &["sasha-cli", "send"][..]] {
            let (code, out, err) = run_capture(argv);
            assert_eq!(code, 2, "argv {argv:?}");
            assert!(out.is_empty());
            assert!(!err.is_empty());
        }
    }

    #[test]
    fn help_and_version_exit_zero_on_stdout() {
        let (code, out, err) = run_capture(&["sasha-cli", "--help"]);
        assert_eq!(code, 0);
        assert!(err.is_empty());
        for sub in ["ports", "send", "monitor", "run"] {
            assert!(out.contains(sub), "help should mention {sub}");
        }
        let (code, out, _) = run_capture(&["sasha-cli", "--version"]);
        assert_eq!(code, 0);
        assert!(out.contains("sasha-cli"));
    }

    #[test]
    fn ports_output_formats() {
        let ports = vec![
            DetectedPort {
                name: "COM3".into(),
                description: "USB Serial".into(),
            },
            DetectedPort {
                name: "COM4".into(),
                description: "Bluetooth".into(),
            },
        ];
        let json: Vec<serde_json::Value> =
            serde_json::from_str(&format_ports(&ports, true)).unwrap();
        assert_eq!(json.len(), 2);
        assert_eq!(json[0]["name"], "COM3");
        assert_eq!(json[1]["description"], "Bluetooth");
        assert_eq!(
            format_ports(&ports, false),
            "COM3\tUSB Serial\nCOM4\tBluetooth\n"
        );
        assert_eq!(format_ports(&[], true).trim(), "[]");
        assert_eq!(format_ports(&[], false), "");
    }
}
