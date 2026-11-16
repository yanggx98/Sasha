/// Direction of a serial log entry.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogDirection {
    Rx,
    Tx,
    System,
}

impl LogDirection {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Rx => "RX",
            Self::Tx => "TX",
            Self::System => "SYS",
        }
    }
}

/// A single entry in the session log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: String,
    pub direction: LogDirection,
    pub bytes: Vec<u8>,
    /// Explicitly marks whether this entry comes from simulated demo data.
    pub is_simulated: bool,
}

impl LogEntry {
    pub fn new_simulated(
        id: u64,
        timestamp: impl Into<String>,
        direction: LogDirection,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            id,
            timestamp: timestamp.into(),
            direction,
            bytes: bytes.into(),
            is_simulated: true,
        }
    }

    /// Formats payload as UTF-8 text or space-separated uppercase HEX bytes.
    pub fn format_payload(&self, hex_mode: bool) -> String {
        if hex_mode {
            self.bytes
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            String::from_utf8_lossy(&self.bytes).into_owned()
        }
    }
}

/// Errors that can prevent simulated TX submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendPayloadError {
    Disconnected,
    EmptyInput,
    InvalidHex(&'static str),
}

/// Parses strict space-separated 2-digit HEX byte pairs (e.g. `"41 54"`).
/// Rejects odd-length tokens, unseparated pairs (`"4142"`), prefixes (`"0x41"`),
/// and non-hex characters without modifying or submitting any bytes.
pub fn parse_hex_bytes(input: &str) -> Result<Vec<u8>, SendPayloadError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(SendPayloadError::EmptyInput);
    }

    let mut bytes = Vec::new();
    for token in trimmed.split(' ').filter(|s| !s.is_empty()) {
        if token.len() != 2 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(SendPayloadError::InvalidHex(
                "HEX 格式无效：请使用空格分隔的两位十六进制字节（如 41 54）",
            ));
        }
        let byte = u8::from_str_radix(token, 16).map_err(|_| {
            SendPayloadError::InvalidHex(
                "HEX 格式无效：请使用空格分隔的两位十六进制字节（如 41 54）",
            )
        })?;
        bytes.push(byte);
    }

    Ok(bytes)
}

/// Formats visible log entries into the exact text representation shown in `LogView`
/// for the current `show_timestamp` and `hex_mode` flags.
pub fn format_visible_logs(logs: &[LogEntry], show_timestamp: bool, hex_mode: bool) -> String {
    logs.iter()
        .map(|entry| selectable_log_line(entry, show_timestamp, hex_mode).text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// One log row's selectable text. Timestamp, direction tag, and payload are
/// joined with a single space, matching [`format_visible_logs`]. The simulated
/// badge and the session footer are not part of this string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectableLogLine {
    pub text: String,
    pub timestamp_bytes: Option<std::ops::Range<usize>>,
    pub tag_bytes: std::ops::Range<usize>,
    pub payload_bytes: std::ops::Range<usize>,
}

/// Builds the selectable display string for one log row.
pub fn selectable_log_line(
    entry: &LogEntry,
    show_timestamp: bool,
    hex_mode: bool,
) -> SelectableLogLine {
    let payload = entry.format_payload(hex_mode);
    let tag = entry.direction.tag();
    if show_timestamp {
        let timestamp = format!("[{}]", entry.timestamp);
        let text = format!("{timestamp} {tag} {payload}");
        let timestamp_end = timestamp.len();
        let tag_start = timestamp_end + 1;
        let tag_end = tag_start + tag.len();
        let payload_start = tag_end + 1;
        SelectableLogLine {
            text,
            timestamp_bytes: Some(0..timestamp_end),
            tag_bytes: tag_start..tag_end,
            payload_bytes: payload_start..payload_start + payload.len(),
        }
    } else {
        let text = format!("{tag} {payload}");
        let tag_end = tag.len();
        let payload_start = tag_end + 1;
        SelectableLogLine {
            text,
            timestamp_bytes: None,
            tag_bytes: 0..tag_end,
            payload_bytes: payload_start..payload_start + payload.len(),
        }
    }
}

/// Selectable rows joined the same way a full-log copy is joined.
#[cfg(test)]
fn selectable_log_document(logs: &[LogEntry], show_timestamp: bool, hex_mode: bool) -> String {
    format_visible_logs(logs, show_timestamp, hex_mode)
}

/// A character position in the selectable log document.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LogTextPoint {
    line: usize,
    char_index: usize,
}

