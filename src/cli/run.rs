//! `sasha-cli run`: execute a Lua script against a port without any UI.

use std::io::Write;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use crate::scripting::{ScriptEvent, ScriptLogLevel, ScriptManager, ScriptStatus, StopReason};
use crate::session_service::SessionService;

use super::args::RunArgs;
use super::exit::{CliError, CliExit};
use super::session::LiveSession;

const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// `[INFO] msg`, `[WARN] msg` or `[ERROR] msg`.
pub fn format_script_log(level: ScriptLogLevel, message: &str) -> String {
    let tag = match level {
        ScriptLogLevel::Info => "INFO",
        ScriptLogLevel::Warn => "WARN",
        ScriptLogLevel::Error => "ERROR",
    };
    format!("[{tag}] {message}")
}

fn print_logs(manager: &mut ScriptManager, out: &mut dyn Write) {
    for event in manager.drain_events() {
        if let ScriptEvent::Log(entry) = event {
            let _ = writeln!(out, "{}", format_script_log(entry.level, &entry.message));
            let _ = out.flush();
        }
    }
}

/// Runs `source` on an attached session until it finishes or `timeout` passes, writing the
/// script's log lines to `out` and problems to `err`.
pub fn script_flow(
    service: &SessionService,
    generation: u64,
    path: PathBuf,
    source: String,
    timeout: Option<Duration>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> CliExit {
    let mut manager = ScriptManager::new();
    let run_id = match manager.start_script(path, source, service, generation) {
        Ok(run_id) => run_id,
        Err(message) => {
            let _ = writeln!(err, "error: {message}");
            return CliExit::OpenFailed;
        }
    };
    let deadline = timeout.map(|t| Instant::now() + t);
    let mut timed_out = false;
    let status = loop {
        print_logs(&mut manager, out);
        let finished = manager
            .snapshots()
            .into_iter()
            .find(|snap| snap.run_id == run_id)
            .map(|snap| snap.status)
            .filter(|status| !status.is_active());
        if let Some(status) = finished {
            break status;
        }
        if !timed_out && deadline.is_some_and(|d| Instant::now() >= d) {
            timed_out = true;
            manager.stop_instance(run_id);
        }
        thread::sleep(POLL_INTERVAL);
    };
    manager.join_all();
    print_logs(&mut manager, out);

    if timed_out {
        let _ = writeln!(err, "error: script timed out");
        return CliExit::Timeout;
    }
    match status {
        ScriptStatus::Completed => CliExit::Ok,
        ScriptStatus::Failed { error } => {
            let _ = writeln!(err, "error: {error}");
            CliExit::Failure
        }
        ScriptStatus::Stopped {
            reason: StopReason::Disconnected,
        } => {
            let _ = writeln!(err, "error: port disconnected while the script was running");
            CliExit::OpenFailed
        }
        ScriptStatus::Stopped { reason } => {
            let _ = writeln!(err, "error: script {reason}");
            CliExit::Failure
        }
        ScriptStatus::Running | ScriptStatus::Stopping => CliExit::Failure,
    }
}

/// Reads the script, opens the port, runs the script and closes the port.
pub fn run_script(args: RunArgs, out: &mut dyn Write, err: &mut dyn Write) -> CliExit {
    match run_once(args, out, err) {
        Ok(exit) => exit,
        Err(error) => {
            let _ = writeln!(err, "error: {}", error.message);
            error.exit
        }
    }
}

fn run_once(args: RunArgs, out: &mut dyn Write, err: &mut dyn Write) -> Result<CliExit, CliError> {
    // The file and the arguments are checked before the port is opened.
    let source = std::fs::read_to_string(&args.script)
        .map_err(|e| CliError::failure(format!("cannot read {}: {e}", args.script.display())))?;
    let config = args.serial.to_config()?;
    let session = LiveSession::open(&config)?;
    let exit = script_flow(
        &session.service,
        session.generation,
        args.script,
        source,
        args.timeout.map(Duration::from_millis),
        out,
        err,
    );
    session.close();
    Ok(exit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_service::PortWriter;
    use std::sync::{Arc, Mutex};

    type Recorded = Arc<Mutex<Vec<Vec<u8>>>>;

    struct RecordingWriter(Recorded);

    impl PortWriter for RecordingWriter {
        fn write_all_and_flush(&mut self, data: &[u8]) -> Result<(), String> {
            self.0.lock().unwrap().push(data.to_vec());
            Ok(())
        }
    }

    fn connected() -> (Arc<SessionService>, u64, Recorded) {
        let service = Arc::new(SessionService::new());
        let writes = Arc::new(Mutex::new(Vec::new()));
        let generation = service.attach_writer(RecordingWriter(Arc::clone(&writes)));
        (service, generation, writes)
    }

    fn run_source(
        service: &SessionService,
        generation: u64,
        source: &str,
        timeout: Option<Duration>,
    ) -> (CliExit, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let exit = script_flow(
            service,
            generation,
            PathBuf::from("test.lua"),
            source.to_string(),
            timeout,
            &mut out,
            &mut err,
        );
        (
            exit,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn format_script_log_prefixes_levels() {
        assert_eq!(format_script_log(ScriptLogLevel::Info, "a"), "[INFO] a");
        assert_eq!(format_script_log(ScriptLogLevel::Warn, "b"), "[WARN] b");
        assert_eq!(format_script_log(ScriptLogLevel::Error, "c"), "[ERROR] c");
    }

    #[test]
    fn script_flow_completes_and_prints_logs() {
        let (service, generation, _writes) = connected();
        let (exit, out, err) = run_source(
            &service,
            generation,
            r#"print("hello") log.warn("careful")"#,
            None,
        );
        assert_eq!(exit, CliExit::Ok);
        assert!(out.contains("[INFO] hello"), "stdout was {out:?}");
        assert!(out.contains("[WARN] careful"), "stdout was {out:?}");
        assert!(err.is_empty(), "stderr was {err:?}");
    }

    #[test]
    fn script_flow_sends_through_the_port() {
        let (service, generation, writes) = connected();
        let (exit, _out, _err) =
            run_source(&service, generation, r#"serial.send_hex("AA BB")"#, None);
        assert_eq!(exit, CliExit::Ok);
        assert_eq!(writes.lock().unwrap().clone(), vec![vec![0xAA, 0xBB]]);
    }

    #[test]
    fn script_flow_failed_script_exits_1_and_reports_error_on_stderr() {
        let (service, generation, _writes) = connected();
        let (exit, _out, err) = run_source(&service, generation, r#"error("boom")"#, None);
        assert_eq!(exit, CliExit::Failure);
        assert!(err.contains("boom"), "stderr was {err:?}");
    }

    #[test]
    fn script_flow_timeout_stops_an_infinite_loop() {
        let (service, generation, _writes) = connected();
        let started = Instant::now();
        let (exit, _out, _err) = run_source(
            &service,
            generation,
            "while true do end",
            Some(Duration::from_millis(300)),
        );
        assert_eq!(exit, CliExit::Timeout);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn script_flow_disconnect_maps_to_open_failed_code() {
        let (service, generation, _writes) = connected();
        let closer = {
            let service = Arc::clone(&service);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(150));
                service.disconnect(None);
            })
        };
        let (exit, _out, err) = run_source(&service, generation, "task.sleep(5000)", None);
        closer.join().unwrap();
        assert_eq!(exit, CliExit::OpenFailed);
        assert!(!err.is_empty());
    }

    #[test]
    fn run_script_missing_file_is_a_failure_before_opening_the_port() {
        let args = RunArgs {
            script: PathBuf::from("definitely/not/here.lua"),
            serial: crate::cli::args::SerialArgs {
                port: "COM99".into(),
                baud: 115200,
                data_bits: 8,
                parity: crate::cli::args::ParityArg::None,
                stop_bits: crate::cli::args::StopBitsArg::One,
                flow_control: crate::cli::args::FlowControlArg::None,
                dtr: false,
                rts: false,
            },
            timeout: None,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let exit = run_script(args, &mut out, &mut err);
        assert_eq!(exit, CliExit::Failure);
        let err = String::from_utf8(err).unwrap();
        assert!(err.contains("not/here.lua"), "stderr was {err:?}");
    }
}
