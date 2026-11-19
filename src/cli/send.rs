//! `sasha-cli send`: transmit once and optionally wait for a matching response.

use std::io::Write;
use std::time::{Duration, Instant};

use crate::checksum::Checksum;
use crate::model::SendLineEnding;
use crate::session_service::{RxRecvError, SessionService, SessionTxError};

use super::args::{LineEndingArg, SendArgs};
use super::exit::{CliError, CliExit};
use super::matcher::ExpectMatcher;
use super::session::LiveSession;

/// What a completed send produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Sent,
    Matched(Vec<u8>),
}

/// Parses hex pairs written as `"AA BB 01"` or `"AABB01"` (any case, any whitespace).
pub fn parse_hex_arg(input: &str) -> Result<Vec<u8>, CliError> {
    let digits: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.is_empty() {
        return Err(CliError::usage("--hex must contain at least one byte"));
    }
    if digits.len() % 2 != 0 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(CliError::usage(
            "--hex must be hex byte pairs such as \"AA BB 01\" or \"AABB01\"",
        ));
    }
    Ok(digits
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).unwrap_or("00");
            u8::from_str_radix(text, 16).unwrap_or(0)
        })
        .collect())
}

/// Builds the exact bytes to transmit from `--text`/`--hex`, `--line-ending` and `--checksum`.
pub fn build_payload(args: &SendArgs) -> Result<Vec<u8>, CliError> {
    if let Some(text) = &args.text {
        if args.checksum.is_some() {
            return Err(CliError::usage("--checksum only works with --hex"));
        }
        let ending = match args.line_ending {
            LineEndingArg::None => SendLineEnding::None,
            LineEndingArg::Crlf => SendLineEnding::CrLf,
            LineEndingArg::Lf => SendLineEnding::Lf,
        };
        let mut bytes = text.as_bytes().to_vec();
        bytes.extend_from_slice(ending.suffix_bytes());
        return Ok(bytes);
    }
    let hex = args
        .hex
        .as_deref()
        .ok_or_else(|| CliError::usage("give either --text or --hex"))?;
    let bytes = parse_hex_arg(hex)?;
    match args.checksum.as_deref() {
        None => Ok(bytes),
        Some(name) => {
            let algo = Checksum::from_name(name).ok_or_else(|| {
                CliError::usage(format!(
                    "unknown --checksum '{name}' (use sum8, xor8, crc8, crc16_modbus, crc16_ccitt or crc32)"
                ))
            })?;
            Ok(algo.append_to(bytes))
        }
    }
}

/// Sends `payload` and, when `expect` is given, waits for the pattern to match.
/// Receiving is subscribed before sending so a fast response is never missed.
pub fn send_flow(
    service: &SessionService,
    generation: u64,
    payload: Vec<u8>,
    expect: Option<(ExpectMatcher, Duration)>,
) -> Result<SendOutcome, CliError> {
    let subscriber = match &expect {
        Some(_) => Some(
            service
                .subscribe_rx(generation)
                .map_err(|err| CliError::open_failed(err.to_string()))?,
        ),
        None => None,
    };
    service
        .send_tx_blocking(generation, payload, None)
        .map_err(|err| match err {
            SessionTxError::WriteFailed(msg) => CliError::failure(format!("write failed: {msg}")),
            other => CliError::open_failed(other.to_string()),
        })?;
    let (Some((mut matcher, timeout)), Some(subscriber)) = (expect, subscriber) else {
        return Ok(SendOutcome::Sent);
    };
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(CliError::timeout(
                "timed out waiting for a matching response",
            ));
        }
        match subscriber.recv_timeout(deadline - now) {
            Ok(chunk) => {
                if let Some(response) = matcher.push(&chunk) {
                    return Ok(SendOutcome::Matched(response));
                }
            }
            Err(RxRecvError::Timeout) | Err(RxRecvError::Empty) => {}
            Err(RxRecvError::Disconnected(reason)) => {
                return Err(CliError::open_failed(match reason {
                    Some(reason) => format!("port disconnected: {reason}"),
                    None => "port disconnected".to_string(),
                }));
            }
            Err(err @ RxRecvError::Overflow { .. }) => {
                return Err(CliError::failure(err.to_string()));
            }
        }
    }
}

