//! Text selection over log rows.
//!
//! A selection is two positions, each a log entry id plus a byte offset into
//! that row's displayed text. It says nothing about which rows are currently
//! rendered, so it survives scrolling a virtualized list any distance, and
//! the selected text is built from the log data rather than from the screen.
//! No UI code lives here.

use std::ops::Range;

/// A caret between two characters of one log row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogPos {
    pub entry_id: u64,
    /// Byte offset into the row's displayed text.
    pub offset: usize,
}

impl LogPos {
    pub fn new(entry_id: u64, offset: usize) -> Self {
        Self { entry_id, offset }
    }
}

/// Supplies a row's displayed text, or `None` when the row is unknown.
pub type RowText<'a> = &'a dyn Fn(u64) -> Option<String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Word,
    Line,
}

/// The word or line a double or triple click selected. A drag from that click
/// keeps extending by the same unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HeldUnit {
    kind: UnitKind,
    start: LogPos,
    end: LogPos,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogSelection {
    anchor: Option<LogPos>,
    cursor: Option<LogPos>,
    unit: Option<HeldUnit>,
    dragging: bool,
}

impl LogSelection {
    /// Starts a gesture at `pos`: a click when `click_count` is 1 (extending the
    /// current selection when `extend` and one exists), a word at 2, a whole
    /// row from 3.
    pub fn begin(&mut self, pos: LogPos, click_count: usize, extend: bool, text_of: RowText) {
        self.dragging = true;
        self.unit = None;
        match click_count {
            0 | 1 => self.place(pos, extend),
            2 => {
                let word = text_of(pos.entry_id).and_then(|text| word_range_at(&text, pos.offset));
                match word {
                    Some(range) => self.hold(UnitKind::Word, pos.entry_id, range),
                    None => self.place(pos, false),
                }
            }
            _ => match text_of(pos.entry_id) {
                Some(text) => self.hold(UnitKind::Line, pos.entry_id, 0..text.len()),
                None => self.place(pos, false),
            },
        }
    }

    fn place(&mut self, pos: LogPos, extend: bool) {
        match self.anchor {
            Some(_) if extend => self.cursor = Some(pos),
            _ => {
                self.anchor = Some(pos);
                self.cursor = Some(pos);
            }
        }
    }

    fn hold(&mut self, kind: UnitKind, entry_id: u64, range: Range<usize>) {
        let start = LogPos::new(entry_id, range.start);
        let end = LogPos::new(entry_id, range.end);
        self.anchor = Some(start);
        self.cursor = Some(end);
        self.unit = Some(HeldUnit { kind, start, end });
    }

    /// Moves the free end to `pos` while a gesture is in progress.
    pub fn drag_to(&mut self, pos: LogPos, text_of: RowText) {
        if !self.dragging {
            return;
        }
        let Some(unit) = self.unit else {
            self.cursor = Some(pos);
            return;
        };
        let Some(text) = text_of(pos.entry_id) else {
            self.cursor = Some(pos);
            return;
        };
        let at = match unit.kind {
            UnitKind::Word => word_range_at(&text, pos.offset).unwrap_or(pos.offset..pos.offset),
            UnitKind::Line => 0..text.len(),
        };
        if pos < unit.start {
            self.anchor = Some(unit.end);
            self.cursor = Some(LogPos::new(pos.entry_id, at.start));
        } else {
            self.anchor = Some(unit.start);
            self.cursor = Some(LogPos::new(pos.entry_id, at.end));
        }
    }

    /// Ends the gesture. A plain click that selected nothing leaves no selection.
    pub fn end(&mut self) {
        self.dragging = false;
        self.unit = None;
        if self.anchor == self.cursor {
            self.clear();
        }
    }

