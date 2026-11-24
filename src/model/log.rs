use super::ansi::{AnsiSpans, parse_ansi};

/// Direction of a serial log entry.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogDirection {
    Rx,
    Tx,
    System,
    /// A Lua script's debug log: `log.info` / `print`, `log.warn` and `log.error`.
    ScriptInfo,
    ScriptWarn,
    ScriptError,
    /// Content a Lua script chose to show with `output.print`.
    ScriptOutput,
}

impl LogDirection {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Rx => "RX",
            Self::Tx => "TX",
            Self::System => "SYS",
            Self::ScriptInfo => "LOG",
            Self::ScriptWarn => "WARN",
            Self::ScriptError => "ERR",
            Self::ScriptOutput => "LUA",
        }
    }

    /// Lines a script printed itself, as opposed to serial data.
    pub fn is_script(self) -> bool {
        self.is_script_debug() || self == Self::ScriptOutput
    }

    /// The levelled debug log lines (`log.*`, `print`), shown only while script debugging is
    /// on and never part of a live stream capture.
    pub fn is_script_debug(self) -> bool {
        matches!(
            self,
            Self::ScriptInfo | Self::ScriptWarn | Self::ScriptError
        )
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
    pub fn new(
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
            is_simulated: false,
        }
    }

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

    /// Formats payload as UTF-8 text (stripping `\0`, trailing `\r`/`\n` and ANSI
    /// escape sequences) or space-separated uppercase HEX bytes.
    pub fn format_payload(&self, hex_mode: bool) -> String {
        self.format_payload_styled(hex_mode).0
    }

    /// Like [`Self::format_payload`], plus ANSI style spans (byte ranges into the
    /// returned text). HEX mode never has spans.
    pub fn format_payload_styled(&self, hex_mode: bool) -> (String, AnsiSpans) {
        // Script output is text the script chose to show, never raw serial bytes.
        if hex_mode && !self.direction.is_script() {
            (format_hex_bytes(&self.bytes), Vec::new())
        } else {
            let text = String::from_utf8_lossy(&self.bytes);
            let text = text.trim_end_matches(&['\r', '\n'][..]).replace('\0', "");
            parse_ansi(&text)
        }
    }
}

/// Formats bytes as space-separated uppercase 2-digit HEX pairs (e.g. `"41 54"`).
pub fn format_hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Formats send input in HEX mode into uppercase 2-digit space-separated groups
/// (e.g. `"4154"` -> `"41 54"`, `"4Edddd"` -> `"4E DD DD"`), preserving a single
/// trailing space after an even-length group so users can type or backspace spaces
/// naturally, and computes the updated UTF-8 byte cursor offset.
pub fn format_hex_input_with_cursor(raw: &str, cursor_utf8_offset: usize) -> (String, usize) {
    let clamped_cursor = cursor_utf8_offset.min(raw.len());
    let mut hex_chars = Vec::new();
    let mut hex_before_cursor = 0usize;
    let mut last_hex_end = 0usize;
    let mut trailing_space_after_last_hex = false;
    let mut cursor_after_trailing_space = false;

    for (idx, ch) in raw.char_indices() {
        let ch_end = idx + ch.len_utf8();
        if ch.is_ascii_hexdigit() {
            hex_chars.push(ch.to_ascii_uppercase());
            if idx < clamped_cursor {
                hex_before_cursor += 1;
            }
            last_hex_end = ch_end;
            trailing_space_after_last_hex = false;
            cursor_after_trailing_space = false;
        } else if ch == ' ' && !hex_chars.is_empty() {
            trailing_space_after_last_hex = true;
            if idx < clamped_cursor && idx >= last_hex_end {
                cursor_after_trailing_space = true;
            }
        }
    }

    let n = hex_chars.len();
    if n == 0 {
        return (String::new(), 0);
    }

    let mut formatted = String::with_capacity(n + n / 2 + 1);
    for (i, ch) in hex_chars.iter().enumerate() {
        if i > 0 && i % 2 == 0 {
            formatted.push(' ');
        }
        formatted.push(*ch);
    }

    let keep_trailing_space = n % 2 == 0 && trailing_space_after_last_hex;
    if keep_trailing_space {
        formatted.push(' ');
    }

    let next_cursor =
        if keep_trailing_space && hex_before_cursor == n && cursor_after_trailing_space {
            formatted.len()
        } else if hex_before_cursor == 0 {
            0
        } else {
            hex_before_cursor + (hex_before_cursor - 1) / 2
        };

    (formatted, next_cursor)
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
    /// ANSI style spans, as byte ranges into `text` (already offset past the prefix).
    pub payload_styles: AnsiSpans,
}

