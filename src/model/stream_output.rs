//! Per-session, append-only capture independent of the display buffer.
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};

use super::{LogDirection, LogEntry};

static CAPTURE_PATHS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Debug)]
enum StreamMessage {
    Entry(LogEntry),
    History(Vec<LogEntry>),
}

struct PathLease(PathBuf);
impl Drop for PathLease {
    fn drop(&mut self) {
        CAPTURE_PATHS.get().unwrap().lock().unwrap().remove(&self.0);
    }
}

#[derive(Debug)]
pub struct StreamOutput {
    path: PathBuf,
    sender: Option<SyncSender<StreamMessage>>,
    errors: Receiver<String>,
    worker: Option<JoinHandle<()>>,
    error: Option<String>,
}

impl StreamOutput {
    /// Call on a background executor: opening a file can block.
    pub fn open(path: &Path, hex_mode: bool) -> io::Result<Self> {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        let file_len = file.metadata()?.len();
        let canonical = path.canonicalize()?;
        if !CAPTURE_PATHS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(canonical.clone())
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "This file is already used by a session",
            ));
        }
        let lease = PathLease(canonical);
        let mut head = [0; 4];
        let count = file_len.min(head.len() as u64) as usize;
        std::fs::File::open(path)?.read_exact(&mut head[..count])?;
        if head[..count].starts_with(b"\xFF\xFE")
            || head[..count].starts_with(b"\xFE\xFF")
            || head[..count].starts_with(b"\x00\x00\xFE\xFF")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Cannot append UTF-8 logs to a UTF-16/UTF-32 file; choose a new file",
            ));
        }
        let needs_separator = file_len > 0
            && !(file_len == UTF8_BOM.len() as u64 && head[..count] == *UTF8_BOM)
            && !ends_with_newline(path);
        if file_len == 0 {
            file.write_all(UTF8_BOM)?;
            file.flush()?;
        }
        let (sender, receiver) = mpsc::sync_channel::<StreamMessage>(1024);
        let (errors_tx, errors) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("sasha-log-stream".into())
            .spawn(move || {
                let _lease = lease;
                let mut formatter = StreamFormatter {
                    hex_mode,
                    open_line: None,
                    needs_separator,
                    pending_utf8: Vec::new(),
                };
                for message in receiver {
                    let entries = match message {
                        StreamMessage::Entry(entry) => vec![entry],
                        StreamMessage::History(entries) => entries,
                    };
                    for entry in entries {
                        let bytes = formatter.record(&entry);
                        if let Err(error) = file.write_all(&bytes).and_then(|_| file.flush()) {
                            let _ = errors_tx.send(error.to_string());
                            return;
                        }
                    }
                }
                if let Err(error) = file
                    .write_all(&formatter.finish())
                    .and_then(|_| file.flush())
                {
                    let _ = errors_tx.send(error.to_string());
                }
            })?;
        Ok(Self {
            path: path.to_owned(),
            sender: Some(sender),
            errors,
            worker: Some(worker),
            error: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_active(&self) -> bool {
        self.sender.is_some() && self.error.is_none()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn poll(&mut self) -> bool {
        if let Ok(error) = self.errors.try_recv() {
            self.error = Some(error);
            self.sender.take();
            true
        } else {
            false
        }
    }

    pub fn record(&mut self, entry: &LogEntry) {
        self.enqueue(StreamMessage::Entry(entry.clone()));
    }

    pub fn record_history(&mut self, entries: Vec<LogEntry>) {
        self.enqueue(StreamMessage::History(entries));
    }

    fn enqueue(&mut self, message: StreamMessage) {
        self.poll();
        let Some(sender) = &self.sender else {
            return;
        };
        if let Err(error) = sender.try_send(message) {
            self.error = Some(match error {
                TrySendError::Full(_) => "Log output queue is full; capture stopped".into(),
                TrySendError::Disconnected(_) => "Log output writer stopped".into(),
            });
            self.sender.take();
        }
    }

    /// Drain queued records on a background executor when explicitly stopping.
    pub fn finish(mut self) -> Option<String> {
        self.drain();
        self.error.take()
    }

    fn drain(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            self.error = Some("Log output writer panicked".into());
        }
        self.poll();
    }
}

impl Drop for StreamOutput {
    fn drop(&mut self) {
        self.drain();
    }
}

fn ends_with_newline(path: &Path) -> bool {
    let tail = || -> io::Result<bool> {
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::End(-1))?;
        let mut byte = [0];
        file.read_exact(&mut byte)?;
        Ok(byte[0] == b'\n')
    };
    tail().unwrap_or(false)
}