/// Text for stdout: the raw response, or a JSON object when `json` is set.
pub fn format_send_result(outcome: &SendOutcome, json: bool) -> String {
    match (outcome, json) {
        (SendOutcome::Sent, false) => String::new(),
        (SendOutcome::Sent, true) => serde_json::json!({ "sent": true }).to_string(),
        (SendOutcome::Matched(bytes), false) => String::from_utf8_lossy(bytes).into_owned(),
        (SendOutcome::Matched(bytes), true) => {
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
            serde_json::json!({
                "matched": true,
                "response_hex": hex.join(" "),
                "response_text": String::from_utf8_lossy(bytes),
            })
            .to_string()
        }
    }
}

/// Validates arguments, opens the port, sends, prints the result and closes the port.
pub fn run_send(args: SendArgs, out: &mut dyn Write, err: &mut dyn Write) -> CliExit {
    match send_once(&args) {
        Ok(outcome) => {
            let text = format_send_result(&outcome, args.json);
            if !text.is_empty() {
                let _ = writeln!(out, "{text}");
            }
            CliExit::Ok
        }
        Err(error) => {
            let _ = writeln!(err, "error: {}", error.message);
            error.exit
        }
    }
}

fn send_once(args: &SendArgs) -> Result<SendOutcome, CliError> {
    // Everything that can be rejected is rejected before the port is opened.
    let config = args.serial.to_config()?;
    let payload = build_payload(args)?;
    let expect = match &args.expect {
        Some(pattern) => Some((
            ExpectMatcher::new(pattern)?,
            Duration::from_millis(args.timeout),
        )),
        None => None,
    };
    let session = LiveSession::open(&config)?;
    let result = send_flow(&session.service, session.generation, payload, expect);
    session.close();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Command};
    use crate::session_service::PortWriter;
    use clap::Parser;
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread;

    fn send_args(extra: &[&str]) -> SendArgs {
        let mut argv = vec!["sasha-cli", "send", "--port", "COM3"];
        argv.extend_from_slice(extra);
        match Cli::try_parse_from(argv).expect("args parse").command {
            Command::Send(args) => args,
            other => panic!("expected send, got {other:?}"),
        }
    }

    fn parse_fails(extra: &[&str]) -> bool {
        let mut argv = vec!["sasha-cli", "send", "--port", "COM3"];
        argv.extend_from_slice(extra);
        Cli::try_parse_from(argv).is_err()
    }

    /// Records writes and, if asked, signals each write so a test thread can answer at once.
    struct TestWriter {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
        on_write: Option<Sender<()>>,
    }

    impl PortWriter for TestWriter {
        fn write_all_and_flush(&mut self, data: &[u8]) -> Result<(), String> {
            self.writes.lock().unwrap().push(data.to_vec());
            if let Some(signal) = &self.on_write {
                let _ = signal.send(());
            }
            Ok(())
        }
    }

    fn connected(
        on_write: Option<Sender<()>>,
    ) -> (Arc<SessionService>, u64, Arc<Mutex<Vec<Vec<u8>>>>) {
        let service = Arc::new(SessionService::new());
        let writes = Arc::new(Mutex::new(Vec::new()));
        let generation = service.attach_writer(TestWriter {
            writes: Arc::clone(&writes),
            on_write,
        });
        (service, generation, writes)
    }

    #[test]
    fn build_payload_text_appends_line_ending() {
        let crlf = build_payload(&send_args(&["--text", "AT", "--line-ending", "crlf"])).unwrap();
        assert_eq!(crlf, b"AT\r\n".to_vec());
        let lf = build_payload(&send_args(&["--text", "AT", "--line-ending", "lf"])).unwrap();
        assert_eq!(lf, b"AT\n".to_vec());
        let none = build_payload(&send_args(&["--text", "AT"])).unwrap();
        assert_eq!(none, b"AT".to_vec());
    }

    #[test]
    fn build_payload_hex_with_checksum() {
        let framed = build_payload(&send_args(&[
            "--hex",
            "01 03 00 00 00 0A",
            "--checksum",
            "crc16_modbus",
        ]))
        .unwrap();
        assert_eq!(framed, vec![1, 3, 0, 0, 0, 0x0A, 0xC5, 0xCD]);
        let compact = build_payload(&send_args(&["--hex", "AABB"])).unwrap();
        assert_eq!(compact, vec![0xAA, 0xBB]);
        let lower = build_payload(&send_args(&["--hex", "aa bb"])).unwrap();
        assert_eq!(lower, vec![0xAA, 0xBB]);
    }

    #[test]
    fn build_payload_rejects_bad_input_as_usage() {
        for extra in [
            &["--hex", "ZZ"][..],
            &["--hex", "A BB"][..],
            &["--hex", ""][..],
            &["--hex", "   "][..],
            &["--text", "AT", "--checksum", "crc8"][..],
            &["--hex", "AA", "--checksum", "md5"][..],
        ] {
            let err = build_payload(&send_args(extra)).expect_err("must fail");
            assert_eq!(err.exit, CliExit::Usage, "args {extra:?}");
        }
    }

    #[test]
    fn send_requires_exactly_one_payload_and_timeout_needs_expect() {
        assert!(parse_fails(&[]));
        assert!(parse_fails(&["--text", "A", "--hex", "AA"]));
        assert!(parse_fails(&["--text", "A", "--timeout", "500"]));
        assert!(!parse_fails(&[
            "--text",
            "A",
            "--expect",
            "OK",
            "--timeout",
            "500"
        ]));
    }

    #[test]
    fn send_flow_transmits_and_returns_matched_response() {
        let (service, generation, writes) = connected(None);
        let responder = {
            let service = Arc::clone(&service);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(50));
                service.publish_rx(generation, b"OK\r\n");
            })
        };
        let matcher = ExpectMatcher::new("OK").unwrap();
        let outcome = send_flow(
            &service,
            generation,
            b"AT\r\n".to_vec(),
            Some((matcher, Duration::from_secs(2))),
        )
        .unwrap();
        responder.join().unwrap();
        assert_eq!(outcome, SendOutcome::Matched(b"OK".to_vec()));
        assert_eq!(writes.lock().unwrap().clone(), vec![b"AT\r\n".to_vec()]);
    }

    #[test]
    fn send_flow_response_that_arrives_immediately_is_not_lost() {
        let (signal, written) = mpsc::channel();
        let (service, generation, _writes) = connected(Some(signal));
        let responder = {
            let service = Arc::clone(&service);
            thread::spawn(move || {
                if written.recv().is_ok() {
                    service.publish_rx(generation, b"READY\r\n");
                }
            })
        };
        let matcher = ExpectMatcher::new("READY").unwrap();
        let outcome = send_flow(
            &service,
            generation,
            b"go".to_vec(),
            Some((matcher, Duration::from_secs(2))),
        )
        .unwrap();
        responder.join().unwrap();
        assert_eq!(outcome, SendOutcome::Matched(b"READY".to_vec()));
    }

    #[test]
    fn send_flow_times_out_with_code_4() {
        let (service, generation, _writes) = connected(None);
        let matcher = ExpectMatcher::new("OK").unwrap();
        let err = send_flow(
            &service,
            generation,
            b"AT".to_vec(),
            Some((matcher, Duration::from_millis(100))),
        )
        .expect_err("no response arrives");
        assert_eq!(err.exit, CliExit::Timeout);
    }

    #[test]
    fn send_flow_without_expect_returns_sent_after_write() {
        let (service, generation, writes) = connected(None);
        let outcome = send_flow(&service, generation, b"hello".to_vec(), None).unwrap();
        assert_eq!(outcome, SendOutcome::Sent);
        assert_eq!(writes.lock().unwrap().clone(), vec![b"hello".to_vec()]);
    }

    #[test]
    fn send_flow_reports_a_disconnected_session() {
        let (service, generation, _writes) = connected(None);
        service.disconnect(None);
        let err = send_flow(&service, generation, b"x".to_vec(), None).expect_err("must fail");
        assert_eq!(err.exit, CliExit::OpenFailed);
    }

    #[test]
    fn format_send_result_variants() {
        let matched = SendOutcome::Matched(vec![0xFF, b'O', b'K']);
        let json: serde_json::Value =
            serde_json::from_str(&format_send_result(&matched, true)).unwrap();
        assert_eq!(json["matched"], true);
        assert_eq!(json["response_hex"], "FF 4F 4B");
        assert!(json["response_text"].as_str().unwrap().contains("OK"));
        assert_eq!(
            format_send_result(&SendOutcome::Matched(b"OK".to_vec()), false),
            "OK"
        );
        assert_eq!(format_send_result(&SendOutcome::Sent, false), "");
        let sent: serde_json::Value =
            serde_json::from_str(&format_send_result(&SendOutcome::Sent, true)).unwrap();
        assert_eq!(sent["sent"], true);
    }
}
