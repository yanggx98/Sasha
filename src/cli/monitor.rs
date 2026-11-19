//! `sasha-cli monitor`: listen without sending, until a time limit, a match or Ctrl+C.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::model::current_timestamp_hms_ms;
use crate::session_service::{RxRecvError, SessionService};

use super::args::MonitorArgs;
use super::exit::{CliError, CliExit};
use super::matcher::ExpectMatcher;
use super::session::LiveSession;

/// How often the loop wakes to check the stop flag, the time limit and disconnects.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Turns received chunks into output lines.
pub struct MonitorFormatter {
    hex: bool,
    timestamp: bool,
    json: bool,
    pending: Vec<u8>,
}

impl MonitorFormatter {
    pub fn new(hex: bool, timestamp: bool, json: bool) -> Self {
        Self {
            hex,
            timestamp,
            json,
            pending: Vec::new(),
        }
    }

    /// Lines completed by `chunk`. Text mode keeps an unfinished line for the next chunk.
    pub fn feed(&mut self, ts: &str, chunk: &[u8]) -> Vec<String> {
        if self.json {
            return vec![self.json_line(ts, chunk)];
        }
        if self.hex {
            return vec![self.stamped(ts, hex_text(chunk))];
        }
        self.pending.extend_from_slice(chunk);
        let mut lines = Vec::new();
        while let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = self.pending.drain(..=pos).collect();
            lines.push(self.stamped(ts, text_line(&raw)));
        }
        lines
    }

    /// The unfinished text line, if any.
    pub fn finish(&mut self, ts: &str) -> Vec<String> {
        if self.pending.is_empty() {
            return Vec::new();
        }
        let raw = std::mem::take(&mut self.pending);
        vec![self.stamped(ts, text_line(&raw))]
    }

    fn stamped(&self, ts: &str, text: String) -> String {
        if self.timestamp {
            format!("[{ts}] {text}")
        } else {
            text
        }
    }

    fn json_line(&self, ts: &str, chunk: &[u8]) -> String {
        serde_json::json!({
            "ts": ts,
            "direction": "rx",
            "hex": hex_text(chunk),
            "text": String::from_utf8_lossy(chunk),
        })
        .to_string()
    }
}

fn hex_text(bytes: &[u8]) -> String {
    let parts: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    parts.join(" ")
}

/// A text line without its line ending.
fn text_line(raw: &[u8]) -> String {
    let mut end = raw.len();
    if raw[..end].ends_with(b"\n") {
        end -= 1;
    }
    if raw[..end].ends_with(b"\r") {
        end -= 1;
    }
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// Why `monitor_flow` returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitorEnd {
    Duration,
    Matched,
    Interrupted,
    Disconnected(String),
}

pub struct MonitorOptions {
    pub duration: Option<Duration>,
    pub until: Option<ExpectMatcher>,
    pub formatter: MonitorFormatter,
}

/// Streams received data through `emit` until one of the end conditions is reached.
pub fn monitor_flow(
    service: &SessionService,
    generation: u64,
    opts: &mut MonitorOptions,
    stop: &AtomicBool,
    emit: &mut dyn FnMut(String),
) -> MonitorEnd {
    let subscriber = match service.subscribe_rx(generation) {
        Ok(subscriber) => subscriber,
        Err(err) => return MonitorEnd::Disconnected(err.to_string()),
    };
    let deadline = opts.duration.map(|d| Instant::now() + d);
    let finish = |opts: &mut MonitorOptions, emit: &mut dyn FnMut(String), end: MonitorEnd| {
        for line in opts.formatter.finish(&current_timestamp_hms_ms()) {
            emit(line);
        }
        end
    };
    loop {
        if stop.load(Ordering::SeqCst) {
            return finish(opts, emit, MonitorEnd::Interrupted);
        }
        let wait = match deadline {
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return finish(opts, emit, MonitorEnd::Duration);
                }
                (deadline - now).min(POLL_INTERVAL)
            }
            None => POLL_INTERVAL,
        };
        match subscriber.recv_timeout(wait) {
            Ok(chunk) => {
                for line in opts.formatter.feed(&current_timestamp_hms_ms(), &chunk) {
                    emit(line);
                }
                if let Some(until) = opts.until.as_mut()
                    && until.push(&chunk).is_some()
                {
                    return finish(opts, emit, MonitorEnd::Matched);
                }
            }
            Err(RxRecvError::Timeout) | Err(RxRecvError::Empty) => {}
            Err(RxRecvError::Disconnected(reason)) => {
                let reason = reason.unwrap_or_else(|| "port disconnected".to_string());
                return finish(opts, emit, MonitorEnd::Disconnected(reason));
            }
            Err(err @ RxRecvError::Overflow { .. }) => {
                return finish(opts, emit, MonitorEnd::Disconnected(err.to_string()));
            }
        }
    }
}

