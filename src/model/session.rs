use std::borrow::Cow;

use crate::i18n::{Locale, tr};

use super::log::format_hex_bytes;
use super::settings::DEFAULT_LOG_BUFFER_LIMIT;
use super::{
    ActivePickerKind, LogDirection, LogEntry, ModemSignals, SendPayloadError, SerialConfig,
    format_visible_logs, parse_hex_bytes,
};

/// Simulated connection state. Never claims real hardware connectivity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulatedConnectionState {
    Disconnected,
    Connected,
}

impl SimulatedConnectionState {
    pub fn is_connected(self) -> bool {
        matches!(self, Self::Connected)
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Disconnected => Self::Connected,
            Self::Connected => Self::Disconnected,
        }
    }

    #[allow(dead_code)]
    pub fn status_label(self) -> Cow<'static, str> {
        self.status_label_for(Locale::ZhCn)
    }

    pub fn status_label_for(self, locale: Locale) -> Cow<'static, str> {
        match self {
            Self::Disconnected => tr(locale, "status.sim_disconnected"),
            Self::Connected => tr(locale, "status.sim_connected"),
        }
    }

    #[allow(dead_code)]
    pub fn action_label(self) -> Cow<'static, str> {
        self.action_label_for(Locale::ZhCn)
    }

    pub fn action_label_for(self, locale: Locale) -> Cow<'static, str> {
        match self {
            Self::Disconnected => tr(locale, "status.action_sim_connect"),
            Self::Connected => tr(locale, "status.action_sim_disconnect"),
        }
    }
}

fn trim_log_buffer(logs: &mut Vec<LogEntry>, limit: usize) {
    let mut total: usize = logs.iter().map(|entry| entry.bytes.len()).sum();
    let mut drop_count = 0;
    while drop_count + 1 < logs.len() && total > limit {
        total = total.saturating_sub(logs[drop_count].bytes.len());
        drop_count += 1;
    }
    if drop_count > 0 {
        logs.drain(..drop_count);
    }
}

/// Receive view display flags (independent from send card HEX flag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiveViewState {
    pub hex_mode: bool,
    pub show_timestamp: bool,
    pub paused: bool,
    pub auto_scroll: bool,
}

impl Default for ReceiveViewState {
    fn default() -> Self {
        Self {
            hex_mode: false,
            show_timestamp: true,
            paused: false,
            auto_scroll: true,
        }
    }
}

/// Line ending appended to outgoing text payloads on send (ignored in HEX send mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SendLineEnding {
    #[default]
    None,
    CrLf,
    Lf,
}

impl SendLineEnding {
    pub fn cycle(self) -> Self {
        match self {
            Self::None => Self::CrLf,
            Self::CrLf => Self::Lf,
            Self::Lf => Self::None,
        }
    }

    pub fn suffix_bytes(self) -> &'static [u8] {
        match self {
            Self::None => b"",
            Self::CrLf => b"\r\n",
            Self::Lf => b"\n",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "\\r\\n",
            Self::CrLf => "\\r\\n",
            Self::Lf => "\\n",
        }
    }

    pub fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Floating send card state (independent from receive HEX flag).
///
/// `display_hex` controls only how `text` is displayed/converted in the input box,
/// whereas `send_hex` controls only whether `text` is parsed as HEX bytes on send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendDraftState {
    pub text: String,
    pub display_hex: bool,
    pub send_hex: bool,
    pub line_ending: SendLineEnding,
}

impl Default for SendDraftState {
    fn default() -> Self {
        Self {
            text: String::new(),
            display_hex: false,
            send_hex: false,
            line_ending: SendLineEnding::None,
        }
    }
}

/// Simulated byte counters for RX and TX.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionCounters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Shared single-session UI state boundary for TASK-001 through TASK-004.
#[derive(Debug)]
pub struct SessionState {
    pub stream_output: Option<super::stream_output::StreamOutput>,
    pub is_simulated_session: bool,
    pub simulated_notice: &'static str,
    pub config: SerialConfig,
    /// Latest sampled modem input lines (real sessions only).
    pub modem_signals: ModemSignals,
    pub connection: SimulatedConnectionState,
    pub receive: ReceiveViewState,
    pub send: SendDraftState,
    pub counters: SessionCounters,
    pub logs: Vec<LogEntry>,
    pub retained_while_paused: Vec<LogEntry>,
    /// Maximum receive-log payload kept, in bytes. Oldest rows are dropped past this cap.
    pub log_buffer_limit: usize,
    #[allow(dead_code)]
    pub active_picker: Option<ActivePickerKind>,
    next_log_id: u64,
    next_sim_rx_seq: u64,
    rx_line_open: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new_simulated_demo()
    }
}