/// Builds the selectable display string for one log row.
pub fn selectable_log_line(
    entry: &LogEntry,
    show_timestamp: bool,
    hex_mode: bool,
) -> SelectableLogLine {
    let (payload, mut styles) = entry.format_payload_styled(hex_mode);
    let tag = entry.direction.tag();
    let (text, timestamp_bytes, tag_bytes, payload_start) = if show_timestamp {
        let timestamp = format!("[{}]", entry.timestamp);
        let timestamp_end = timestamp.len();
        let tag_start = timestamp_end + 1;
        let tag_end = tag_start + tag.len();
        (
            format!("{timestamp} {tag} {payload}"),
            Some(0..timestamp_end),
            tag_start..tag_end,
            tag_end + 1,
        )
    } else {
        let tag_end = tag.len();
        (format!("{tag} {payload}"), None, 0..tag_end, tag_end + 1)
    };
    for (range, _) in &mut styles {
        range.start += payload_start;
        range.end += payload_start;
    }
    SelectableLogLine {
        text,
        timestamp_bytes,
        tag_bytes,
        payload_bytes: payload_start..payload_start + payload.len(),
        payload_styles: styles,
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

/// Returns a `HH:MM:SS.mmm` local system timestamp string for new log entries.
pub fn current_timestamp_hms_ms() -> String {
    chrono::Local::now().format("%H:%M:%S%.3f").to_string()
}

/// A single search match within a rendered log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Index of the matching entry in the supplied (possibly filtered) log slice.
    pub log_index: usize,
    /// UTF-8 byte range within `selectable_log_line(entry, show_timestamp, hex_mode).text`.
    pub byte_range: std::ops::Range<usize>,
}

/// Per-tab log search state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogSearchState {
    pub open: bool,
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub active_match: Option<usize>,
}

impl LogSearchState {
    /// Recomputes matches against `logs` for the current display flags.
    /// When `preserve_active` is true, keeps the active index clamped to the new match count.
    pub fn recompute(
        &mut self,
        logs: &[LogEntry],
        show_timestamp: bool,
        hex_mode: bool,
        preserve_active: bool,
    ) {
        self.matches = find_log_matches(logs, &self.query, show_timestamp, hex_mode);
        if self.matches.is_empty() {
            self.active_match = None;
        } else if preserve_active {
            let idx = self.active_match.unwrap_or(0).min(self.matches.len() - 1);
            self.active_match = Some(idx);
        } else {
            self.active_match = Some(0);
        }
    }

    /// Advances to the next match (wrapping around) and returns it.
    pub fn next(&mut self) -> Option<&SearchMatch> {
        if self.matches.is_empty() {
            self.active_match = None;
            return None;
        }
        let next = match self.active_match {
            Some(i) => (i + 1) % self.matches.len(),
            None => 0,
        };
        self.active_match = Some(next);
        self.matches.get(next)
    }

    /// Moves to the previous match (wrapping around) and returns it.
    pub fn prev(&mut self) -> Option<&SearchMatch> {
        if self.matches.is_empty() {
            self.active_match = None;
            return None;
        }
        let prev = match self.active_match {
            Some(0) | None => self.matches.len() - 1,
            Some(i) => i - 1,
        };
        self.active_match = Some(prev);
        self.matches.get(prev)
    }

