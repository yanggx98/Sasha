use regex::{Regex, RegexBuilder};

use super::log::{LogDirection, LogEntry};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FilterDirection {
    #[default]
    All,
    Rx,
    Tx,
    /// Only the lines scripts printed with `log.*` / `print`.
    Script,
}

impl FilterDirection {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Rx,
            Self::Rx => Self::Tx,
            Self::Tx => Self::Script,
            Self::Script => Self::All,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogFilter {
    pub direction: FilterDirection,
    pub query: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub exclude: bool,
}

impl LogFilter {
    pub fn is_active(&self) -> bool {
        self.direction != FilterDirection::All || !self.query.is_empty()
    }

    fn compile(&self) -> Result<Option<Regex>, regex::Error> {
        if self.query.is_empty() {
            return Ok(None);
        }
        let pattern = if self.regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.case_sensitive)
            .build()
            .map(Some)
    }
}

/// Invalid drafts leave the complete last valid filter in effect, including
/// for new arrivals. The error itself is shown through localized UI text.
#[derive(Debug, Default)]
pub struct LogFilterState {
    pub open: bool,
    pub draft: LogFilter,
    pub invalid: bool,
    applied: LogFilter,
    matcher: Option<Regex>,
    revision: u64,
}

impl LogFilterState {
    pub fn apply(&mut self) {
        match self.draft.compile() {
            Ok(matcher) => {
                self.invalid = false;
                if self.applied != self.draft {
                    self.applied = self.draft.clone();
                    self.matcher = matcher;
                    self.revision += 1;
                }
            }
            Err(_) => self.invalid = true,
        }
    }

    /// The draft differs from the filter in effect (not applied yet, or invalid).
    pub fn has_pending(&self) -> bool {
        self.draft != self.applied
    }

    pub fn is_active(&self) -> bool {
        self.applied.is_active()
    }