struct StreamFormatter {
    hex_mode: bool,
    open_line: Option<(LogDirection, bool)>,
    needs_separator: bool,
    pending_utf8: Vec<u8>,
}

impl StreamFormatter {
    fn record(&mut self, entry: &LogEntry) -> Vec<u8> {
        if entry.bytes.is_empty() {
            return Vec::new();
        }
        let mut output = Vec::new();
        if self.needs_separator {
            output.push(b'\n');
            self.needs_separator = false;
        }
        let prefix = format!(
            "[{}] {}{} ",
            entry.timestamp,
            entry.direction.tag(),
            if entry.is_simulated { " [SIM]" } else { "" }
        );
        if self.hex_mode {
            output
                .extend_from_slice(format!("{prefix}{}\n", entry.format_payload(true)).as_bytes());
            return output;
        }
        let key = (entry.direction, entry.is_simulated);
        // Only RX chunks continue an unfinished line. Each TX remains a distinct event.
        if self.open_line.is_some()
            && (self.open_line != Some(key) || entry.direction != LogDirection::Rx)
        {
            self.flush_pending(&mut output);
            output.push(b'\n');
            self.open_line = None;
        }
        for chunk in entry.bytes.split_inclusive(|&byte| byte == b'\n') {
            if self.open_line.is_none() {
                output.extend_from_slice(prefix.as_bytes());
            }
            self.encode_text(chunk, &mut output);
            self.open_line = if chunk.ends_with(b"\n") {
                None
            } else {
                Some(key)
            };
        }
        if entry.direction != LogDirection::Rx && self.open_line.take().is_some() {
            self.flush_pending(&mut output);
            output.push(b'\n');
        }
        output
    }

    fn finish(&mut self) -> Vec<u8> {
        let mut output = Vec::new();
        self.flush_pending(&mut output);
        if self.open_line.take().is_some() {
            output.push(b'\n');
        }
        output
    }

    fn flush_pending(&mut self, output: &mut Vec<u8>) {
        for byte in self.pending_utf8.drain(..) {
            output.extend_from_slice(format!("\\x{byte:02X}").as_bytes());
        }
    }

    fn encode_text(&mut self, bytes: &[u8], output: &mut Vec<u8>) {
        let mut combined = std::mem::take(&mut self.pending_utf8);
        combined.extend_from_slice(bytes);
        let mut remaining = combined.as_slice();
        while !remaining.is_empty() {
            let (valid_len, invalid_len) = match std::str::from_utf8(remaining) {
                Ok(_) => (remaining.len(), Some(0)),
                Err(error) => (error.valid_up_to(), error.error_len()),
            };
            let valid = std::str::from_utf8(&remaining[..valid_len]).unwrap();
            for ch in valid.chars() {
                match ch {
                    '\0' => output.extend_from_slice(b"\\0"),
                    ch if ch.is_control() && !matches!(ch, '\r' | '\n' | '\t') => {
                        for byte in ch.to_string().bytes() {
                            output.extend_from_slice(format!("\\x{byte:02X}").as_bytes());
                        }
                    }
                    ch => output.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes()),
                }
            }
            remaining = &remaining[valid_len..];
            match invalid_len {
                Some(count) => {
                    for byte in &remaining[..count] {
                        output.extend_from_slice(format!("\\x{byte:02X}").as_bytes());
                    }
                    remaining = &remaining[count..];
                }
                None => {
                    // At most three trailing bytes can be an incomplete UTF-8 scalar.
                    self.pending_utf8.extend_from_slice(remaining);
                    break;
                }
            }
        }
    }
}