    /// Returns the currently active match, if any.
    pub fn current(&self) -> Option<&SearchMatch> {
        self.active_match.and_then(|idx| self.matches.get(idx))
    }

    /// Formats `"current/total"` (1-based), `"0/0"`, or `None` when the query is empty.
    pub fn count_label(&self) -> Option<String> {
        if self.query.is_empty() {
            return None;
        }
        if self.matches.is_empty() {
            Some("0/0".to_string())
        } else {
            let current = self.active_match.unwrap_or(0) + 1;
            Some(format!("{}/{}", current, self.matches.len()))
        }
    }
}

/// Finds all case-insensitive non-overlapping matches of `query` across visible log lines.
pub fn find_log_matches(
    logs: &[LogEntry],
    query: &str,
    show_timestamp: bool,
    hex_mode: bool,
) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for (log_index, entry) in logs.iter().enumerate() {
        let line = selectable_log_line(entry, show_timestamp, hex_mode);
        for byte_range in find_case_insensitive_ranges(&line.text, query) {
            matches.push(SearchMatch {
                log_index,
                byte_range,
            });
        }
    }
    matches
}

fn find_case_insensitive_ranges(haystack: &str, needle: &str) -> Vec<std::ops::Range<usize>> {
    if needle.is_empty() || haystack.is_empty() {
        return Vec::new();
    }

    if haystack.is_ascii() && needle.is_ascii() {
        let haystack_lower = haystack.to_ascii_lowercase();
        let needle_lower = needle.to_ascii_lowercase();
        let mut ranges = Vec::new();
        let mut offset = 0;
        while let Some(pos) = haystack_lower[offset..].find(&needle_lower) {
            let start = offset + pos;
            let end = start + needle_lower.len();
            ranges.push(start..end);
            offset = end;
        }
        return ranges;
    }

    let needle_lower: Vec<char> = needle.chars().flat_map(|c| c.to_lowercase()).collect();
    if needle_lower.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut byte_idx = 0;
    while byte_idx < haystack.len() {
        if let Some(matched_len) = starts_with_lower_chars(&haystack[byte_idx..], &needle_lower) {
            let end = byte_idx + matched_len;
            ranges.push(byte_idx..end);
            byte_idx = end;
        } else if let Some(ch) = haystack[byte_idx..].chars().next() {
            byte_idx += ch.len_utf8();
        } else {
            break;
        }
    }
    ranges
}

