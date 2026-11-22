//! Per-session history of sent drafts, recalled with Up/Down like a terminal.

const MAX_ENTRIES: usize = 100;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendHistory {
    entries: Vec<String>,
    /// Index of the entry being shown, or `None` while editing a fresh draft.
    pos: Option<usize>,
    /// The fresh draft saved when navigation began, restored by walking past the newest entry.
    stash: String,
}

impl SendHistory {
    /// Records a sent draft. Blank drafts and a repeat of the newest entry are
    /// ignored; navigation restarts from the fresh draft.
    pub fn push(&mut self, text: &str) {
        self.pos = None;
        self.stash.clear();
        if text.trim().is_empty() || self.entries.last().is_some_and(|last| last == text) {
            return;
        }
        self.entries.push(text.to_string());
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
    }

    /// Steps back to an older entry. `current` is the draft in the box, kept so
    /// [`Self::newer`] can restore it. Returns `None` when there is nothing older.
    pub fn older(&mut self, current: &str) -> Option<String> {
        let next = match self.pos {
            None => {
                let last = self.entries.len().checked_sub(1)?;
                self.stash = current.to_string();
                last
            }
            Some(0) => return None,
            Some(i) => i - 1,
        };
        self.pos = Some(next);
        Some(self.entries[next].clone())
    }

    /// Steps forward; past the newest entry it restores the draft saved by
    /// [`Self::older`]. Returns `None` when not navigating.
    pub fn newer(&mut self) -> Option<String> {
        let i = self.pos?;
        if i + 1 < self.entries.len() {
            self.pos = Some(i + 1);
            Some(self.entries[i + 1].clone())
        } else {
            self.pos = None;
            Some(std::mem::take(&mut self.stash))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(items: &[&str]) -> SendHistory {
        let mut h = SendHistory::default();
        for item in items {
            h.push(item);
        }
        h
    }

    #[test]
    fn walks_back_and_forward_and_restores_the_draft() {
        let mut h = history(&["AT", "AT+CSQ", "AT+VERSION"]);
        assert_eq!(h.older("half typed").as_deref(), Some("AT+VERSION"));
        assert_eq!(h.older("ignored").as_deref(), Some("AT+CSQ"));
        assert_eq!(h.older("ignored").as_deref(), Some("AT"));
        assert_eq!(h.older("ignored"), None, "stops at the oldest");
        assert_eq!(h.newer().as_deref(), Some("AT+CSQ"));
        assert_eq!(h.newer().as_deref(), Some("AT+VERSION"));
        assert_eq!(h.newer().as_deref(), Some("half typed"));
        assert_eq!(h.newer(), None, "not navigating any more");
    }

    #[test]
    fn empty_history_does_nothing() {
        let mut h = SendHistory::default();
        assert_eq!(h.older("x"), None);
        assert_eq!(h.newer(), None);
    }

    #[test]
    fn push_skips_blank_and_consecutive_duplicates_and_resets_navigation() {
        let mut h = history(&["AT", "AT", "  ", "", "AT+CSQ"]);
        assert_eq!(h.older("").as_deref(), Some("AT+CSQ"));
        h.push("AT+CSQ");
        assert_eq!(h.older("").as_deref(), Some("AT+CSQ"));
        assert_eq!(h.older("").as_deref(), Some("AT"));
        assert_eq!(h.older(""), None);
    }

    #[test]
    fn keeps_only_the_newest_entries() {
        let mut h = SendHistory::default();
        for i in 0..MAX_ENTRIES + 5 {
            h.push(&format!("cmd{i}"));
        }
        let mut count = 0;
        let mut last = String::new();
        while let Some(entry) = h.older("") {
            count += 1;
            last = entry;
        }
        assert_eq!(count, MAX_ENTRIES);
        assert_eq!(last, "cmd5");
    }
}