/// Copies the inclusive-exclusive character span from `anchor` to `cursor`.
///
/// Partial selections are substrings of that row. A span that crosses rows is
/// joined with `\n` and does not include text outside the span. An empty span
/// returns `None` so the caller does not replace the clipboard.
#[cfg(test)]
fn selected_log_text(
    lines: &[impl AsRef<str>],
    anchor: LogTextPoint,
    cursor: LogTextPoint,
) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let (start, end) = if (anchor.line, anchor.char_index) <= (cursor.line, cursor.char_index) {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    if start.line >= lines.len() {
        return None;
    }
    if start.line == end.line && start.char_index == end.char_index {
        return None;
    }

    let mut parts = Vec::new();
    let last_line = end.line.min(lines.len() - 1);
    for line_index in start.line..=last_line {
        let line = lines[line_index].as_ref();
        let char_len = line.chars().count();
        let from = if line_index == start.line {
            start.char_index.min(char_len)
        } else {
            0
        };
        let to = if line_index == end.line {
            end.char_index.min(char_len)
        } else {
            char_len
        };
        if from > to {
            continue;
        }
        let byte_from = char_byte_index(line, from);
        let byte_to = char_byte_index(line, to);
        parts.push(line[byte_from..byte_to].to_string());
    }

    if parts.is_empty() {
        return None;
    }
    let text = parts.join("\n");
    copied_log_text(&text)
}

#[cfg(test)]
fn char_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

/// Exact clipboard payload for a log selection.
/// Empty means there is no selection and the clipboard must be left unchanged.
pub fn copied_log_text(selected: &str) -> Option<String> {
    if selected.is_empty() {
        None
    } else {
        Some(selected.to_string())
    }
}

/// Applies a log copy to `current`, or keeps `current` when nothing is selected.
#[cfg(test)]
fn clipboard_after_log_copy(current: &str, selected: Option<&str>) -> String {
    selected
        .and_then(copied_log_text)
        .unwrap_or_else(|| current.to_string())
}

/// Tracks whether a log selection is still valid for the current display.
///
/// Switching HEX/text or timestamp visibility, or clearing the log, drops the
/// selection. Appending rows does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSelectionModel {
    show_timestamp: bool,
    hex_mode: bool,
    log_len: usize,
    selected: Option<String>,
}

impl LogSelectionModel {
    pub fn new(show_timestamp: bool, hex_mode: bool, log_len: usize) -> Self {
        Self {
            show_timestamp,
            hex_mode,
            log_len,
            selected: None,
        }
    }

    pub fn set_selected_text(&mut self, text: &str) {
        self.selected = copied_log_text(text);
    }

    pub fn copy_payload(&self) -> Option<String> {
        self.selected.clone()
    }

    #[cfg(test)]
    fn clipboard_after_copy(&self, current: &str) -> String {
        clipboard_after_log_copy(current, self.selected.as_deref())
    }

    /// Updates the remembered log length without invalidating a selection.
    /// Used when rows are appended but the existing display text is unchanged.
    pub fn observe_log_len(&mut self, log_len: usize) {
        self.log_len = log_len;
    }

    /// Returns whether the display change invalidated the selection.
    pub fn sync_display(&mut self, show_timestamp: bool, hex_mode: bool, log_len: usize) -> bool {
        let invalidate = self.show_timestamp != show_timestamp
            || self.hex_mode != hex_mode
            || (log_len == 0 && self.log_len != 0);
        if invalidate {
            self.selected = None;
        }
        self.show_timestamp = show_timestamp;
        self.hex_mode = hex_mode;
        self.log_len = log_len;
        invalidate
    }
}