fn starts_with_lower_chars(slice: &str, needle_lower: &[char]) -> Option<usize> {
    let mut needle_iter = needle_lower.iter().copied();
    let mut current_target = needle_iter.next()?;
    for (offset, ch) in slice.char_indices() {
        for lc in ch.to_lowercase() {
            if lc != current_target {
                return None;
            }
            match needle_iter.next() {
                Some(next) => current_target = next,
                None => return Some(offset + ch.len_utf8()),
            }
        }
    }
    None
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

        let with_nul = LogEntry::new(2, "14:26:07.400", LogDirection::Rx, b"\0OK\r\n".to_vec());
        assert_eq!(with_nul.format_payload(false), "OK");
        assert_eq!(with_nul.format_payload(true), "00 4F 4B 0D 0A");
    }

    #[test]
    fn script_directions_have_level_tags_and_stay_text_in_hex_mode() {
        assert_eq!(LogDirection::ScriptInfo.tag(), "LOG");
        assert_eq!(LogDirection::ScriptWarn.tag(), "WARN");
        assert_eq!(LogDirection::ScriptError.tag(), "ERR");
        assert_eq!(LogDirection::ScriptOutput.tag(), "LUA");
        for direction in [
            LogDirection::ScriptInfo,
            LogDirection::ScriptWarn,
            LogDirection::ScriptError,
            LogDirection::ScriptOutput,
        ] {
            assert!(direction.is_script());
        }
        for direction in [LogDirection::Rx, LogDirection::Tx, LogDirection::System] {
            assert!(!direction.is_script());
            assert!(!direction.is_script_debug());
        }
        // Only the levelled `log.*` lines are debug output; `output.print` is content.
        assert!(!LogDirection::ScriptOutput.is_script_debug());
        for direction in [
            LogDirection::ScriptInfo,
            LogDirection::ScriptWarn,
            LogDirection::ScriptError,
        ] {
            assert!(direction.is_script_debug());
        }

        // A script's message is already text: HEX mode must not turn it into bytes.
        let entry = LogEntry::new(
            1,
            "10:00:00.000",
            LogDirection::ScriptInfo,
            b"T=25".to_vec(),
        );
        assert_eq!(entry.format_payload(true), "T=25");
        assert_eq!(entry.format_payload(false), "T=25");
        assert_eq!(
            selectable_log_line(&entry, true, true).text,
            "[10:00:00.000] LOG T=25"
        );
    }

    #[test]
    fn parses_strict_space_separated_hex_pairs() {
        assert_eq!(
            parse_hex_bytes("41 54 0d 0A"),
            Ok(vec![0x41, 0x54, 0x0D, 0x0A])
        );
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
        assert_eq!(format_visible_logs(&logs, false, false), "TX AT\nRX OK");
        assert_eq!(
            format_visible_logs(&logs, true, true),
            "[14:26:07.285] TX 41 54\n[14:26:07.343] RX 4F 4B"
        );
        assert_eq!(
            format_visible_logs(&logs, false, true),
            "TX 41 54\nRX 4F 4B"
        );
    }

    #[test]
    fn ansi_sequences_are_stripped_from_text_and_styled_in_place() {
        let entry = LogEntry::new(
            1,
            "12:00:00.000",
            LogDirection::Rx,
            b"\x1b[31mERR\x1b[0m ok\x1b[0m\r\n".to_vec(),
        );
        assert_eq!(entry.format_payload(false), "ERR ok");
        assert_eq!(entry.format_payload(true).split(' ').next(), Some("1B"));

        let line = selectable_log_line(&entry, true, false);
        assert_eq!(line.text, "[12:00:00.000] RX ERR ok");
        assert_eq!(line.payload_styles.len(), 1);
        let (range, style) = &line.payload_styles[0];
        assert_eq!(&line.text[range.clone()], "ERR");
        assert_eq!(style.fg, Some(crate::model::ansi::AnsiColor::Indexed(1)));
        assert!(
            selectable_log_line(&entry, false, true)
                .payload_styles
                .is_empty()
        );
    }

    fn chinese_rx() -> LogEntry {
        LogEntry::new_simulated(
            1,
            "14:26:07.343",
            LogDirection::Rx,
            "数据".as_bytes().to_vec(),
        )
    }

    #[test]
    fn selectable_line_matches_save_text_and_skips_simulated_decoration() {
        let logs = vec![
            LogEntry::new_simulated(
                1,
                "14:26:07.285",
                LogDirection::Tx,
                "数据".as_bytes().to_vec(),
            ),
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
            text_line
                .timestamp_bytes
                .as_ref()
                .map(|range| &text_line.text[range.clone()]),
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
        assert_eq!(
            &hex_line.text[hex_line.payload_bytes.clone()],
            "E6 95 B0 E6 8D AE"
        );
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
                LogTextPoint {
                    line: 0,
                    char_index: data_char
                },
                LogTextPoint {
                    line: 0,
                    char_index: data_char + 1
                },
            )
            .as_deref(),
            Some("数")
        );

        let rx_at = lines[0].find("RX").unwrap();
        let rx_char = lines[0][..rx_at].chars().count();
        assert_eq!(
            selected_log_text(
                &lines,
                LogTextPoint {
                    line: 0,
                    char_index: rx_char
                },
                LogTextPoint {
                    line: 0,
                    char_index: rx_char + 2
                },
            )
            .as_deref(),
            Some("RX")
        );

        let timestamp_end = lines[0].find(']').unwrap() + 1;
        let timestamp_chars = lines[0][..timestamp_end].chars().count();
        assert_eq!(
            selected_log_text(
                &lines,
                LogTextPoint {
                    line: 0,
                    char_index: 0
                },
                LogTextPoint {
                    line: 0,
                    char_index: timestamp_chars
                },
            )
            .as_deref(),
            Some("[14:26:07.343]")
        );

        let hidden = selectable_log_line(&chinese_rx(), false, false);
        assert_eq!(
            selected_log_text(
                &[hidden.text.as_str()],
                LogTextPoint {
                    line: 0,
                    char_index: 0
                },
                LogTextPoint {
                    line: 0,
                    char_index: usize::MAX
                },
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
                LogTextPoint {
                    line: 0,
                    char_index: payload_char
                },
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
            LogTextPoint {
                line: 0,
                char_index: data_char,
            },
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
        assert!(
            model.sync_display(false, true, 2),
            "timestamp switch clears"
        );
        assert_eq!(model.clipboard_after_copy("KEEP"), "KEEP");

        model.set_selected_text("RX");
        assert!(
            !model.sync_display(false, true, 4),
            "append keeps selection"
        );
        assert_eq!(model.copy_payload().as_deref(), Some("RX"));
        model.observe_log_len(4);

        assert!(model.sync_display(false, true, 0), "clearing logs clears");
        assert!(model.copy_payload().is_none());
        assert_eq!(model.clipboard_after_copy("KEEP"), "KEEP");

        assert!(!model.sync_display(false, true, 0));
        assert_eq!(
            selected_log_text(
                &[] as &[&str],
                LogTextPoint {
                    line: 0,
                    char_index: 0
                },
                LogTextPoint {
                    line: 0,
                    char_index: 4
                },
            ),
            None
        );
        assert_eq!(copied_log_text(""), None);
        assert_eq!(clipboard_after_log_copy("KEEP", None), "KEEP");
        assert_eq!(clipboard_after_log_copy("KEEP", Some("")), "KEEP");
        assert_eq!(clipboard_after_log_copy("KEEP", Some("RX 数据")), "RX 数据");
    }

    #[test]
    fn formats_hex_input_with_auto_grouping_trailing_space_and_cursor() {
        // Empty and whitespace-only
        assert_eq!(format_hex_input_with_cursor("", 0), ("".into(), 0));
        assert_eq!(format_hex_input_with_cursor("   ", 2), ("".into(), 0));

        // Continuous typing / pasting auto-groups every 2 hex chars and uppercases
        assert_eq!(format_hex_input_with_cursor("415", 3), ("41 5".into(), 4));
        assert_eq!(format_hex_input_with_cursor("4154", 4), ("41 54".into(), 5));
        assert_eq!(
            format_hex_input_with_cursor("4Edddd", 6),
            ("4E DD DD".into(), 8)
        );

        // Trailing space preserved after an even-length group, dropped after odd group or backspace
        assert_eq!(format_hex_input_with_cursor("41 ", 3), ("41 ".into(), 3));
        assert_eq!(format_hex_input_with_cursor("41  ", 4), ("41 ".into(), 3));
        assert_eq!(format_hex_input_with_cursor("41 ", 2), ("41 ".into(), 2));
        assert_eq!(format_hex_input_with_cursor("41", 2), ("41".into(), 2));
        assert_eq!(format_hex_input_with_cursor("415 ", 4), ("41 5".into(), 4));

        // Invalid character at end keeps cursor at end (does not jump to 0)
        assert_eq!(
            format_hex_input_with_cursor("41 54z", 6),
            ("41 54".into(), 5)
        );
        assert_eq!(
            format_hex_input_with_cursor("41 54中文", "41 54中文".len()),
            ("41 54".into(), 5)
        );
        assert_eq!(format_hex_input_with_cursor("41 z", 4), ("41 ".into(), 3));

        // Middle insertion / deletion keeps cursor accurately positioned
        assert_eq!(
            format_hex_input_with_cursor("4z154", 2),
            ("41 54".into(), 1)
        );
        assert_eq!(
            format_hex_input_with_cursor("41A 54", 3),
            ("41 A5 4".into(), 4)
        );
        assert_eq!(format_hex_input_with_cursor("4154", 2), ("41 54".into(), 2));
    }

    #[test]
    fn current_timestamp_hms_ms_formats_hh_mm_ss_mmm() {
        let ts = current_timestamp_hms_ms();
        let bytes = ts.as_bytes();
        assert_eq!(bytes.len(), 12);
        assert_eq!(bytes[2], b':');
        assert_eq!(bytes[5], b':');
        assert_eq!(bytes[8], b'.');
        assert!(bytes[0..2].iter().all(u8::is_ascii_digit));
        assert!(bytes[3..5].iter().all(u8::is_ascii_digit));
        assert!(bytes[6..8].iter().all(u8::is_ascii_digit));
        assert!(bytes[9..12].iter().all(u8::is_ascii_digit));
    }

    #[test]
    fn search_matches_case_insensitively_and_navigates() {
        let logs = vec![
            LogEntry::new(1, "12:00:00.000", LogDirection::Tx, b"AT+CSQ".to_vec()),
            LogEntry::new(2, "12:00:00.100", LogDirection::Rx, b"+CSQ: 24,99".to_vec()),
            LogEntry::new(3, "12:00:00.200", LogDirection::Rx, b"AT+OK at".to_vec()),
        ];

        let mut search = LogSearchState {
            query: "at".to_string(),
            ..Default::default()
        };
        search.recompute(&logs, false, false, false);

        assert_eq!(search.matches.len(), 3);
        assert_eq!(search.active_match, Some(0));
        assert_eq!(search.count_label().as_deref(), Some("1/3"));
        assert_eq!(search.current().map(|m| m.log_index), Some(0));

        assert_eq!(search.next().map(|m| m.log_index), Some(2));
        assert_eq!(search.count_label().as_deref(), Some("2/3"));

        assert_eq!(search.next().map(|m| m.log_index), Some(2));
        assert_eq!(search.count_label().as_deref(), Some("3/3"));

        // Wrap around forward
        assert_eq!(search.next().map(|m| m.log_index), Some(0));
        assert_eq!(search.count_label().as_deref(), Some("1/3"));

        // Wrap around backward
        assert_eq!(search.prev().map(|m| m.log_index), Some(2));
        assert_eq!(search.count_label().as_deref(), Some("3/3"));
    }

    #[test]
    fn search_handles_empty_unicode_and_hex_modes() {
        let logs = vec![LogEntry::new(
            1,
            "12:00:00.000",
            LogDirection::Rx,
            "你好AT".as_bytes().to_vec(),
        )];

        let mut search = LogSearchState {
            query: "你好at".to_string(),
            ..Default::default()
        };
        search.recompute(&logs, false, false, false);
        assert_eq!(search.matches.len(), 1);
        let line = selectable_log_line(&logs[0], false, false);
        assert_eq!(&line.text[search.matches[0].byte_range.clone()], "你好AT");

        // In HEX mode, text query no longer matches UTF-8 literal, while hex bytes do.
        search.recompute(&logs, false, true, false);
        assert_eq!(search.matches.len(), 0);
        assert_eq!(search.count_label().as_deref(), Some("0/0"));

        search.query = "41 54".to_string();
        search.recompute(&logs, false, true, false);
        assert_eq!(search.matches.len(), 1);
    }
}