impl SessionState {
    /// Snapshot and attachment happen together on the UI thread after file preparation.
    pub fn start_stream_output(
        &mut self,
        mut output: super::stream_output::StreamOutput,
        scope: super::settings::StreamOutputScope,
    ) {
        if scope == super::settings::StreamOutputScope::CurrentBufferAndFuture {
            let mut history = self.logs.clone();
            history.extend(self.retained_while_paused.iter().cloned());
            history.sort_by_key(|entry| entry.id);
            history.dedup_by_key(|entry| entry.id);
            output.record_history(history);
        }
        self.stream_output = Some(output);
    }
    /// Creates a clearly-labelled simulated demo session for UI skeleton verification.
    pub fn new_simulated_demo() -> Self {
        let tx_payload = b"AT+VERSION".to_vec();
        let rx_payload_1 = concat!("+VERSION: Sasha v", env!("CARGO_PKG_VERSION"))
            .as_bytes()
            .to_vec();
        let rx_payload_2 = b"OK".to_vec();

        let tx_bytes = tx_payload.len() as u64;
        let rx_bytes = (rx_payload_1.len() + rx_payload_2.len()) as u64;

        Self {
            is_simulated_session: true,
            stream_output: None,
            simulated_notice: "模拟会话（无真实串口硬件）",
            config: SerialConfig::default(),
            modem_signals: ModemSignals::default(),
            connection: SimulatedConnectionState::Disconnected,
            receive: ReceiveViewState::default(),
            send: SendDraftState {
                text: "AT+VERSION".to_string(),
                display_hex: false,
                send_hex: false,
                line_ending: SendLineEnding::None,
            },
            counters: SessionCounters { rx_bytes, tx_bytes },
            logs: vec![
                LogEntry::new_simulated(1, "14:26:07.285", LogDirection::Tx, tx_payload),
                LogEntry::new_simulated(2, "14:26:07.342", LogDirection::Rx, rx_payload_1),
                LogEntry::new_simulated(3, "14:26:07.343", LogDirection::Rx, rx_payload_2),
            ],
            retained_while_paused: Vec::new(),
            log_buffer_limit: DEFAULT_LOG_BUFFER_LIMIT as usize,
            active_picker: None,
            next_log_id: 4,
            next_sim_rx_seq: 0,
            rx_line_open: false,
        }
    }

    /// Creates a real hardware serial session with empty logs and zeroed counters.
    #[allow(dead_code)]
    pub fn new_real_session(config: SerialConfig, connected: bool) -> Self {
        Self {
            is_simulated_session: false,
            stream_output: None,
            simulated_notice: "",
            config,
            modem_signals: ModemSignals::default(),
            connection: if connected {
                SimulatedConnectionState::Connected
            } else {
                SimulatedConnectionState::Disconnected
            },
            receive: ReceiveViewState::default(),
            send: SendDraftState::default(),
            counters: SessionCounters::default(),
            logs: Vec::new(),
            retained_while_paused: Vec::new(),
            log_buffer_limit: DEFAULT_LOG_BUFFER_LIMIT as usize,
            active_picker: None,
            next_log_id: 1,
            next_sim_rx_seq: 0,
            rx_line_open: false,
        }
    }