    pub fn clear(&mut self) {
        self.anchor = None;
        self.cursor = None;
        self.unit = None;
        self.dragging = false;
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn has_selection(&self) -> bool {
        self.range().is_some()
    }

    /// The selection as `(start, end)` in document order, or `None` when empty.
    pub fn range(&self) -> Option<(LogPos, LogPos)> {
        let (a, c) = (self.anchor?, self.cursor?);
        if a == c {
            None
        } else if a < c {
            Some((a, c))
        } else {
            Some((c, a))
        }
    }

    /// First and last entry id the selection touches.
    pub fn id_span(&self) -> Option<(u64, u64)> {
        self.range()
            .map(|(start, end)| (start.entry_id, end.entry_id))
    }

    /// Byte range of the selection inside the row `entry_id` whose text is
    /// `len` bytes long, or `None` when the row is outside it.
    pub fn row_range(&self, entry_id: u64, len: usize) -> Option<Range<usize>> {
        let (start, end) = self.range()?;
        if entry_id < start.entry_id || entry_id > end.entry_id {
            return None;
        }
        let from = if entry_id == start.entry_id {
            start.offset.min(len)
        } else {
            0
        };
        let to = if entry_id == end.entry_id {
            end.offset.min(len)
        } else {
            len
        };
        (from < to).then_some(from..to)
    }

    /// The selected text. `rows` are the rows in display order, at least those
    /// between the selection's ends; others are ignored.
    pub fn selected_text(&self, rows: impl IntoIterator<Item = (u64, String)>) -> String {
        let mut parts = Vec::new();
        for (entry_id, text) in rows {
            if let Some(range) = self.row_range(entry_id, text.len()) {
                parts.push(slice_chars(&text, range));
            } else if self
                .range()
                .is_some_and(|(s, e)| entry_id >= s.entry_id && entry_id <= e.entry_id)
            {
                // A row inside the span that contributes nothing still
                // occupies a line, so empty entries are not silently dropped.
                parts.push(String::new());
            }
        }
        parts.join("\n")
    }

    /// Selects everything from the start of `first` to the end of `last`.
    pub fn select_all(&mut self, first: LogPos, last: LogPos) {
        self.dragging = false;
        self.unit = None;
        self.anchor = Some(first);
        self.cursor = Some(last);
    }

    /// Entries older than `first_id` were dropped from the log. Clears the
    /// selection if it lay wholly in them, otherwise pulls its start in.
    pub fn retain_from(&mut self, first_id: u64) {
        let Some((_, end)) = self.range() else {
            return;
        };
        if end.entry_id < first_id {
            self.clear();
            return;
        }
        let floor = LogPos::new(first_id, 0);
        for pos in [&mut self.anchor, &mut self.cursor].into_iter().flatten() {
            if *pos < floor {
                *pos = floor;
            }
        }
        if let Some(unit) = &mut self.unit {
            unit.start = unit.start.max(floor);
            unit.end = unit.end.max(floor);
        }
    }
}

/// Slices `text` by byte range, snapping to character boundaries.
fn slice_chars(text: &str, range: Range<usize>) -> String {
    let mut start = range.start.min(text.len());
    let mut end = range.end.min(text.len());
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    text[start..end].to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Space,
    Other,
}

fn class_of(c: char) -> CharClass {
    if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else if c.is_whitespace() {
        CharClass::Space
    } else {
        CharClass::Other
    }
}

/// The run of same-class characters around the byte `offset` (letters, digits
/// and `_`; whitespace; or other punctuation). An offset at or past the end
/// refers to the last character; an empty text has no word.
pub fn word_range_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let last = text.char_indices().next_back()?.0;
    let mut at = offset.min(last);
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let class = class_of(text[at..].chars().next()?);
    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| class_of(*c) == class)
        .last()
        .map_or(at, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .find(|(_, c)| class_of(*c) != class)
        .map_or(text.len(), |(i, _)| at + i);
    Some(start..end)
}

#[cfg(test)]
mod tests {
    use super::{LogPos, LogSelection, word_range_at};

    fn p(entry_id: u64, offset: usize) -> LogPos {
        LogPos::new(entry_id, offset)
    }

    /// Rows 1..=5 hold "row N: alpha_beta gamma".
    fn text_of(id: u64) -> Option<String> {
        (1..=5)
            .contains(&id)
            .then(|| format!("row {id}: alpha_beta gamma"))
    }

    fn rows() -> Vec<(u64, String)> {
        (1..=5).map(|id| (id, text_of(id).unwrap())).collect()
    }

    fn click(sel: &mut LogSelection, pos: LogPos) {
        sel.begin(pos, 1, false, &text_of);
    }

    #[test]
    fn a_plain_click_selects_nothing() {
        let mut sel = LogSelection::default();
        click(&mut sel, p(2, 4));
        assert!(sel.is_dragging());
        sel.end();
        assert!(!sel.has_selection());
        assert_eq!(sel.range(), None);
    }