/// Exit code for each way a monitor can end.
pub fn exit_for(end: &MonitorEnd, had_until: bool) -> CliExit {
    match end {
        MonitorEnd::Duration if had_until => CliExit::Timeout,
        MonitorEnd::Duration | MonitorEnd::Matched | MonitorEnd::Interrupted => CliExit::Ok,
        MonitorEnd::Disconnected(_) => CliExit::OpenFailed,
    }
}

/// Validates arguments, opens the port, streams to stdout and closes the port.
pub fn run_monitor(args: MonitorArgs, out: &mut dyn Write, err: &mut dyn Write) -> CliExit {
    match monitor_once(&args, out, err) {
        Ok(exit) => exit,
        Err(error) => {
            let _ = writeln!(err, "error: {}", error.message);
            error.exit
        }
    }
}

fn monitor_once(
    args: &MonitorArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<CliExit, CliError> {
    // Everything that can be rejected is rejected before the port is opened.
    let config = args.serial.to_config()?;
    let until = match &args.until {
        Some(pattern) => Some(ExpectMatcher::new(pattern)?),
        None => None,
    };
    let had_until = until.is_some();
    let mut opts = MonitorOptions {
        duration: args.duration.map(Duration::from_millis),
        until,
        formatter: MonitorFormatter::new(args.hex, args.timestamp, args.json),
    };

    let stop = Arc::new(AtomicBool::new(false));
    let handler_stop = Arc::clone(&stop);
    // Only the first handler in a process can be installed; a failure just means Ctrl+C
    // keeps its default behaviour of ending the process.
    let _ = ctrlc::set_handler(move || handler_stop.store(true, Ordering::SeqCst));

    let session = LiveSession::open(&config)?;
    let mut write_failed = false;
    let end = monitor_flow(
        &session.service,
        session.generation,
        &mut opts,
        &stop,
        &mut |line| {
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                write_failed = true;
                stop.store(true, Ordering::SeqCst);
            }
        },
    );
    session.close();
    if let MonitorEnd::Disconnected(reason) = &end {
        let _ = writeln!(err, "error: {reason}");
    }
    if write_failed {
        return Ok(CliExit::Failure);
    }
    Ok(exit_for(&end, had_until))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_service::PortWriter;
    use std::thread;

    struct NullWriter;

    impl PortWriter for NullWriter {
        fn write_all_and_flush(&mut self, _data: &[u8]) -> Result<(), String> {
            Ok(())
        }
    }

    fn connected() -> (Arc<SessionService>, u64) {
        let service = Arc::new(SessionService::new());
        let generation = service.attach_writer(NullWriter);
        (service, generation)
    }

    fn options(duration: Option<u64>, until: Option<&str>) -> MonitorOptions {
        MonitorOptions {
            duration: duration.map(Duration::from_millis),
            until: until.map(|p| ExpectMatcher::new(p).unwrap()),
            formatter: MonitorFormatter::new(false, false, false),
        }
    }

    #[test]
    fn formatter_text_splits_lines_across_chunks() {
        let mut f = MonitorFormatter::new(false, false, false);
        assert!(f.feed("t", b"AB").is_empty());
        assert_eq!(f.feed("t", b"C\r\nD"), vec!["ABC".to_string()]);
        assert_eq!(f.finish("t"), vec!["D".to_string()]);
        assert!(f.finish("t").is_empty());
    }

    #[test]
    fn formatter_hex_and_timestamp() {
        let mut f = MonitorFormatter::new(true, true, false);
        assert_eq!(
            f.feed("12:00:00.001", &[0xAA, 0x0B]),
            vec!["[12:00:00.001] AA 0B".to_string()]
        );
        let mut text = MonitorFormatter::new(false, true, false);
        assert_eq!(
            text.feed("12:00:00.002", b"hi\n"),
            vec!["[12:00:00.002] hi".to_string()]
        );
    }

    #[test]
    fn formatter_json_is_one_valid_object_per_chunk() {
        let mut f = MonitorFormatter::new(false, false, true);
        let lines = f.feed("12:00:00.003", &[0xFF, b'O', b'K']);
        assert_eq!(lines.len(), 1);
        let value: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(value["ts"], "12:00:00.003");
        assert_eq!(value["direction"], "rx");
        assert_eq!(value["hex"], "FF 4F 4B");
        assert!(value["text"].as_str().unwrap().contains("OK"));
    }

    #[test]
    fn monitor_flow_stops_after_duration() {
        let (service, generation) = connected();
        let started = Instant::now();
        let end = monitor_flow(
            &service,
            generation,
            &mut options(Some(200), None),
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert_eq!(end, MonitorEnd::Duration);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn monitor_flow_stops_on_match_and_emits_lines() {
        let (service, generation) = connected();
        let publisher = {
            let service = Arc::clone(&service);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(80));
                service.publish_rx(generation, b"boot\r\nREADY\r\n");
            })
        };
        let mut lines = Vec::new();
        let end = monitor_flow(
            &service,
            generation,
            &mut options(Some(5000), Some("READY")),
            &AtomicBool::new(false),
            &mut |line| lines.push(line),
        );
        publisher.join().unwrap();
        assert_eq!(end, MonitorEnd::Matched);
        assert_eq!(lines, vec!["boot".to_string(), "READY".to_string()]);
    }

    #[test]
    fn monitor_flow_reports_disconnect() {
        let (service, generation) = connected();
        let closer = {
            let service = Arc::clone(&service);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(80));
                service.disconnect(Some("unplugged".into()));
            })
        };
        let end = monitor_flow(
            &service,
            generation,
            &mut options(None, None),
            &AtomicBool::new(false),
            &mut |_| {},
        );
        closer.join().unwrap();
        assert_eq!(end, MonitorEnd::Disconnected("unplugged".into()));
    }

    #[test]
    fn monitor_flow_honors_stop_flag() {
        let (service, generation) = connected();
        let stop = Arc::new(AtomicBool::new(false));
        let setter = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                stop.store(true, Ordering::SeqCst);
            })
        };
        let end = monitor_flow(
            &service,
            generation,
            &mut options(None, None),
            &stop,
            &mut |_| {},
        );
        setter.join().unwrap();
        assert_eq!(end, MonitorEnd::Interrupted);
    }

    #[test]
    fn exit_codes_for_each_end() {
        assert_eq!(exit_for(&MonitorEnd::Duration, false), CliExit::Ok);
        assert_eq!(exit_for(&MonitorEnd::Duration, true), CliExit::Timeout);
        assert_eq!(exit_for(&MonitorEnd::Matched, true), CliExit::Ok);
        assert_eq!(exit_for(&MonitorEnd::Interrupted, true), CliExit::Ok);
        assert_eq!(
            exit_for(&MonitorEnd::Disconnected("x".into()), false),
            CliExit::OpenFailed
        );
    }
}