    /// Returns the localized simulated session notice (empty for real sessions).
    pub fn simulated_notice_for(&self, locale: Locale) -> Cow<'static, str> {
        if self.is_simulated_session {
            tr(locale, "status.sim_notice")
        } else {
            Cow::Borrowed("")
        }
    }

    /// Returns the mode-aware connection status label (real vs. simulated).
    #[allow(dead_code)]
    pub fn connection_status_label(&self) -> Cow<'static, str> {
        self.connection_status_label_for(Locale::ZhCn)
    }

    pub fn connection_status_label_for(&self, locale: Locale) -> Cow<'static, str> {
        if self.is_simulated_session {
            self.connection.status_label_for(locale)
        } else if self.connection.is_connected() {
            tr(locale, "status.connected")
        } else {
            tr(locale, "status.disconnected")
        }
    }

    /// Returns the mode-aware connection toggle button label (real vs. simulated).
    #[allow(dead_code)]
    pub fn connection_action_label(&self) -> Cow<'static, str> {
        self.connection_action_label_for(Locale::ZhCn)
    }

    pub fn connection_action_label_for(&self, locale: Locale) -> Cow<'static, str> {
        if self.is_simulated_session {
            self.connection.action_label_for(locale)
        } else if self.connection.is_connected() {
            tr(locale, "status.action_disconnect")
        } else {
            tr(locale, "status.action_connect")
        }
    }

    /// Explicitly sets the connection state.
    #[allow(dead_code)]
    pub fn set_connected(&mut self, connected: bool) {
        self.connection = if connected {
            SimulatedConnectionState::Connected
        } else {
            self.rx_line_open = false;
            self.modem_signals = ModemSignals::default();
            SimulatedConnectionState::Disconnected
        };
    }

    /// Toggles the simulated connection state without claiming real hardware connectivity.
    pub fn toggle_simulated_connection(&mut self) {
        self.connection = self.connection.toggled();
    }

    /// Toggles receive HEX display independently from send card HEX mode.
    pub fn toggle_receive_hex(&mut self) {
        self.receive.hex_mode = !self.receive.hex_mode;
    }

    /// Toggles timestamp visibility in the receive log view.
    pub fn toggle_receive_timestamp(&mut self) {
        self.receive.show_timestamp = !self.receive.show_timestamp;
    }

    /// Toggles receive pause. While paused, simulated RX and TX log events are retained
    /// in `retained_while_paused` in arrival order and flushed into `logs` upon resume.
    pub fn toggle_receive_pause(&mut self) {
        self.receive.paused = !self.receive.paused;
        if !self.receive.paused && !self.retained_while_paused.is_empty() {
            self.logs.append(&mut self.retained_while_paused);
            self.trim_visible_logs();
        }
    }

    /// Toggles automatic scrolling to the newest log entry.
    pub fn toggle_auto_scroll(&mut self) {
        self.receive.auto_scroll = !self.receive.auto_scroll;
    }

    /// Records a log entry, queuing it while receive display is paused so
    /// visible logs stay frozen and RX/TX events preserve arrival order on resume.
    fn record_log(&mut self, entry: LogEntry) {
        if (entry.direction != LogDirection::Rx || self.is_simulated_session)
            && let Some(output) = &mut self.stream_output
        {
            output.record(&entry);
        }
        if self.receive.paused {
            self.retained_while_paused.push(entry);
        } else {
            self.logs.push(entry);
            self.trim_visible_logs();
        }
    }

    /// Payload bytes currently kept in the visible receive log.
    pub fn log_buffer_used_bytes(&self) -> usize {
        self.logs.iter().map(|entry| entry.bytes.len()).sum()
    }

    /// Sets the receive-log payload cap in bytes and drops the oldest rows past it.
    pub fn set_log_buffer_limit(&mut self, limit: usize) {
        self.log_buffer_limit = limit.max(1);
        self.trim_visible_logs();
        trim_log_buffer(&mut self.retained_while_paused, self.log_buffer_limit);
    }

    fn trim_visible_logs(&mut self) {
        trim_log_buffer(&mut self.logs, self.log_buffer_limit);
    }

    fn build_log_entry(
        &mut self,
        timestamp: impl Into<String>,
        direction: LogDirection,
        bytes: Vec<u8>,
    ) -> LogEntry {
        let id = self.next_log_id;
        self.next_log_id += 1;
        if self.is_simulated_session {
            LogEntry::new_simulated(id, timestamp, direction, bytes)
        } else {
            LogEntry::new(id, timestamp, direction, bytes)
        }
    }

    /// Clears visible logs without resetting RX/TX byte counters.
    pub fn clear_receive_logs(&mut self) {
        self.logs.clear();
        self.rx_line_open = false;
    }

    /// Returns the exact text currently visible in `LogView` for Save As export.
    pub fn visible_log_export_text(&self) -> String {
        format_visible_logs(
            &self.logs,
            self.receive.show_timestamp,
            self.receive.hex_mode,
        )
    }

    /// Appends received bytes (used by real serial backend) with streaming `\n` line framing,
    /// updating RX counters, respecting `receive.paused`, and omitting the simulated badge in real mode.
    #[allow(dead_code)]
    pub fn append_rx_bytes(&mut self, payload: &[u8], timestamp: &str) {
        if payload.is_empty() {
            return;
        }
        self.counters.rx_bytes += payload.len() as u64;
        if let Some(output) = &mut self.stream_output {
            output.record(&LogEntry::new(0, timestamp, LogDirection::Rx, payload));
        }

        for chunk in payload.split_inclusive(|&b| b == b'\n') {
            let appended = if self.rx_line_open {
                let target = if self.receive.paused {
                    &mut self.retained_while_paused
                } else {
                    &mut self.logs
                };
                if let Some(last) = target.last_mut()
                    && last.direction == LogDirection::Rx
                    && !last.bytes.ends_with(b"\n")
                {
                    last.bytes.extend_from_slice(chunk);
                    if chunk.ends_with(b"\n") {
                        self.rx_line_open = false;
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            };

            if !appended {
                let entry = self.build_log_entry(timestamp, LogDirection::Rx, chunk.to_vec());
                self.record_log(entry);
                self.rx_line_open = !chunk.ends_with(b"\n");
            }
        }
    }

    /// Injects a clearly-labelled simulated RX event.
    /// When receive display is paused, the event is retained until resume.
    pub fn inject_simulated_rx(&mut self, timestamp: impl Into<String>, bytes: Vec<u8>) {
        let id = self.next_log_id;
        self.next_log_id += 1;
        self.counters.rx_bytes += bytes.len() as u64;

        let entry = LogEntry::new_simulated(id, timestamp, LogDirection::Rx, bytes);
        self.record_log(entry);
    }

    /// Deterministically injects the next simulated RX payload for reproducible verification.
    pub fn inject_next_deterministic_simulated_rx(&mut self, timestamp: impl Into<String>) -> u64 {
        self.next_sim_rx_seq += 1;
        let seq = self.next_sim_rx_seq;
        let payload = match seq {
            1 => b"SIM_RX_01: OK".to_vec(),
            2 => "SIM_RX_02: 数据".as_bytes().to_vec(),
            _ => format!("SIM_RX_{seq:02}: OK").into_bytes(),
        };
        self.inject_simulated_rx(timestamp, payload);
        seq
    }

    /// Toggles send input box HEX display mode independently from `send.send_hex` and
    /// `receive.hex_mode`, converting `self.send.text` between UTF-8 text and
    /// space-separated uppercase HEX bytes.
    ///
    /// When switching from HEX display to text, blocks the transition and preserves both
    /// `display_hex == true` and the original `text` if the HEX input is malformed or
    /// cannot be decoded as valid UTF-8.
    #[allow(dead_code)]
    pub fn toggle_send_display_hex(&mut self) -> Result<(), Cow<'static, str>> {
        self.toggle_send_display_hex_for(Locale::ZhCn)
    }

    pub fn toggle_send_display_hex_for(&mut self, locale: Locale) -> Result<(), Cow<'static, str>> {
        if self.send.display_hex {
            if self.send.text.trim().is_empty() {
                self.send.text.clear();
                self.send.display_hex = false;
                return Ok(());
            }

            let bytes = match parse_hex_bytes(&self.send.text) {
                Ok(bytes) => bytes,
                Err(SendPayloadError::InvalidHex(_)) => return Err(tr(locale, "err.invalid_hex")),
                Err(SendPayloadError::EmptyInput) => {
                    self.send.text.clear();
                    self.send.display_hex = false;
                    return Ok(());
                }
                Err(SendPayloadError::Disconnected) => {
                    return Err(tr(locale, "err.hex_toggle_failed"));
                }
            };

            let text = String::from_utf8(bytes).map_err(|_| tr(locale, "err.hex_non_utf8"))?;
            self.send.text = text;
            self.send.display_hex = false;
            Ok(())
        } else {
            self.send.text = format_hex_bytes(self.send.text.as_bytes());
            self.send.display_hex = true;
            Ok(())
        }
    }

    /// Toggles whether outgoing payloads are parsed as HEX (`self.send.send_hex`)
    /// without modifying `self.send.text` or `self.send.display_hex`.
    pub fn toggle_send_format_hex(&mut self) {
        self.send.send_hex = !self.send.send_hex;
    }

    /// Cycles the outgoing text line ending: `None -> CrLf -> Lf -> None`.
    pub fn cycle_send_line_ending(&mut self) {
        self.send.line_ending = self.send.line_ending.cycle();
    }

    /// Validates connection, non-empty draft, and HEX syntax (when `send.send_hex` is
    /// enabled) without mutating state.
    pub fn prepare_send_bytes(&self) -> Result<Vec<u8>, SendPayloadError> {
        if !self.connection.is_connected() {
            return Err(SendPayloadError::Disconnected);
        }

        if self.send.send_hex {
            parse_hex_bytes(&self.send.text)
        } else if self.send.text.is_empty() {
            Err(SendPayloadError::EmptyInput)
        } else {
            let mut bytes = self.send.text.as_bytes().to_vec();
            bytes.extend_from_slice(self.send.line_ending.suffix_bytes());
            Ok(bytes)
        }
    }

    /// Records transmitted bytes after a successful serial write, updating TX counters
    /// and respecting `receive.paused`.
    pub fn record_tx_bytes(&mut self, payload: &[u8], timestamp: &str) {
        self.rx_line_open = false;
        self.counters.tx_bytes += payload.len() as u64;
        let entry = self.build_log_entry(timestamp, LogDirection::Tx, payload.to_vec());
        self.record_log(entry);
    }

    /// Validates and submits the current send draft as a TX log entry.
    /// Preserves `self.send.text` after submission and accumulates UTF-8/HEX byte counts.
    pub fn submit_send_with_timestamp(
        &mut self,
        timestamp: impl Into<String>,
    ) -> Result<Vec<u8>, SendPayloadError> {
        let bytes = self.prepare_send_bytes()?;
        let ts = timestamp.into();
        self.record_tx_bytes(&bytes, &ts);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_demo_is_clearly_labelled() {
        let state = SessionState::new_simulated_demo();
        assert!(state.is_simulated_session);
        assert!(state.simulated_notice.contains("模拟"));
        assert!(!state.logs.is_empty());
        assert!(state.logs.iter().all(|entry| entry.is_simulated));
        assert!(
            SimulatedConnectionState::Disconnected
                .status_label()
                .contains("模拟")
        );
        assert!(
            SimulatedConnectionState::Connected
                .status_label()
                .contains("模拟")
        );
        assert!(
            SimulatedConnectionState::Disconnected
                .action_label()
                .contains("模拟")
        );
        assert!(
            SimulatedConnectionState::Connected
                .action_label()
                .contains("模拟")
        );
    }

    #[test]
    fn receive_and_send_hex_states_are_independent() {
        let mut state = SessionState::new_simulated_demo();
        assert!(!state.receive.hex_mode);
        assert!(!state.send.display_hex);
        assert!(!state.send.send_hex);

        state.toggle_receive_hex();
        assert!(state.receive.hex_mode);
        assert!(!state.send.display_hex);
        assert!(!state.send.send_hex);

        state.send.text = "AT".to_string();
        state.toggle_send_display_hex().unwrap();
        assert!(state.receive.hex_mode);
        assert!(state.send.display_hex);
        assert!(!state.send.send_hex);
        assert_eq!(state.send.text, "41 54");

        state.toggle_send_format_hex();
        assert!(state.receive.hex_mode);
        assert!(state.send.display_hex);
        assert!(state.send.send_hex);
        assert_eq!(state.send.text, "41 54");

        state.toggle_receive_hex();
        state.toggle_send_display_hex().unwrap();
        assert!(!state.receive.hex_mode);
        assert!(!state.send.display_hex);
        assert!(state.send.send_hex);
        assert_eq!(state.send.text, "AT");
    }

    #[test]
    fn pause_retains_simulated_events_and_resume_displays_them() {
        let mut state = SessionState::new_simulated_demo();
        let initial_len = state.logs.len();
        let initial_rx = state.counters.rx_bytes;

        state.toggle_receive_pause();
        assert!(state.receive.paused);

        state.inject_simulated_rx("14:26:08.001", b"EVT1".to_vec());
        state.inject_simulated_rx("14:26:08.002", b"EVT2".to_vec());

        // Display stays frozen while paused, but events are retained and RX bytes counted.
        assert_eq!(state.logs.len(), initial_len);
        assert_eq!(state.retained_while_paused.len(), 2);
        assert_eq!(state.counters.rx_bytes, initial_rx + 8);

        state.toggle_receive_pause();
        assert!(!state.receive.paused);
        assert!(state.retained_while_paused.is_empty());
        assert_eq!(state.logs.len(), initial_len + 2);
        assert_eq!(state.logs[initial_len].bytes, b"EVT1");
        assert_eq!(state.logs[initial_len + 1].bytes, b"EVT2");
    }

    #[test]
    fn pause_freezes_display_for_mixed_rx_tx_and_preserves_arrival_order_on_resume() {
        let mut state = SessionState::new_simulated_demo();
        let initial_len = state.logs.len();
        let initial_rx = state.counters.rx_bytes;
        let initial_tx = state.counters.tx_bytes;
        let frozen_export = state.visible_log_export_text();

        state.toggle_receive_pause();
        assert!(state.receive.paused);

        // 1. RX arrives while paused
        let rx1_seq = state.inject_next_deterministic_simulated_rx("14:26:08.100");
        assert_eq!(rx1_seq, 1);

        // 2. Connect and send UTF-8 text while paused
        state.toggle_simulated_connection();
        state.send.text = "AT+VERSION".to_string();
        state.send.send_hex = false;
        let tx1 = state
            .submit_send_with_timestamp("14:26:08.200")
            .expect("TX text should submit while paused");
        assert_eq!(tx1, b"AT+VERSION");

        // 3. Second RX arrives while paused (UTF-8 multi-byte payload)
        let rx2_seq = state.inject_next_deterministic_simulated_rx("14:26:08.300");
        assert_eq!(rx2_seq, 2);
        let rx2_expected = "SIM_RX_02: 数据".as_bytes().to_vec();

        // 4. Second TX in HEX mode while paused
        state.toggle_send_format_hex();
        state.send.text = "0A FF".to_string();
        let tx2 = state
            .submit_send_with_timestamp("14:26:08.400")
            .expect("TX HEX should submit while paused");
        assert_eq!(tx2, vec![0x0A, 0xFF]);

        // Visible display remains completely frozen while counters accumulate.
        assert_eq!(state.logs.len(), initial_len);
        assert_eq!(state.visible_log_export_text(), frozen_export);
        assert_eq!(state.retained_while_paused.len(), 4);
        assert_eq!(
            state.counters.rx_bytes,
            initial_rx + b"SIM_RX_01: OK".len() as u64 + rx2_expected.len() as u64
        );
        assert_eq!(
            state.counters.tx_bytes,
            initial_tx + b"AT+VERSION".len() as u64 + 2
        );

        // Resume flushes retained RX/TX events in exact arrival order.
        state.toggle_receive_pause();
        assert!(!state.receive.paused);
        assert!(state.retained_while_paused.is_empty());
        assert_eq!(state.logs.len(), initial_len + 4);

        let resumed = &state.logs[initial_len..];
        assert_eq!(resumed[0].direction, LogDirection::Rx);
        assert_eq!(resumed[0].timestamp, "14:26:08.100");
        assert_eq!(resumed[0].bytes, b"SIM_RX_01: OK");

        assert_eq!(resumed[1].direction, LogDirection::Tx);
        assert_eq!(resumed[1].timestamp, "14:26:08.200");
        assert_eq!(resumed[1].bytes, b"AT+VERSION");

        assert_eq!(resumed[2].direction, LogDirection::Rx);
        assert_eq!(resumed[2].timestamp, "14:26:08.300");
        assert_eq!(resumed[2].bytes, rx2_expected);

        assert_eq!(resumed[3].direction, LogDirection::Tx);
        assert_eq!(resumed[3].timestamp, "14:26:08.400");
        assert_eq!(resumed[3].bytes, vec![0x0A, 0xFF]);
    }

    #[test]
    fn clear_empties_visible_logs_without_resetting_counters() {
        let mut state = SessionState::new_simulated_demo();
        let rx_before = state.counters.rx_bytes;
        let tx_before = state.counters.tx_bytes;
        assert!(rx_before > 0);
        assert!(tx_before > 0);

        state.clear_receive_logs();
        assert!(state.logs.is_empty());
        assert_eq!(state.counters.rx_bytes, rx_before);
        assert_eq!(state.counters.tx_bytes, tx_before);
        assert_eq!(state.visible_log_export_text(), "");
    }

    #[test]
    fn save_export_matches_current_visible_display_including_hex_and_timestamp() {
        let mut state = SessionState::new_simulated_demo();
        state.clear_receive_logs();
        state.inject_simulated_rx("14:26:09.100", b"AB".to_vec());

        assert_eq!(state.visible_log_export_text(), "[14:26:09.100] RX AB");

        state.toggle_receive_hex();
        assert_eq!(state.visible_log_export_text(), "[14:26:09.100] RX 41 42");

        state.toggle_receive_timestamp();
        assert_eq!(state.visible_log_export_text(), "RX 41 42");
    }

    #[test]
    fn send_rejects_disconnected_empty_and_invalid_hex_without_submitting() {
        let mut state = SessionState::new_simulated_demo();
        let initial_logs = state.logs.len();
        let initial_tx = state.counters.tx_bytes;

        // Disconnected: no submission.
        assert!(!state.connection.is_connected());
        assert_eq!(
            state.submit_send_with_timestamp("14:26:10.000"),
            Err(SendPayloadError::Disconnected)
        );
        assert_eq!(state.logs.len(), initial_logs);
        assert_eq!(state.counters.tx_bytes, initial_tx);

        // Connect, then test empty input in text mode.
        state.toggle_simulated_connection();
        assert!(state.connection.is_connected());
        state.send.text.clear();
        assert_eq!(
            state.submit_send_with_timestamp("14:26:10.001"),
            Err(SendPayloadError::EmptyInput)
        );
        assert_eq!(state.logs.len(), initial_logs);
        assert_eq!(state.counters.tx_bytes, initial_tx);

        // Invalid HEX: error, no submission, no counter change.
        state.toggle_send_format_hex();
        state.send.text = "4142".to_string();
        assert!(matches!(
            state.submit_send_with_timestamp("14:26:10.002"),
            Err(SendPayloadError::InvalidHex(_))
        ));
        assert_eq!(state.logs.len(), initial_logs);
        assert_eq!(state.counters.tx_bytes, initial_tx);
    }

    #[test]
    fn send_utf8_and_valid_hex_submit_exact_bytes_preserve_input_and_accumulate_counts() {
        let mut state = SessionState::new_simulated_demo();
        state.toggle_simulated_connection();
        let initial_tx = state.counters.tx_bytes;

        // UTF-8 text without newline appended.
        state.send.send_hex = false;
        state.send.text = "你好".to_string();
        let submitted_utf8 = state
            .submit_send_with_timestamp("14:26:11.000")
            .expect("UTF-8 text should submit");
        assert_eq!(submitted_utf8, "你好".as_bytes());
        assert_eq!(submitted_utf8.len(), 6);
        assert_eq!(state.send.text, "你好");
        assert_eq!(state.counters.tx_bytes, initial_tx + 6);
        assert_eq!(
            state.logs.last().unwrap().bytes,
            vec![0xE4, 0xBD, 0xA0, 0xE5, 0xA5, 0xBD]
        );

        // Valid space-separated HEX bytes.
        state.toggle_send_format_hex();
        state.send.text = "0A FF 1b".to_string();
        let submitted_hex = state
            .submit_send_with_timestamp("14:26:11.100")
            .expect("Valid HEX should submit");
        assert_eq!(submitted_hex, vec![0x0A, 0xFF, 0x1B]);
        assert_eq!(state.send.text, "0A FF 1b");
        assert_eq!(state.counters.tx_bytes, initial_tx + 6 + 3);
        assert_eq!(state.logs.last().unwrap().bytes, vec![0x0A, 0xFF, 0x1B]);
    }

    #[test]
    fn toggle_send_display_hex_converts_ascii_utf8_and_empty_bidirectionally() {
        let mut state = SessionState::new_simulated_demo();

        // ASCII: text -> hex -> text
        state.send.display_hex = false;
        state.send.text = "AT+VERSION".to_string();
        assert_eq!(state.toggle_send_display_hex(), Ok(()));
        assert!(state.send.display_hex);
        assert!(!state.send.send_hex);
        assert_eq!(state.send.text, "41 54 2B 56 45 52 53 49 4F 4E");

        assert_eq!(state.toggle_send_display_hex(), Ok(()));
        assert!(!state.send.display_hex);
        assert!(!state.send.send_hex);
        assert_eq!(state.send.text, "AT+VERSION");

        // Chinese UTF-8: text -> hex -> text
        state.send.text = "数据".to_string();
        assert_eq!(state.toggle_send_display_hex(), Ok(()));
        assert!(state.send.display_hex);
        assert_eq!(state.send.text, "E6 95 B0 E6 8D AE");

        assert_eq!(state.toggle_send_display_hex(), Ok(()));
        assert!(!state.send.display_hex);
        assert_eq!(state.send.text, "数据");

        // Lowercase / extra spaces in HEX -> text
        state.send.display_hex = true;
        state.send.text = "  e6  95 b0 ".to_string();
        assert_eq!(state.toggle_send_display_hex(), Ok(()));
        assert!(!state.send.display_hex);
        assert_eq!(state.send.text, "数");

        // Empty string: text -> hex -> text
        state.send.text.clear();
        assert_eq!(state.toggle_send_display_hex(), Ok(()));
        assert!(state.send.display_hex);
        assert_eq!(state.send.text, "");

        assert_eq!(state.toggle_send_display_hex(), Ok(()));
        assert!(!state.send.display_hex);
        assert_eq!(state.send.text, "");

        // Whitespace-only in HEX -> clears to "" and switches to text
        state.send.display_hex = true;
        state.send.text = "   ".to_string();
        assert_eq!(state.toggle_send_display_hex(), Ok(()));
        assert!(!state.send.display_hex);
        assert_eq!(state.send.text, "");
    }

    #[test]
    fn toggle_send_display_hex_blocks_invalid_hex_and_non_utf8_bytes() {
        let mut state = SessionState::new_simulated_demo();
        state.send.display_hex = true;
        state.send.send_hex = true;

        // Odd-length token
        state.send.text = "41 5".to_string();
        let err = state.toggle_send_display_hex().unwrap_err();
        assert!(err.contains("HEX"));
        assert!(state.send.display_hex);
        assert!(state.send.send_hex);
        assert_eq!(state.send.text, "41 5");

        // Unseparated pair
        state.send.text = "4142".to_string();
        let err = state.toggle_send_display_hex().unwrap_err();
        assert!(err.contains("HEX"));
        assert!(state.send.display_hex);
        assert_eq!(state.send.text, "4142");

        // Non-UTF-8 bytes
        state.send.text = "FF 00".to_string();
        let err = state.toggle_send_display_hex().unwrap_err();
        assert!(err.contains("UTF-8"));
        assert!(state.send.display_hex);
        assert_eq!(state.send.text, "FF 00");
    }

    #[test]
    fn real_session_creation_and_labels_reflect_real_mode() {
        let mut cfg = SerialConfig::default();
        cfg.port = "COM5".to_string();
        cfg.baud_rate = 230400;

        let mut real = SessionState::new_real_session(cfg.clone(), false);
        assert!(!real.is_simulated_session);
        assert_eq!(real.simulated_notice, "");
        assert_eq!(real.config, cfg);
        assert!(real.logs.is_empty());
        assert_eq!(real.counters, SessionCounters::default());
        assert!(!real.connection.is_connected());
        assert_eq!(real.connection_status_label(), "未连接");
        assert_eq!(real.connection_action_label(), "连接");

        real.set_connected(true);
        assert!(real.connection.is_connected());
        assert_eq!(real.connection_status_label(), "已连接");
        assert_eq!(real.connection_action_label(), "断开");

        let mut sim = SessionState::new_simulated_demo();
        assert_eq!(sim.connection_status_label(), "模拟未连接");
        assert_eq!(sim.connection_action_label(), "模拟连接");
        sim.set_connected(true);
        assert_eq!(sim.connection_status_label(), "模拟已连接");
        assert_eq!(sim.connection_action_label(), "模拟断开");
    }

    #[test]
    fn real_session_prepare_record_tx_and_append_rx_with_pause_retention() {
        let mut state = SessionState::new_real_session(SerialConfig::default(), false);
        state.send.text = "AT".to_string();

        // Disconnected: prepare_send_bytes fails without mutating state.
        assert_eq!(
            state.prepare_send_bytes(),
            Err(SendPayloadError::Disconnected)
        );
        assert!(state.logs.is_empty());
        assert_eq!(state.counters.tx_bytes, 0);

        state.set_connected(true);

        // Empty text fails.
        state.send.text.clear();
        assert_eq!(
            state.prepare_send_bytes(),
            Err(SendPayloadError::EmptyInput)
        );

        // Invalid HEX fails without mutating state.
        state.send.send_hex = true;
        state.send.text = "4142".to_string();
        assert!(matches!(
            state.prepare_send_bytes(),
            Err(SendPayloadError::InvalidHex(_))
        ));
        assert!(state.logs.is_empty());

        // Valid HEX prepares bytes without mutating counters or logs.
        state.send.text = "41 54".to_string();
        let prepared = state
            .prepare_send_bytes()
            .expect("valid HEX should prepare");
        assert_eq!(prepared, b"AT");
        assert!(state.logs.is_empty());
        assert_eq!(state.counters.tx_bytes, 0);

        // Record TX and RX in normal (unpaused) mode -> non-simulated LogEntry.
        state.record_tx_bytes(&prepared, "15:00:00.100");
        state.append_rx_bytes(b"OK\r\n", "15:00:00.150");

        assert_eq!(state.counters.tx_bytes, 2);
        assert_eq!(state.counters.rx_bytes, 4);
        assert_eq!(state.logs.len(), 2);
        assert!(!state.logs[0].is_simulated);
        assert_eq!(state.logs[0].direction, LogDirection::Tx);
        assert_eq!(state.logs[0].bytes, b"AT");
        assert!(!state.logs[1].is_simulated);
        assert_eq!(state.logs[1].direction, LogDirection::Rx);
        assert_eq!(state.logs[1].bytes, b"OK\r\n");

        // Pause retains both RX and TX in arrival order and flushes on resume.
        state.toggle_receive_pause();
        state.append_rx_bytes(b"R1", "15:00:01.000");
        state.record_tx_bytes(b"T1", "15:00:01.100");
        assert_eq!(state.logs.len(), 2);
        assert_eq!(state.retained_while_paused.len(), 2);
        assert_eq!(state.counters.rx_bytes, 6);
        assert_eq!(state.counters.tx_bytes, 4);

        state.toggle_receive_pause();
        assert!(state.retained_while_paused.is_empty());
        assert_eq!(state.logs.len(), 4);
        assert_eq!(state.logs[2].direction, LogDirection::Rx);
        assert_eq!(state.logs[2].bytes, b"R1");
        assert!(!state.logs[2].is_simulated);
        assert_eq!(state.logs[3].direction, LogDirection::Tx);
        assert_eq!(state.logs[3].bytes, b"T1");
        assert!(!state.logs[3].is_simulated);
    }

    #[test]
    fn streaming_rx_line_framing_joins_fragments_and_splits_newlines() {
        let mut state = SessionState::new_real_session(SerialConfig::default(), true);

        // Empty payload is a no-op.
        state.append_rx_bytes(b"", "15:10:00.000");
        assert!(state.logs.is_empty());
        assert_eq!(state.counters.rx_bytes, 0);

        state.append_rx_bytes(b"I", "15:10:00.001");
        state.append_rx_bytes(b": 35559\r\nI: 35563\r\n", "15:10:00.002");
        state.append_rx_bytes(b"I: TH boo", "15:10:00.003");
        state.append_rx_bytes(b"t=1\r\n", "15:10:00.004");

        assert_eq!(state.logs.len(), 3);
        assert_eq!(state.logs[0].bytes, b"I: 35559\r\n");
        assert_eq!(state.logs[1].bytes, b"I: 35563\r\n");
        assert_eq!(state.logs[2].bytes, b"I: TH boot=1\r\n");
        assert_eq!(
            state.counters.rx_bytes,
            (b"I".len() + b": 35559\r\nI: 35563\r\n".len() + b"I: TH boo".len() + b"t=1\r\n".len())
                as u64
        );

        // Text mode trims trailing \r\n, while HEX mode preserves 0D 0A.
        assert_eq!(state.logs[0].format_payload(false), "I: 35559");
        assert_eq!(
            state.logs[0].format_payload(true),
            "49 3A 20 33 35 35 35 39 0D 0A"
        );
        assert_eq!(state.logs[2].format_payload(false), "I: TH boot=1");
    }

    #[test]
    fn streaming_rx_line_open_resets_on_tx_clear_and_disconnect() {
        let mut state = SessionState::new_real_session(SerialConfig::default(), true);

        // TX interrupts an open RX line.
        state.append_rx_bytes(b"partial1", "15:11:00.001");
        state.record_tx_bytes(b"AT\r\n", "15:11:00.002");
        state.append_rx_bytes(b"next\n", "15:11:00.003");
        assert_eq!(state.logs.len(), 3);
        assert_eq!(state.logs[0].bytes, b"partial1");
        assert_eq!(state.logs[2].bytes, b"next\n");

        // Clear resets open line.
        state.append_rx_bytes(b"partial2", "15:11:00.004");
        state.clear_receive_logs();
        state.append_rx_bytes(b"fresh\n", "15:11:00.005");
        assert_eq!(state.logs.len(), 1);
        assert_eq!(state.logs[0].bytes, b"fresh\n");

        // Disconnect resets open line.
        state.append_rx_bytes(b"partial3", "15:11:00.006");
        state.set_connected(false);
        state.set_connected(true);
        state.append_rx_bytes(b"reconnected\n", "15:11:00.007");
        assert_eq!(state.logs.len(), 3);
        assert_eq!(state.logs[1].bytes, b"partial3");
        assert_eq!(state.logs[2].bytes, b"reconnected\n");
    }

    #[test]
    fn display_hex_and_send_hex_operate_independently() {
        let mut state = SessionState::new_real_session(SerialConfig::default(), true);

        // a) display_hex = true, send_hex = false -> sends UTF-8 bytes of current input text ("41 54")
        state.send.text = "AT".to_string();
        state.toggle_send_display_hex().unwrap();
        assert!(state.send.display_hex);
        assert!(!state.send.send_hex);
        assert_eq!(state.send.text, "41 54");
        assert_eq!(state.prepare_send_bytes(), Ok(b"41 54".to_vec()));

        // b) display_hex = false, send_hex = true -> parses current input text as HEX
        state.toggle_send_display_hex().unwrap();
        state.toggle_send_format_hex();
        assert!(!state.send.display_hex);
        assert!(state.send.send_hex);
        assert_eq!(state.send.text, "AT");

        state.send.text = "41 54".to_string();
        assert_eq!(state.prepare_send_bytes(), Ok(vec![0x41, 0x54]));

        state.send.text = "4142".to_string();
        assert!(matches!(
            state.prepare_send_bytes(),
            Err(SendPayloadError::InvalidHex(_))
        ));

        // Toggling send_hex does not alter display_hex, text, or receive.hex_mode
        state.toggle_send_format_hex();
        assert!(!state.send.send_hex);
        assert!(!state.send.display_hex);
        assert!(!state.receive.hex_mode);
        assert_eq!(state.send.text, "4142");
    }

    #[test]
    fn line_ending_cycles_and_appends_only_in_text_mode() {
        let mut state = SessionState::new_simulated_demo();
        state.toggle_simulated_connection();
        state.send.text = "AT".to_string();

        assert_eq!(state.send.line_ending, SendLineEnding::None);
        assert_eq!(state.prepare_send_bytes(), Ok(b"AT".to_vec()));

        state.cycle_send_line_ending();
        assert_eq!(state.send.line_ending, SendLineEnding::CrLf);
        assert_eq!(state.prepare_send_bytes(), Ok(b"AT\r\n".to_vec()));

        state.cycle_send_line_ending();
        assert_eq!(state.send.line_ending, SendLineEnding::Lf);
        assert_eq!(state.prepare_send_bytes(), Ok(b"AT\n".to_vec()));

        // HEX send mode ignores line ending suffix
        state.send.send_hex = true;
        state.send.text = "41 54".to_string();
        assert_eq!(state.prepare_send_bytes(), Ok(vec![0x41, 0x54]));

        state.cycle_send_line_ending();
        assert_eq!(state.send.line_ending, SendLineEnding::None);
    }

    #[test]
    fn lowering_buffer_limit_drops_oldest_bytes() {
        let mut state = SessionState::new_simulated_demo();
        assert_eq!(state.log_buffer_limit, DEFAULT_LOG_BUFFER_LIMIT as usize);
        assert_eq!(state.logs.len(), 3);
        let used: usize = state.logs.iter().map(|entry| entry.bytes.len()).sum();
        assert_eq!(state.log_buffer_used_bytes(), used);

        let newest = state.logs.last().unwrap().bytes.len();
        state.set_log_buffer_limit(newest);
        assert_eq!(state.logs.len(), 1);
        assert_eq!(state.logs[0].id, 3);

        state.receive.paused = true;
        state.record_log(LogEntry::new_simulated(
            9,
            "14:26:08.000",
            LogDirection::Rx,
            b"QUEUED".to_vec(),
        ));
        state.record_log(LogEntry::new_simulated(
            10,
            "14:26:08.001",
            LogDirection::Rx,
            b"QUEUED2".to_vec(),
        ));
        state.set_log_buffer_limit(b"QUEUED2".len());
        assert_eq!(state.logs.len(), 1);
        assert_eq!(state.retained_while_paused.len(), 1);
        assert_eq!(state.retained_while_paused[0].id, 10);
    }
}
