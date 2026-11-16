use super::log::format_hex_bytes;
use super::{
    ActivePickerKind, LogDirection, LogEntry, SendPayloadError, SerialConfig, format_visible_logs,
    parse_hex_bytes,
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

    pub fn status_label(self) -> &'static str {
        match self {
            Self::Disconnected => "模拟未连接",
            Self::Connected => "模拟已连接",
        }
    }

    pub fn action_label(self) -> &'static str {
        match self {
            Self::Disconnected => "模拟连接",
            Self::Connected => "模拟断开",
        }
    }
}

/// Receive view display flags (independent from send card HEX flag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiveViewState {
    pub hex_mode: bool,
    pub show_timestamp: bool,
    pub paused: bool,
}

impl Default for ReceiveViewState {
    fn default() -> Self {
        Self {
            hex_mode: false,
            show_timestamp: true,
            paused: false,
        }
    }
}

/// Floating send card state (independent from receive HEX flag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendDraftState {
    pub text: String,
    pub hex_mode: bool,
}

impl Default for SendDraftState {
    fn default() -> Self {
        Self {
            text: String::new(),
            hex_mode: false,
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
#[derive(Debug, Clone)]
pub struct SessionState {
    pub is_simulated_session: bool,
    pub simulated_notice: &'static str,
    pub config: SerialConfig,
    pub connection: SimulatedConnectionState,
    pub receive: ReceiveViewState,
    pub send: SendDraftState,
    pub counters: SessionCounters,
    pub logs: Vec<LogEntry>,
    pub retained_while_paused: Vec<LogEntry>,
    #[allow(dead_code)]
    pub active_picker: Option<ActivePickerKind>,
    next_log_id: u64,
    next_sim_rx_seq: u64,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new_simulated_demo()
    }
}

impl SessionState {
    /// Creates a clearly-labelled simulated demo session for UI skeleton verification.
    pub fn new_simulated_demo() -> Self {
        let tx_payload = b"AT+VERSION".to_vec();
        let rx_payload_1 = b"+VERSION: NanoModem v1.2.3".to_vec();
        let rx_payload_2 = b"OK".to_vec();

        let tx_bytes = tx_payload.len() as u64;
        let rx_bytes = (rx_payload_1.len() + rx_payload_2.len()) as u64;

        Self {
            is_simulated_session: true,
            simulated_notice: "模拟会话（无真实串口硬件）",
            config: SerialConfig::default(),
            connection: SimulatedConnectionState::Disconnected,
            receive: ReceiveViewState::default(),
            send: SendDraftState {
                text: "AT+VERSION".to_string(),
                hex_mode: false,
            },
            counters: SessionCounters { rx_bytes, tx_bytes },
            logs: vec![
                LogEntry::new_simulated(1, "14:26:07.285", LogDirection::Tx, tx_payload),
                LogEntry::new_simulated(2, "14:26:07.342", LogDirection::Rx, rx_payload_1),
                LogEntry::new_simulated(3, "14:26:07.343", LogDirection::Rx, rx_payload_2),
            ],
            retained_while_paused: Vec::new(),
            active_picker: None,
            next_log_id: 4,
            next_sim_rx_seq: 0,
        }
    }

    /// Creates a real hardware serial session with empty logs and zeroed counters.
    #[allow(dead_code)]
    pub fn new_real_session(config: SerialConfig, connected: bool) -> Self {
        Self {
            is_simulated_session: false,
            simulated_notice: "",
            config,
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
            active_picker: None,
            next_log_id: 1,
            next_sim_rx_seq: 0,
        }
    }

    /// Returns the mode-aware connection status label (real vs. simulated).
    #[allow(dead_code)]
    pub fn connection_status_label(&self) -> &'static str {
        if self.is_simulated_session {
            self.connection.status_label()
        } else if self.connection.is_connected() {
            "已连接"
        } else {
            "未连接"
        }
    }

    /// Returns the mode-aware connection toggle button label (real vs. simulated).
    #[allow(dead_code)]
    pub fn connection_action_label(&self) -> &'static str {
        if self.is_simulated_session {
            self.connection.action_label()
        } else if self.connection.is_connected() {
            "断开"
        } else {
            "连接"
        }
    }

    /// Explicitly sets the connection state.
    #[allow(dead_code)]
    pub fn set_connected(&mut self, connected: bool) {
        self.connection = if connected {
            SimulatedConnectionState::Connected
        } else {
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
        }
    }