// Closing a session waits for accepted records to drain before releasing the file.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LogDirection, SerialConfig, SessionState};

    #[test]
    fn session_start_scope_snapshots_paused_cache_once_and_continues_partial_rx() {
        use crate::model::settings::StreamOutputScope;
        for (label, scope, expected) in [
            (
                "history",
                StreamOutputScope::CurrentBufferAndFuture,
                "[1] RX old\n[2] RX partial-new\n[4] TX cmd\n",
            ),
            (
                "future",
                StreamOutputScope::FutureOnly,
                "[3] RX -new\n[4] TX cmd\n",
            ),
        ] {
            let path = std::env::temp_dir().join(format!(
                "sasha-scope-{label}-{}-{}.txt",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            ));
            let mut session = SessionState::new_real_session(SerialConfig::default(), true);
            session.append_rx_bytes(b"old\n", "1");
            session.toggle_receive_pause();
            session.append_rx_bytes(b"partial", "2");
            let output = StreamOutput::open(&path, false).unwrap();
            session.start_stream_output(output, scope);
            session.append_rx_bytes(b"-new\n", "3");
            session.record_tx_bytes(b"cmd", "4");
            assert!(session.stream_output.take().unwrap().finish().is_none());
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                format!("\u{FEFF}{expected}")
            );
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn session_history_larger_than_queue_keeps_order_and_excludes_cleared_data_and_other_sessions()
    {
        use crate::model::settings::StreamOutputScope;
        let path = std::env::temp_dir().join(format!(
            "sasha-history-{}-{}.txt",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let mut session = SessionState::new_real_session(SerialConfig::default(), true);
        let mut other = SessionState::new_real_session(SerialConfig::default(), true);
        session.append_rx_bytes(b"cleared\n", "0");
        session.clear_receive_logs();
        for index in 0..2100 {
            session.append_rx_bytes(format!("row-{index}\n").as_bytes(), "1");
        }
        other.append_rx_bytes(b"other-tab\n", "0");
        let output = StreamOutput::open(&path, false).unwrap();
        session.append_rx_bytes(b"during-file-preparation\n", "2");
        session.start_stream_output(output, StreamOutputScope::CurrentBufferAndFuture);
        assert!(session.stream_output.as_ref().unwrap().is_active());
        session.append_rx_bytes(b"live\n", "3");
        assert!(session.stream_output.take().unwrap().finish().is_none());
        let text = std::fs::read_to_string(&path).unwrap();
        let expected = format!(
            "\u{FEFF}{}[2] RX during-file-preparation\n[3] RX live\n",
            (0..2100)
                .map(|index| format!("[1] RX row-{index}\n"))
                .collect::<String>()
        );
        assert_eq!(text, expected);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn capture_appends_and_survives_pause_clear_and_trim() {
        let path = std::env::temp_dir().join(format!(
            "sasha-stream-{}-{}.txt",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::write(&path, b"existing\n").unwrap();
        let mut state = SessionState::new_real_session(SerialConfig::default(), true);
        state.stream_output = Some(StreamOutput::open(&path, true).unwrap());
        state.set_log_buffer_limit(1);
        state.append_rx_bytes(b"ab", "10:00:00.001");
        state.append_rx_bytes(b"c\0\r\n", "10:00:00.002");
        state.toggle_receive_pause();
        state.record_tx_bytes(&[0xff, 0], "10:00:00.003");
        state.clear_receive_logs();
        state.append_rx_bytes(b"z", "10:00:00.004");
        assert!(state.stream_output.take().unwrap().finish().is_none());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("existing\n"));
        assert!(text.contains("61 62"));
        assert!(text.contains("63 00 0D 0A"));
        assert!(text.contains("FF 00"));
        assert!(text.contains("7A"));
        assert_eq!(text.matches(" RX ").count(), 3);
        assert_eq!(text.matches(" TX ").count(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_directory_and_reports_disconnected_writer() {
        assert!(StreamOutput::open(&std::env::temp_dir(), false).is_err());
        // A closed worker is reported instead of silently dropping future records.
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        let (_, errors) = mpsc::channel();
        let mut output = StreamOutput {
            path: PathBuf::new(),
            sender: Some(sender),
            errors,
            worker: None,
            error: None,
        };
        output.record(&LogEntry::new(1, "now", LogDirection::Rx, b"x"));
        assert!(!output.is_active());
        assert!(output.error().is_some());
    }

    #[test]
    fn text_joins_rx_chunks_and_preserves_device_newlines() {
        let mut formatter = StreamFormatter {
            hex_mode: false,
            open_line: None,
            needs_separator: false,
            pending_utf8: Vec::new(),
        };
        let mut bytes = Vec::new();
        for (timestamp, chunk) in [
            ("1", &b"a"[..]),
            ("2", b"b"),
            ("3", b"c\r"),
            ("4", b"\nnext\n"),
        ] {
            bytes.extend(formatter.record(&LogEntry::new(1, timestamp, LogDirection::Rx, chunk)));
        }
        assert_eq!(bytes, b"[1] RX abc\r\n[4] RX next\n");
        assert!(formatter.finish().is_empty());
    }

    #[test]
    fn text_preserves_split_utf8_and_escapes_unsafe_controls() {
        let mut formatter = StreamFormatter {
            hex_mode: false,
            open_line: None,
            needs_separator: false,
            pending_utf8: Vec::new(),
        };
        let mut bytes =
            formatter.record(&LogEntry::new(1, "1", LogDirection::Rx, &[0xe4, 0xb8][..]));
        assert_eq!(bytes, b"[1] RX ");
        assert_eq!(formatter.pending_utf8.len(), 2);
        bytes.extend(formatter.record(&LogEntry::new(
            2,
            "2",
            LogDirection::Rx,
            &[0xad, b'\t', b'\\', 0, b'\n'][..],
        )));
        assert_eq!(bytes, "[1] RX 中\t\\\\0\n".as_bytes());
    }

    #[test]
    fn invalid_and_unfinished_utf8_is_printable_on_direction_change_and_finish() {
        let mut formatter = StreamFormatter {
            hex_mode: false,
            open_line: None,
            needs_separator: false,
            pending_utf8: Vec::new(),
        };
        let mut bytes = formatter.record(&LogEntry::new(
            1,
            "1",
            LogDirection::Rx,
            &[b'a', 0, 1, 0xff, 0xe4, 0xb8][..],
        ));
        assert_eq!(bytes, b"[1] RX a\\0\\x01\\xFF");
        bytes.extend(formatter.record(&LogEntry::new(2, "2", LogDirection::Tx, &[b'b', 0xe4][..])));
        bytes.extend(formatter.record(&LogEntry::new(
            3,
            "3",
            LogDirection::Rx,
            &[0xf0, 0x9f, 0x98][..],
        )));
        assert_eq!(formatter.pending_utf8.len(), 3);
        bytes.extend(formatter.finish());
        assert_eq!(
            bytes,
            b"[1] RX a\\0\\x01\\xFF\\xE4\\xB8\n[2] TX b\\xE4\n[3] RX \\xF0\\x9F\\x98\n"
        );
        assert!(std::str::from_utf8(&bytes).is_ok());
        assert!(!bytes.contains(&0));
    }

    #[test]
    fn arbitrary_binary_chunks_always_produce_valid_text() {
        let mut formatter = StreamFormatter {
            hex_mode: false,
            open_line: None,
            needs_separator: false,
            pending_utf8: Vec::new(),
        };
        let mut all = Vec::new();
        for byte in 0..=255 {
            let output = formatter.record(&LogEntry::new(1, "1", LogDirection::Rx, vec![byte]));
            assert!(std::str::from_utf8(&output).is_ok());
            assert!(formatter.pending_utf8.len() <= 3);
            all.extend(output);
        }
        all.extend(formatter.finish());
        assert!(
            std::str::from_utf8(&all)
                .unwrap()
                .chars()
                .all(|ch| !ch.is_control() || matches!(ch, '\r' | '\n' | '\t'))
        );
    }

    #[test]
    fn empty_file_gets_one_bom_and_append_keeps_it_at_the_start() {
        let path = std::env::temp_dir().join(format!(
            "sasha-bom-{}-{}.txt",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::write(&path, b"").unwrap();
        assert!(StreamOutput::open(&path, false).unwrap().finish().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), UTF8_BOM);
        for _ in 0..2 {
            let mut output = StreamOutput::open(&path, false).unwrap();
            output.record(&LogEntry::new(1, "1", LogDirection::Rx, b"ok\n"));
            assert!(output.finish().is_none());
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "\u{FEFF}[1] RX ok\n[1] RX ok\n"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_utf16_and_utf32_without_changing_existing_bytes() {
        let path = std::env::temp_dir().join(format!(
            "sasha-encoding-{}-{}.txt",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        for bom in [
            &b"\xFF\xFE"[..],
            &b"\xFE\xFF"[..],
            &b"\xFF\xFE\0\0"[..],
            &b"\0\0\xFE\xFF"[..],
        ] {
            std::fs::write(&path, bom).unwrap();
            let error = StreamOutput::open(&path, false).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains("UTF-16/UTF-32"));
            assert_eq!(std::fs::read(&path).unwrap(), bom);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn direction_and_simulated_changes_separate_partial_lines_and_finish() {
        let mut formatter = StreamFormatter {
            hex_mode: false,
            open_line: None,
            needs_separator: false,
            pending_utf8: Vec::new(),
        };
        let mut bytes = formatter.record(&LogEntry::new(1, "1", LogDirection::Rx, b"partial"));
        bytes.extend(formatter.record(&LogEntry::new(2, "2", LogDirection::Tx, b"cmd")));
        bytes.extend(formatter.record(&LogEntry::new(3, "3", LogDirection::Tx, b"next\r\n")));
        bytes.extend(formatter.record(&LogEntry::new(4, "4", LogDirection::Rx, b"real")));
        bytes.extend(formatter.record(&LogEntry::new_simulated(5, "5", LogDirection::Rx, b"demo")));
        bytes.extend(formatter.finish());
        assert_eq!(
            bytes,
            b"[1] RX partial\n[2] TX cmd\n[3] TX next\r\n[4] RX real\n[5] RX [SIM] demo\n"
        );
        assert!(formatter.finish().is_empty());
    }

    #[test]
    fn full_queue_stops_capture_with_visible_error() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        sender
            .try_send(StreamMessage::Entry(LogEntry::new(
                1,
                "pending",
                LogDirection::Rx,
                b"pending",
            )))
            .unwrap();
        let (_, errors) = mpsc::channel();
        let mut output = StreamOutput {
            path: PathBuf::new(),
            sender: Some(sender),
            errors,
            worker: None,
            error: None,
        };
        output.record(&LogEntry::new(1, "now", LogDirection::Rx, b"x"));
        assert!(!output.is_active());
        assert!(output.error().unwrap().contains("full"));
    }

    #[test]
    fn live_output_is_flushed_and_path_is_exclusive_until_stopped() {
        let path = std::env::temp_dir().join(format!(
            "sasha-live-{}-{}.txt",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let mut output = StreamOutput::open(&path, false).unwrap();
        assert!(StreamOutput::open(&path, true).is_err());
        output.record(&LogEntry::new_simulated(
            1,
            "now",
            LogDirection::Rx,
            &b"\0I: \xE5"[..],
        ));
        let partial_deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let partial = std::fs::read_to_string(&path).unwrap();
            if partial.ends_with("\\0I: ") {
                assert!(!partial.contains('\0'));
                break;
            }
            assert!(
                std::time::Instant::now() < partial_deadline,
                "partial output was not flushed"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        output.record(&LogEntry::new_simulated(
            2,
            "later",
            LogDirection::Rx,
            &b"\xAE\x9E\xE6\x97\xB6"[..],
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let content = std::fs::read_to_string(&path).unwrap();
            if content.contains("实时") {
                assert!(content.contains("[SIM]"));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "output did not become visible while running"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(output.finish().is_none());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "\u{FEFF}[now] RX [SIM] \\0I: 实时\n"
        );
        assert!(StreamOutput::open(&path, false).unwrap().finish().is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn text_capture_keeps_continuations_across_pause_clear_and_restart() {
        let path = std::env::temp_dir().join(format!(
            "sasha-text-{}-{}.txt",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::write(&path, b"existing partial").unwrap();
        let mut state = SessionState::new_real_session(SerialConfig::default(), true);
        state.stream_output = Some(StreamOutput::open(&path, false).unwrap());
        state.append_rx_bytes(b"a", "1");
        state.toggle_receive_pause();
        state.clear_receive_logs();
        state.set_log_buffer_limit(1);
        state.append_rx_bytes(b"b\r", "2");
        state.append_rx_bytes(b"\nnext", "3");
        assert!(state.stream_output.take().unwrap().finish().is_none());
        state.stream_output = Some(StreamOutput::open(&path, false).unwrap());
        state.append_rx_bytes(b"restart\n", "4");
        assert!(state.stream_output.take().unwrap().finish().is_none());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"existing partial\n[1] RX ab\r\n[3] RX next\n[4] RX restart\n"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn closing_outputs_drains_queues_and_keeps_sessions_separate() {
        let base = std::env::temp_dir().join(format!(
            "sasha-close-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let first_path = base.with_extension("first.txt");
        let second_path = base.with_extension("second.txt");
        let mut first = StreamOutput::open(&first_path, false).unwrap();
        let mut second = StreamOutput::open(&second_path, false).unwrap();
        for _ in 0..20 {
            first.record(&LogEntry::new(1, "now", LogDirection::Rx, b"first"));
            second.record(&LogEntry::new(1, "now", LogDirection::Tx, b"second"));
        }
        drop(first);
        drop(second);
        let first_text = std::fs::read_to_string(&first_path).unwrap();
        let second_text = std::fs::read_to_string(&second_path).unwrap();
        assert_eq!(first_text.matches("first").count(), 20);
        assert!(!first_text.contains("second"));
        assert_eq!(second_text.matches("second").count(), 20);
        assert!(!second_text.contains("first"));
        std::fs::remove_file(first_path).unwrap();
        std::fs::remove_file(second_path).unwrap();
    }
}