    #[test]
    fn a_click_clears_an_existing_selection() {
        let mut sel = LogSelection::default();
        click(&mut sel, p(1, 0));
        sel.drag_to(p(3, 3), &text_of);
        sel.end();
        assert!(sel.has_selection());
        click(&mut sel, p(4, 2));
        sel.end();
        assert!(!sel.has_selection());
    }

    #[test]
    fn dragging_across_rows_selects_the_text_between() {
        let mut sel = LogSelection::default();
        click(&mut sel, p(1, 4));
        sel.drag_to(p(3, 5), &text_of);
        sel.end();
        assert_eq!(sel.range(), Some((p(1, 4), p(3, 5))));
        assert_eq!(sel.row_range(1, 23), Some(4..23));
        assert_eq!(sel.row_range(2, 23), Some(0..23));
        assert_eq!(sel.row_range(3, 23), Some(0..5));
        assert_eq!(sel.row_range(4, 23), None);
        assert_eq!(sel.row_range(0, 23), None);
        assert_eq!(
            sel.selected_text(rows()),
            "1: alpha_beta gamma\nrow 2: alpha_beta gamma\nrow 3"
        );
    }

    #[test]
    fn dragging_upward_gives_the_same_selection() {
        let mut down = LogSelection::default();
        click(&mut down, p(1, 4));
        down.drag_to(p(3, 5), &text_of);
        let mut up = LogSelection::default();
        click(&mut up, p(3, 5));
        up.drag_to(p(1, 4), &text_of);
        assert_eq!(down.range(), up.range());
        assert_eq!(down.selected_text(rows()), up.selected_text(rows()));
    }

    #[test]
    fn selection_within_one_row() {
        let mut sel = LogSelection::default();
        click(&mut sel, p(2, 7));
        sel.drag_to(p(2, 12), &text_of);
        assert_eq!(sel.selected_text(rows()), "alpha");
        assert_eq!(sel.row_range(2, 23), Some(7..12));
    }

    #[test]
    fn moves_are_ignored_after_the_gesture_ends() {
        let mut sel = LogSelection::default();
        click(&mut sel, p(1, 0));
        sel.drag_to(p(2, 5), &text_of);
        sel.end();
        sel.drag_to(p(5, 9), &text_of);
        assert_eq!(sel.range(), Some((p(1, 0), p(2, 5))));
    }

    #[test]
    fn shift_click_extends_from_the_anchor() {
        let mut sel = LogSelection::default();
        click(&mut sel, p(1, 4));
        sel.drag_to(p(1, 9), &text_of);
        sel.end();
        sel.begin(p(3, 6), 1, true, &text_of);
        sel.end();
        assert_eq!(sel.range(), Some((p(1, 4), p(3, 6))));
        // Shift with nothing selected is just a click.
        let mut empty = LogSelection::default();
        empty.begin(p(2, 3), 1, true, &text_of);
        empty.end();
        assert!(!empty.has_selection());
    }

    #[test]
    fn double_click_selects_a_word_and_keeps_it_after_release() {
        let mut sel = LogSelection::default();
        sel.begin(p(2, 9), 2, false, &text_of);
        sel.end();
        assert_eq!(sel.selected_text(rows()), "alpha_beta");
    }

    #[test]
    fn dragging_after_a_double_click_extends_by_words() {
        let mut sel = LogSelection::default();
        // "row 2: alpha_beta gamma": alpha_beta is 7..17, gamma is 18..23.
        sel.begin(p(2, 9), 2, false, &text_of);
        sel.drag_to(p(2, 20), &text_of);
        assert_eq!(sel.selected_text(rows()), "alpha_beta gamma");
        sel.drag_to(p(2, 13), &text_of);
        assert_eq!(sel.selected_text(rows()), "alpha_beta");
        // Backward past the word's start picks up the word before, anchor flips.
        sel.drag_to(p(2, 1), &text_of);
        assert_eq!(sel.selected_text(rows()), "row 2: alpha_beta");
        // Onto a later row: the word's start stays anchored, the end snaps to a word end.
        sel.drag_to(p(3, 2), &text_of);
        assert_eq!(sel.range(), Some((p(2, 7), p(3, 3))));
        // Onto an earlier row: the anchor flips to the word's end.
        sel.drag_to(p(1, 2), &text_of);
        assert_eq!(sel.range(), Some((p(1, 0), p(2, 17))));
    }