    /// Records a log entry, queuing it while receive display is paused so
    /// visible logs stay frozen and RX/TX events preserve arrival order on resume.
    fn record_log(&mut self, entry: LogEntry) {
        if self.receive.paused {
            self.retained_while_paused.push(entry);
        } else {
            self.logs.push(entry);
        }
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
    }

    /// Returns the exact text currently visible in `LogView` for Save As export.
    pub fn visible_log_export_text(&self) -> String {
        format_visible_logs(&self.logs, self.receive.show_timestamp, self.receive.hex_mode)
    }

    /// Appends received bytes (used by real serial backend) and updates RX counters.
    /// Respects `receive.paused` and omits the simulated badge when in real mode.
    #[allow(dead_code)]
    pub fn append_rx_bytes(&mut self, payload: &[u8], timestamp: &str) {
        self.counters.rx_bytes += payload.len() as u64;
        let entry = self.build_log_entry(timestamp, LogDirection::Rx, payload.to_vec());
        self.record_log(entry);
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

    /// Toggles send card HEX mode independently from receive HEX display, converting
    /// `self.send.text` between UTF-8 text and space-separated uppercase HEX bytes.
    ///
    /// When switching from HEX to text, blocks the transition and preserves both
    /// `hex_mode == true` and the original `text` if the HEX input is malformed or
    /// cannot be decoded as valid UTF-8.
    pub fn toggle_send_hex(&mut self) -> Result<(), &'static str> {
        if self.send.hex_mode {
            if self.send.text.trim().is_empty() {
                self.send.text.clear();
                self.send.hex_mode = false;
                return Ok(());
            }

            let bytes = match parse_hex_bytes(&self.send.text) {
                Ok(bytes) => bytes,
                Err(SendPayloadError::InvalidHex(msg)) => return Err(msg),
                Err(SendPayloadError::EmptyInput) => {
                    self.send.text.clear();
                    self.send.hex_mode = false;
                    return Ok(());
                }
                Err(SendPayloadError::Disconnected) => return Err("HEX 转换失败"),
            };

            let text = String::from_utf8(bytes)
                .map_err(|_| "HEX 字节无法解码为有效 UTF-8 文本，无法切换回文本模式")?;
            self.send.text = text;
            self.send.hex_mode = false;
            Ok(())
        } else {
            self.send.text = format_hex_bytes(self.send.text.as_bytes());
            self.send.hex_mode = true;
            Ok(())
        }
    }

    /// Validates connection, non-empty draft, and HEX syntax without mutating state.
    pub fn prepare_send_bytes(&self) -> Result<Vec<u8>, SendPayloadError> {
        if !self.connection.is_connected() {
            return Err(SendPayloadError::Disconnected);
        }

        if self.send.hex_mode {
            parse_hex_bytes(&self.send.text)
        } else if self.send.text.is_empty() {
            Err(SendPayloadError::EmptyInput)
        } else {
            Ok(self.send.text.as_bytes().to_vec())
        }
    }

    /// Records transmitted bytes after a successful serial write, updating TX counters
    /// and respecting `receive.paused`.
    pub fn record_tx_bytes(&mut self, payload: &[u8], timestamp: &str) {
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
        assert!(SimulatedConnectionState::Disconnected.status_label().contains("模拟"));
        assert!(SimulatedConnectionState::Connected.status_label().contains("模拟"));
        assert!(SimulatedConnectionState::Disconnected.action_label().contains("模拟"));
        assert!(SimulatedConnectionState::Connected.action_label().contains("模拟"));
    }