    fn matches(&self, entry: &LogEntry, hex_mode: bool) -> bool {
        let wanted = self.applied.direction;
        let direction = match entry.direction {
            LogDirection::Rx => matches!(wanted, FilterDirection::All | FilterDirection::Rx),
            LogDirection::Tx => matches!(wanted, FilterDirection::All | FilterDirection::Tx),
            LogDirection::ScriptInfo
            | LogDirection::ScriptWarn
            | LogDirection::ScriptError
            | LogDirection::ScriptOutput => {
                matches!(wanted, FilterDirection::All | FilterDirection::Script)
            }
            // No system entries are currently emitted. Keep future status
            // messages visible until they have an explicit UI control.
            LogDirection::System => return true,
        };
        direction
            && self.matcher.as_ref().is_none_or(|matcher| {
                matcher.is_match(&entry.format_payload(hex_mode)) != self.applied.exclude
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleLogRow {
    pub source_index: usize,
    pub id: u64,
    pub byte_len: usize,
}

/// What a `LogProjection::sync` changed, relative to the rows before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionChange {
    /// Number of leading rows that are identical (id and byte length) to the
    /// rows before the sync.
    pub prefix: usize,
    /// A row that existed before is gone, or the rows were reordered.
    pub removed: bool,
}

#[derive(Debug, Default)]
pub struct LogProjection {
    pub rows: Vec<VisibleLogRow>,
    source_count: usize,
    head_id: Option<u64>,
    tail: Option<(u64, usize)>,
    hex_mode: bool,
    revision: u64,
}

impl LogProjection {
    fn needs_rebuild(&self, logs: &[LogEntry], filter: &LogFilterState, hex_mode: bool) -> bool {
        self.head_id != logs.first().map(|entry| entry.id)
            || logs.len() < self.source_count
            || self.hex_mode != hex_mode
            || self.revision != filter.revision
            || self.tail.is_some_and(|(id, _)| {
                logs.get(self.source_count.saturating_sub(1))
                    .is_none_or(|entry| entry.id != id)
            })
    }

    /// Cheap check: would `sync` leave the rows untouched?
    pub fn is_current(&self, logs: &[LogEntry], filter: &LogFilterState, hex_mode: bool) -> bool {
        !self.needs_rebuild(logs, filter, hex_mode)
            && logs.len() == self.source_count
            && logs.last().map(|entry| (entry.id, entry.bytes.len())) == self.tail
    }

    /// Only recheck the former tail and new entries during ordinary appends.
    /// A trim, replacement, mode change or accepted filter rebuilds the map.
    pub fn sync(
        &mut self,
        logs: &[LogEntry],
        filter: &LogFilterState,
        hex_mode: bool,
    ) -> ProjectionChange {
        if self.is_current(logs, filter, hex_mode) {
            return ProjectionChange {
                prefix: self.rows.len(),
                removed: false,
            };
        }
        let head_id = logs.first().map(|entry| entry.id);
        let tail = logs.last().map(|entry| (entry.id, entry.bytes.len()));
        let rebuild = self.needs_rebuild(logs, filter, hex_mode);
        // Rows from `start` on are recomputed. An append only re-evaluates the
        // former tail (at most one row) plus the new entries.
        let (start, kept, old_rows) = if rebuild {
            (0, 0, std::mem::take(&mut self.rows))
        } else {
            let start = self.source_count.saturating_sub(1);
            let kept = self.rows.partition_point(|row| row.source_index < start);
            (start, kept, self.rows.split_off(kept))
        };
        for (source_index, entry) in logs.iter().enumerate().skip(start) {
            if filter.matches(entry, hex_mode) {
                self.rows.push(VisibleLogRow {
                    source_index,
                    id: entry.id,
                    byte_len: entry.bytes.len(),
                });
            }
        }
        let same = |a: &VisibleLogRow, b: &VisibleLogRow| a.id == b.id && a.byte_len == b.byte_len;
        let prefix = kept
            + old_rows
                .iter()
                .zip(&self.rows[kept..])
                .take_while(|(a, b)| same(a, b))
                .count();
        let removed = old_rows
            .iter()
            .zip(&self.rows[kept..])
            .any(|(a, b)| a.id != b.id)
            || self.rows.len() < kept + old_rows.len();
        self.source_count = logs.len();
        self.head_id = head_id;
        self.tail = tail;
        self.hex_mode = hex_mode;
        self.revision = filter.revision;
        ProjectionChange { prefix, removed }
    }

    pub fn entries(&self, logs: &[LogEntry]) -> Vec<LogEntry> {
        self.rows
            .iter()
            .map(|row| logs[row.source_index].clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, direction: LogDirection, payload: &[u8]) -> LogEntry {
        LogEntry::new(id, "12:00:00.000", direction, payload)
    }

    #[test]
    fn combines_direction_literal_case_and_exclusion_on_payload_only() {
        let logs = vec![
            entry(1, LogDirection::Rx, b"HeartBeat"),
            entry(2, LogDirection::Tx, b"heartbeat"),
            entry(3, LogDirection::Rx, "温度: 25".as_bytes()),
        ];
        let mut filter = LogFilterState::default();
        filter.draft.query = "heartbeat".into();
        filter.apply();
        assert!(filter.matches(&logs[0], false));
        filter.draft.case_sensitive = true;
        filter.apply();
        assert!(!filter.matches(&logs[0], false));
        filter.draft.direction = FilterDirection::Rx;
        filter.draft.exclude = true;
        filter.apply();
        assert!(filter.matches(&logs[0], false));
        assert!(!filter.matches(&logs[1], false));
        filter.draft.query.clear();
        filter.apply();
        assert!(filter.matches(&logs[2], false));
        filter.draft.query = "RX".into();
        filter.draft.exclude = false;
        filter.apply();
        assert!(!filter.matches(&logs[0], false));
        filter.draft.query = "温度".into();
        filter.apply();
        assert!(filter.matches(&logs[2], false));
        filter.draft.query = "12:00".into();
        filter.apply();
        assert!(!filter.matches(&logs[2], false));
    }

    #[test]
    fn script_lines_follow_the_direction_filter() {
        let logs = [
            entry(1, LogDirection::Rx, b"a"),
            entry(2, LogDirection::Tx, b"b"),
            entry(3, LogDirection::ScriptInfo, b"c"),
            entry(4, LogDirection::ScriptWarn, b"d"),
            entry(5, LogDirection::ScriptError, b"e"),
            entry(6, LogDirection::ScriptOutput, b"f"),
        ];
        let visible = |filter: &LogFilterState| -> Vec<u64> {
            logs.iter()
                .filter(|log| filter.matches(log, false))
                .map(|log| log.id)
                .collect()
        };
        let mut filter = LogFilterState::default();
        assert_eq!(visible(&filter), vec![1, 2, 3, 4, 5, 6]);
        filter.draft.direction = FilterDirection::Rx;
        filter.apply();
        assert_eq!(visible(&filter), vec![1]);
        filter.draft.direction = FilterDirection::Tx;
        filter.apply();
        assert_eq!(visible(&filter), vec![2]);
        filter.draft.direction = FilterDirection::Script;
        filter.apply();
        assert_eq!(visible(&filter), vec![3, 4, 5, 6]);
        assert!(filter.is_active());
        filter.draft.query = "d".into();
        filter.apply();
        assert_eq!(visible(&filter), vec![4]);
        assert_eq!(FilterDirection::Tx.next(), FilterDirection::Script);
        assert_eq!(FilterDirection::Script.next(), FilterDirection::All);
    }

    #[test]
    fn regex_hex_and_invalid_drafts_keep_last_valid_filter() {
        let packet = entry(1, LogDirection::Rx, &[0xAA, 0x55]);
        let mut filter = LogFilterState::default();
        filter.draft.query = "aa (55|66)".into();
        filter.draft.regex = true;
        filter.apply();
        assert!(filter.matches(&packet, true));
        assert!(!filter.matches(&packet, false));
        filter.draft.query = "[".into();
        filter.draft.direction = FilterDirection::Tx;
        filter.apply();
        assert!(filter.invalid);
        assert!(filter.matches(&packet, true));
        filter.draft = LogFilter::default();
        filter.draft.query = "AA55".into();
        filter.apply();
        assert!(!filter.matches(&packet, true));
        filter.draft.query = ".*".into();
        filter.apply();
        assert!(!filter.matches(&packet, true)); // literal, not regex
    }

    #[test]
    fn tail_can_enter_and_leave_projection_and_trim_keeps_indices_valid() {
        let mut logs = vec![entry(1, LogDirection::Rx, b"he")];
        let mut filter = LogFilterState::default();
        filter.draft.query = "heart".into();
        filter.apply();
        let mut view = LogProjection::default();
        view.sync(&logs, &filter, false);
        assert!(view.rows.is_empty());
        logs[0].bytes.extend_from_slice(b"art");
        view.sync(&logs, &filter, false);
        assert_eq!(view.rows[0].id, 1);
        filter.draft.exclude = true;
        filter.apply();
        view.sync(&logs, &filter, false);
        assert!(view.rows.is_empty());
        logs.push(entry(2, LogDirection::Tx, b"next"));
        view.sync(&logs, &filter, false);
        assert_eq!(view.rows[0].source_index, 1);
        logs.remove(0);
        logs.push(entry(3, LogDirection::Rx, b"he"));
        view.sync(&logs, &filter, false);
        assert_eq!(view.rows[0].source_index, 0);
        logs[1].bytes.extend_from_slice(b"art");
        view.sync(&logs, &filter, false);
        assert_eq!(view.rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![2]);
        logs.clear();
        view.sync(&logs, &filter, false);
        assert!(view.entries(&logs).is_empty());
    }

    #[test]
    fn projection_search_and_export_share_visible_rows_in_order() {
        use super::super::log::{find_log_matches, format_visible_logs};
        let logs = vec![
            entry(1, LogDirection::Tx, b"OK"),
            entry(2, LogDirection::Rx, b"OK"),
        ];
        let mut filter = LogFilterState::default();
        filter.draft.direction = filter.draft.direction.next();
        filter.apply();
        let mut view = LogProjection::default();
        view.sync(&logs, &filter, false);
        let visible = view.entries(&logs);
        assert_eq!(
            find_log_matches(&visible, "OK", false, false)[0].log_index,
            0
        );
        assert_eq!(format_visible_logs(&visible, false, false), "RX OK");
        filter.draft.direction = filter.draft.direction.next();
        filter.apply();
        view.sync(&logs, &filter, false);
        assert_eq!(
            view.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![1]
        );
        filter.draft.direction = filter.draft.direction.next();
        filter.apply();
        view.sync(&logs, &filter, false);
        assert!(view.rows.is_empty(), "script filter hides serial rows");
        filter.draft.direction = filter.draft.direction.next();
        filter.apply();
        view.sync(&logs, &filter, false);
        assert_eq!(view.rows.len(), 2);
        assert!(!filter.is_active());
        filter.draft.query = "4F 4B".into();
        filter.apply();
        view.sync(&logs, &filter, false);
        assert!(view.rows.is_empty());
        view.sync(&logs, &filter, true);
        assert_eq!(view.rows.len(), 2);
    }

    #[test]
    fn streaming_updates_match_full_rebuild_through_filter_changes_and_trimming() {
        let mut filter = LogFilterState::default();
        let mut view = LogProjection::default();
        let mut logs = Vec::new();
        for id in 1..=80 {
            // Complete the former tail and append a new partial record in one
            // update, as the serial line framer can do between render frames.
            if let Some(last) = logs.last_mut() {
                let last: &mut LogEntry = last;
                last.bytes.extend_from_slice(b" OK\n");
            }
            logs.push(entry(
                id,
                if id % 3 == 0 {
                    LogDirection::Tx
                } else {
                    LogDirection::Rx
                },
                b"he",
            ));
            if id % 4 == 0 {
                logs.last_mut().unwrap().bytes.extend_from_slice(b"artbeat");
            }
            if logs.len() > 7 {
                logs.drain(..2);
            }
            if id % 5 == 0 {
                filter.draft.query = "heart|OK".into();
                filter.draft.regex = true;
                filter.draft.exclude = !filter.draft.exclude;
                filter.draft.direction = filter.draft.direction.next();
                filter.apply();
            }
            if id % 11 == 0 {
                filter.draft.query = "[".into();
                filter.apply();
            }
            let hex = id % 7 == 0;
            view.sync(&logs, &filter, hex);
            let mut rebuilt = LogProjection::default();
            rebuilt.sync(&logs, &filter, hex);
            assert_eq!(view.rows, rebuilt.rows, "update {id}");
        }
    }

    #[test]
    fn sync_reports_unchanged_prefix_and_removed_rows() {
        let mut logs = vec![
            entry(1, LogDirection::Rx, b"a1"),
            entry(2, LogDirection::Rx, b"b"),
        ];
        let mut filter = LogFilterState::default();
        filter.draft.query = "1|2".into();
        filter.draft.regex = true;
        filter.apply();
        let mut view = LogProjection::default();
        let change = view.sync(&logs, &filter, false);
        assert_eq!((change.prefix, change.removed), (0, false));
        assert!(view.is_current(&logs, &filter, false));
        // Nothing changed.
        let change = view.sync(&logs, &filter, false);
        assert_eq!((change.prefix, change.removed), (1, false));
        // A hidden entry arrives: visible rows keep their prefix.
        logs.push(entry(3, LogDirection::Rx, b"c"));
        let change = view.sync(&logs, &filter, false);
        assert_eq!((change.prefix, change.removed), (1, false));
        // The hidden tail grows into a match.
        logs[2].bytes.extend_from_slice(b"2");
        let change = view.sync(&logs, &filter, false);
        assert_eq!((change.prefix, change.removed), (1, false));
        assert_eq!(view.rows.len(), 2);
        // Trimming the front removes a visible row.
        logs.remove(0);
        let change = view.sync(&logs, &filter, false);
        assert!(change.removed);
        assert_eq!(change.prefix, 0);
    }

    #[test]
    fn drafts_stay_pending_until_applied() {
        let mut filter = LogFilterState::default();
        assert!(!filter.has_pending());
        filter.draft.query = "OK".into();
        assert!(filter.has_pending());
        assert!(!filter.is_active());
        filter.apply();
        assert!(!filter.has_pending());
        assert!(filter.is_active());
        filter.draft.query = "[".into();
        filter.draft.regex = true;
        filter.apply();
        assert!(filter.invalid && filter.has_pending());
    }
    #[test]
    fn filtering_isolated_from_pause_retention_counters_and_other_sessions() {
        use super::super::config::SerialConfig;
        use super::super::session::SessionState;
        let mut session = SessionState::new_real_session(SerialConfig::default(), true);
        let mut filter = LogFilterState::default();
        filter.draft.query = "OK".into();
        filter.apply();
        let other_filter = LogFilterState::default();
        session.append_rx_bytes(b"heartbeat\n", "12:00:00.000");
        session.toggle_receive_pause();
        session.append_rx_bytes(b"OK\n", "12:00:00.001");
        let count = session.counters.rx_bytes;
        let mut view = LogProjection::default();
        view.sync(&session.logs, &filter, false);
        assert!(view.rows.is_empty());
        assert_eq!(session.logs.len(), 1);
        session.toggle_receive_pause();
        view.sync(&session.logs, &filter, false);
        assert_eq!(view.entries(&session.logs)[0].format_payload(false), "OK");
        assert_eq!(session.counters.rx_bytes, count);
        let mut other_view = LogProjection::default();
        other_view.sync(&session.logs, &other_filter, false);
        assert_eq!(other_view.rows.len(), 2);
        session.clear_receive_logs();
        view.sync(&session.logs, &filter, false);
        assert!(view.rows.is_empty());
        assert!(filter.is_active());
        assert_eq!(session.counters.rx_bytes, count);
    }
}