    #[test]
    fn triple_click_selects_the_row_and_drag_extends_by_rows() {
        let mut sel = LogSelection::default();
        sel.begin(p(2, 5), 3, false, &text_of);
        sel.end();
        assert_eq!(sel.selected_text(rows()), "row 2: alpha_beta gamma");
        let mut drag = LogSelection::default();
        drag.begin(p(2, 5), 3, false, &text_of);
        drag.drag_to(p(4, 1), &text_of);
        assert_eq!(drag.range(), Some((p(2, 0), p(4, 23))));
        drag.drag_to(p(1, 8), &text_of);
        assert_eq!(drag.range(), Some((p(1, 0), p(2, 23))));
    }

    #[test]
    fn clicks_past_three_still_select_the_row() {
        let mut sel = LogSelection::default();
        sel.begin(p(4, 1), 5, false, &text_of);
        sel.end();
        assert_eq!(sel.selected_text(rows()), "row 4: alpha_beta gamma");
    }

    #[test]
    fn multi_click_on_an_unknown_row_falls_back_to_a_click() {
        let mut sel = LogSelection::default();
        sel.begin(p(99, 3), 2, false, &text_of);
        sel.end();
        assert!(!sel.has_selection());
    }

    #[test]
    fn row_range_clamps_to_the_row_length() {
        let mut sel = LogSelection::default();
        click(&mut sel, p(1, 20));
        sel.drag_to(p(2, 500), &text_of);
        assert_eq!(sel.row_range(2, 23), Some(0..23));
        // A start past the end of its own row contributes nothing there.
        let mut late = LogSelection::default();
        click(&mut late, p(1, 500));
        late.drag_to(p(2, 3), &text_of);
        assert_eq!(late.row_range(1, 23), None);
        assert_eq!(late.row_range(2, 23), Some(0..3));
    }

    #[test]
    fn selected_text_ignores_rows_outside_and_survives_non_ascii() {
        let mut sel = LogSelection::default();
        let rows = vec![
            (1, "温度 23.5".to_string()),
            (2, "湿度 41".to_string()),
            (3, "after".to_string()),
        ];
        // Offsets inside a multi-byte character snap outward.
        sel.select_all(p(1, 1), p(2, 4));
        assert_eq!(sel.selected_text(rows.clone()), "温度 23.5\n湿度");
        assert_eq!(sel.selected_text(vec![(3, "after".to_string())]), "");
    }

    #[test]
    fn empty_rows_inside_the_span_keep_their_line() {
        let mut sel = LogSelection::default();
        sel.select_all(p(1, 0), p(3, 1));
        let rows = vec![
            (1, "a".to_string()),
            (2, String::new()),
            (3, "b".to_string()),
        ];
        assert_eq!(sel.selected_text(rows), "a\n\nb");
    }

    #[test]
    fn retain_from_pulls_in_or_clears() {
        let mut sel = LogSelection::default();
        sel.select_all(p(2, 5), p(5, 3));
        sel.retain_from(3);
        assert_eq!(sel.range(), Some((p(3, 0), p(5, 3))));
        sel.retain_from(6);
        assert!(!sel.has_selection());
        // Nothing lost: unchanged.
        let mut keep = LogSelection::default();
        keep.select_all(p(4, 2), p(5, 3));
        keep.retain_from(3);
        assert_eq!(keep.range(), Some((p(4, 2), p(5, 3))));
    }

    #[test]
    fn clear_forgets_everything() {
        let mut sel = LogSelection::default();
        click(&mut sel, p(1, 0));
        sel.drag_to(p(2, 3), &text_of);
        sel.clear();
        assert!(!sel.has_selection());
        assert!(!sel.is_dragging());
        assert_eq!(sel.id_span(), None);
    }

    #[test]
    fn word_ranges_follow_character_classes() {
        let text = "ab_9 .,x  y";
        assert_eq!(word_range_at(text, 1), Some(0..4));
        assert_eq!(word_range_at(text, 4), Some(4..5)); // the space
        assert_eq!(word_range_at(text, 5), Some(5..7)); // ".,"
        assert_eq!(word_range_at(text, 7), Some(7..8));
        assert_eq!(word_range_at(text, 8), Some(8..10)); // two spaces
        assert_eq!(word_range_at(text, 999), Some(10..11)); // past the end: last char
        assert_eq!(word_range_at("", 0), None);
        // Multi-byte: an offset inside a character lands on that character.
        assert_eq!(word_range_at("温度 x", 1), Some(0..6));
    }
}