/// Returns a `HH:MM:SS.mmm` UTC timestamp string for new simulated log entries.
pub fn current_timestamp_hms_ms() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = duration.as_secs();
    let millis = duration.subsec_millis();
    let secs_of_day = total_secs % 86_400;
    let hours = secs_of_day / 3_600;
    let minutes = (secs_of_day % 3_600) / 60;
    let seconds = secs_of_day % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_text_and_hex_payloads() {
        let entry = LogEntry::new_simulated(1, "14:26:07.343", LogDirection::Rx, b"OK".to_vec());
        assert!(entry.is_simulated);
        assert_eq!(entry.format_payload(false), "OK");
        assert_eq!(entry.format_payload(true), "4F 4B");
    }

    #[test]
    fn parses_strict_space_separated_hex_pairs() {
        assert_eq!(parse_hex_bytes("41 54 0d 0A"), Ok(vec![0x41, 0x54, 0x0D, 0x0A]));
        assert_eq!(parse_hex_bytes("  ff  00 "), Ok(vec![0xFF, 0x00]));
        assert_eq!(parse_hex_bytes("   "), Err(SendPayloadError::EmptyInput));
        assert!(matches!(
            parse_hex_bytes("4142"),
            Err(SendPayloadError::InvalidHex(_))
        ));
        assert!(matches!(
            parse_hex_bytes("4"),
            Err(SendPayloadError::InvalidHex(_))
        ));
        assert!(matches!(
            parse_hex_bytes("0x41"),
            Err(SendPayloadError::InvalidHex(_))
        ));
        assert!(matches!(
            parse_hex_bytes("ZZ 41"),
            Err(SendPayloadError::InvalidHex(_))
        ));
        assert!(matches!(
            parse_hex_bytes("41,42"),
            Err(SendPayloadError::InvalidHex(_))
        ));
    }

    #[test]
    fn formats_visible_logs_for_save_matching_display_flags() {
        let logs = vec![
            LogEntry::new_simulated(1, "14:26:07.285", LogDirection::Tx, b"AT".to_vec()),
            LogEntry::new_simulated(2, "14:26:07.343", LogDirection::Rx, b"OK".to_vec()),
        ];

        assert_eq!(
            format_visible_logs(&logs, true, false),
            "[14:26:07.285] TX AT\n[14:26:07.343] RX OK"
        );
        assert_eq!(
            format_visible_logs(&logs, false, false),
            "TX AT\nRX OK"
        );
        assert_eq!(
            format_visible_logs(&logs, true, true),
            "[14:26:07.285] TX 41 54\n[14:26:07.343] RX 4F 4B"
        );
        assert_eq!(
            format_visible_logs(&logs, false, true),
            "TX 41 54\nRX 4F 4B"
        );
    }

    fn chinese_rx() -> LogEntry {
        LogEntry::new_simulated(1, "14:26:07.343", LogDirection::Rx, "数据".as_bytes().to_vec())
    }

    #[test]
    fn selectable_line_matches_save_text_and_skips_simulated_decoration() {
        let logs = vec![
            LogEntry::new_simulated(1, "14:26:07.285", LogDirection::Tx, "数据".as_bytes().to_vec()),
            LogEntry::new_simulated(2, "14:26:07.343", LogDirection::Rx, vec![0x0A, 0xFF]),
        ];
        let notice = "* 模拟会话（无真实串口硬件）";
        for show_timestamp in [true, false] {
            for hex_mode in [true, false] {
                let document = selectable_log_document(&logs, show_timestamp, hex_mode);
                assert_eq!(
                    document,
                    format_visible_logs(&logs, show_timestamp, hex_mode)
                );
                assert!(!document.contains("[模拟]"));
                assert!(!document.contains(notice));
                assert!(logs.iter().all(|entry| entry.is_simulated));
            }
        }

        let text_line = selectable_log_line(&logs[0], true, false);
        assert_eq!(text_line.text, "[14:26:07.285] TX 数据");
        assert_eq!(
            text_line.timestamp_bytes.as_ref().map(|range| &text_line.text[range.clone()]),
            Some("[14:26:07.285]")
        );
        assert_eq!(&text_line.text[text_line.tag_bytes.clone()], "TX");
        assert_eq!(&text_line.text[text_line.payload_bytes.clone()], "数据");

        let hidden_timestamp = selectable_log_line(&logs[0], false, false);
        assert_eq!(hidden_timestamp.text, "TX 数据");
        assert!(hidden_timestamp.timestamp_bytes.is_none());
        assert!(!hidden_timestamp.text.contains("14:26:07.285"));

        let hex_line = selectable_log_line(&logs[0], true, true);
        assert_eq!(hex_line.text, "[14:26:07.285] TX E6 95 B0 E6 8D AE");
        assert!(!hex_line.text.contains("数据"));
        assert_eq!(&hex_line.text[hex_line.payload_bytes.clone()], "E6 95 B0 E6 8D AE");
    }

    #[test]
    fn selection_is_a_substring_and_cross_line_uses_newlines() {
        let first = selectable_log_line(&chinese_rx(), true, false);
        let second = selectable_log_line(
            &LogEntry::new_simulated(2, "14:26:08.001", LogDirection::Tx, b"AB".to_vec()),
            true,
            false,
        );
        let lines = [first.text.as_str(), second.text.as_str()];
        assert_eq!(lines[0], "[14:26:07.343] RX 数据");
        assert_eq!(lines[1], "[14:26:08.001] TX AB");

        let data_at = lines[0].find("数据").unwrap();
        let data_char = lines[0][..data_at].chars().count();
        assert_eq!(
            selected_log_text(
                &lines,
                LogTextPoint { line: 0, char_index: data_char },
                LogTextPoint { line: 0, char_index: data_char + 1 },
            )
            .as_deref(),
            Some("数")
        );

        let rx_at = lines[0].find("RX").unwrap();
        let rx_char = lines[0][..rx_at].chars().count();
        assert_eq!(
            selected_log_text(
                &lines,
                LogTextPoint { line: 0, char_index: rx_char },
                LogTextPoint { line: 0, char_index: rx_char + 2 },
            )
            .as_deref(),
            Some("RX")
        );

        let timestamp_end = lines[0].find(']').unwrap() + 1;
        let timestamp_chars = lines[0][..timestamp_end].chars().count();
        assert_eq!(
            selected_log_text(
                &lines,
                LogTextPoint { line: 0, char_index: 0 },
                LogTextPoint { line: 0, char_index: timestamp_chars },
            )
            .as_deref(),
            Some("[14:26:07.343]")
        );

        let hidden = selectable_log_line(&chinese_rx(), false, false);
        assert_eq!(
            selected_log_text(
                &[hidden.text.as_str()],
                LogTextPoint { line: 0, char_index: 0 },
                LogTextPoint { line: 0, char_index: usize::MAX },
            )
            .as_deref(),
            Some("RX 数据")
        );

        let hex = selectable_log_line(&chinese_rx(), false, true);
        assert_eq!(hex.text, "RX E6 95 B0 E6 8D AE");
        let payload_char = hex.text[..hex.payload_bytes.start].chars().count();
        let first_byte_chars = "E6 95 B0".chars().count();
        assert_eq!(
            selected_log_text(
                &[hex.text.as_str()],
                LogTextPoint { line: 0, char_index: payload_char },
                LogTextPoint {
                    line: 0,
                    char_index: payload_char + first_byte_chars,
                },
            )
            .as_deref(),
            Some("E6 95 B0")
        );

        let cross = selected_log_text(
            &lines,
            LogTextPoint { line: 0, char_index: data_char },
            LogTextPoint {
                line: 1,
                char_index: lines[1].find("AB").unwrap() + 1,
            },
        )
        .unwrap();
        assert_eq!(cross, "数据\n[14:26:08.001] TX A");
        assert!(!cross.contains("[模拟]"));
        assert_eq!(cross.chars().filter(|ch| *ch == '\n').count(), 1);
    }

    #[test]
    fn mode_switch_or_clear_drops_selection_and_empty_copy_keeps_clipboard() {
        let mut model = LogSelectionModel::new(true, false, 2);
        assert_eq!(model.clipboard_after_copy("KEEP"), "KEEP");
        assert!(model.copy_payload().is_none());

        model.set_selected_text(" 数据 ");
        assert_eq!(model.copy_payload().as_deref(), Some(" 数据 "));
        assert_eq!(model.clipboard_after_copy("KEEP"), " 数据 ");

        assert!(model.sync_display(true, true, 2), "HEX switch clears");
        assert!(model.copy_payload().is_none());
        assert_eq!(model.clipboard_after_copy("KEEP"), "KEEP");

        model.set_selected_text("RX E6 95 B0");
        assert!(model.sync_display(false, true, 2), "timestamp switch clears");
        assert_eq!(model.clipboard_after_copy("KEEP"), "KEEP");

        model.set_selected_text("RX");
        assert!(!model.sync_display(false, true, 4), "append keeps selection");
        assert_eq!(model.copy_payload().as_deref(), Some("RX"));
        model.observe_log_len(4);

        assert!(model.sync_display(false, true, 0), "clearing logs clears");
        assert!(model.copy_payload().is_none());
        assert_eq!(model.clipboard_after_copy("KEEP"), "KEEP");

        assert!(!model.sync_display(false, true, 0));
        assert_eq!(
            selected_log_text(
                &[] as &[&str],
                LogTextPoint { line: 0, char_index: 0 },
                LogTextPoint { line: 0, char_index: 4 },
            ),
            None
        );
        assert_eq!(copied_log_text(""), None);
        assert_eq!(clipboard_after_log_copy("KEEP", None), "KEEP");
        assert_eq!(clipboard_after_log_copy("KEEP", Some("")), "KEEP");
        assert_eq!(
            clipboard_after_log_copy("KEEP", Some("RX 数据")),
            "RX 数据"
        );
    }
}