    #[test]
    fn receive_and_send_hex_states_are_independent() {
        let mut state = SessionState::new_simulated_demo();
        assert!(!state.receive.hex_mode);
        assert!(!state.send.hex_mode);

        state.receive.hex_mode = true;
        assert!(state.receive.hex_mode);
        assert!(!state.send.hex_mode);

        state.send.hex_mode = true;
        state.receive.hex_mode = false;
        assert!(!state.receive.hex_mode);
        assert!(state.send.hex_mode);
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
        state.send.hex_mode = false;
        let tx1 = state
            .submit_send_with_timestamp("14:26:08.200")
            .expect("TX text should submit while paused");
        assert_eq!(tx1, b"AT+VERSION");

        // 3. Second RX arrives while paused (UTF-8 multi-byte payload)
        let rx2_seq = state.inject_next_deterministic_simulated_rx("14:26:08.300");
        assert_eq!(rx2_seq, 2);
        let rx2_expected = "SIM_RX_02: 数据".as_bytes().to_vec();

        // 4. Second TX in HEX mode while paused
        state.toggle_send_hex().unwrap();
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
        state.toggle_send_hex().unwrap();
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
        state.send.hex_mode = false;
        state.send.text = "你好".to_string();
        let submitted_utf8 = state
            .submit_send_with_timestamp("14:26:11.000")
            .expect("UTF-8 text should submit");
        assert_eq!(submitted_utf8, "你好".as_bytes());
        assert_eq!(submitted_utf8.len(), 6);
        assert_eq!(state.send.text, "你好");
        assert_eq!(state.counters.tx_bytes, initial_tx + 6);
        assert_eq!(state.logs.last().unwrap().bytes, vec![0xE4, 0xBD, 0xA0, 0xE5, 0xA5, 0xBD]);

        // Valid space-separated HEX bytes.
        state.toggle_send_hex().unwrap();
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
    fn toggle_send_hex_converts_ascii_utf8_and_empty_bidirectionally() {
        let mut state = SessionState::new_simulated_demo();

        // ASCII: text -> hex -> text
        state.send.hex_mode = false;
        state.send.text = "AT+VERSION".to_string();
        assert_eq!(state.toggle_send_hex(), Ok(()));
        assert!(state.send.hex_mode);
        assert_eq!(state.send.text, "41 54 2B 56 45 52 53 49 4F 4E");

        assert_eq!(state.toggle_send_hex(), Ok(()));
        assert!(!state.send.hex_mode);
        assert_eq!(state.send.text, "AT+VERSION");

        // Chinese UTF-8: text -> hex -> text
        state.send.text = "数据".to_string();
        assert_eq!(state.toggle_send_hex(), Ok(()));
        assert!(state.send.hex_mode);
        assert_eq!(state.send.text, "E6 95 B0 E6 8D AE");

        assert_eq!(state.toggle_send_hex(), Ok(()));
        assert!(!state.send.hex_mode);
        assert_eq!(state.send.text, "数据");

        // Lowercase / extra spaces in HEX -> text
        state.send.hex_mode = true;
        state.send.text = "  e6  95 b0 ".to_string();
        assert_eq!(state.toggle_send_hex(), Ok(()));
        assert!(!state.send.hex_mode);
        assert_eq!(state.send.text, "数");

        // Empty string: text -> hex -> text
        state.send.text.clear();
        assert_eq!(state.toggle_send_hex(), Ok(()));
        assert!(state.send.hex_mode);
        assert_eq!(state.send.text, "");

        assert_eq!(state.toggle_send_hex(), Ok(()));
        assert!(!state.send.hex_mode);
        assert_eq!(state.send.text, "");

        // Whitespace-only in HEX -> clears to "" and switches to text
        state.send.hex_mode = true;
        state.send.text = "   ".to_string();
        assert_eq!(state.toggle_send_hex(), Ok(()));
        assert!(!state.send.hex_mode);
        assert_eq!(state.send.text, "");
    }

    #[test]
    fn toggle_send_hex_blocks_invalid_hex_and_non_utf8_bytes() {
        let mut state = SessionState::new_simulated_demo();
        state.send.hex_mode = true;

        // Odd-length token
        state.send.text = "41 5".to_string();
        let err = state.toggle_send_hex().unwrap_err();
        assert!(err.contains("HEX"));
        assert!(state.send.hex_mode);
        assert_eq!(state.send.text, "41 5");

        // Unseparated pair
        state.send.text = "4142".to_string();
        let err = state.toggle_send_hex().unwrap_err();
        assert!(err.contains("HEX"));
        assert!(state.send.hex_mode);
        assert_eq!(state.send.text, "4142");

        // Non-UTF-8 bytes
        state.send.text = "FF 00".to_string();
        let err = state.toggle_send_hex().unwrap_err();
        assert!(err.contains("UTF-8"));
        assert!(state.send.hex_mode);
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
        assert_eq!(state.prepare_send_bytes(), Err(SendPayloadError::EmptyInput));

        // Invalid HEX fails without mutating state.
        state.send.hex_mode = true;
        state.send.text = "4142".to_string();
        assert!(matches!(
            state.prepare_send_bytes(),
            Err(SendPayloadError::InvalidHex(_))
        ));
        assert!(state.logs.is_empty());

        // Valid HEX prepares bytes without mutating counters or logs.
        state.send.text = "41 54".to_string();
        let prepared = state.prepare_send_bytes().expect("valid HEX should prepare");
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
}
