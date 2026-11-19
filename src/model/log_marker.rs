//! Markers a user puts on received log entries. Pure data, no UI code.

use std::collections::BTreeMap;

/// Longest marker name, in characters.
pub const MARKER_NAME_MAX_CHARS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerColor {
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
    Pink,
}

impl MarkerColor {
    pub const ALL: [MarkerColor; 8] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Cyan,
        Self::Blue,
        Self::Purple,
        Self::Pink,
    ];

    /// `0xRRGGBB`.
    pub fn rgb(self) -> u32 {
        match self {
            Self::Red => 0xE5484D,
            Self::Orange => 0xF76B15,
            Self::Yellow => 0xFFC53D,
            Self::Green => 0x30A46C,
            Self::Cyan => 0x00A2C7,
            Self::Blue => 0x3E63DD,
            Self::Purple => 0x8E4EC6,
            Self::Pink => 0xE93D82,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogMarker {
    pub name: String,
    pub color: MarkerColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerError {
    EmptyName,
    NameTooLong,
}

impl MarkerError {
    /// Locale key of the message shown to the user.
    pub fn locale_key(self) -> &'static str {
        match self {
            Self::EmptyName => "marker.err_empty_name",
            Self::NameTooLong => "marker.err_name_long",
        }
    }
}

/// Markers keyed by `LogEntry.id`, in id order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogMarkers {
    markers: BTreeMap<u64, LogMarker>,
}

impl LogMarkers {
    /// Marks an entry, replacing any earlier marker on it. The name is trimmed.
    pub fn set(
        &mut self,
        entry_id: u64,
        name: &str,
        color: MarkerColor,
    ) -> Result<(), MarkerError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(MarkerError::EmptyName);
        }
        if name.chars().count() > MARKER_NAME_MAX_CHARS {
            return Err(MarkerError::NameTooLong);
        }
        self.markers.insert(
            entry_id,
            LogMarker {
                name: name.to_string(),
                color,
            },
        );
        Ok(())
    }

    pub fn remove(&mut self, entry_id: u64) -> bool {
        self.markers.remove(&entry_id).is_some()
    }

    pub fn get(&self, entry_id: u64) -> Option<&LogMarker> {
        self.markers.get(&entry_id)
    }

    pub fn clear(&mut self) {
        self.markers.clear();
    }

    pub fn len(&self) -> usize {
        self.markers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.markers.is_empty()
    }

    /// Drops markers whose entry has been evicted: every id below `first_live_id`, or all of
    /// them when the log is empty (`None`).
    pub fn prune_before(&mut self, first_live_id: Option<u64>) {
        match first_live_id {
            Some(first) => self.markers = self.markers.split_off(&first),
            None => self.markers.clear(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (u64, &LogMarker)> {
        self.markers.iter().map(|(id, marker)| (*id, marker))
    }
}

/// `(row index, entry id)` for every marker whose entry is among `row_ids`, the ids of the
/// currently visible rows in ascending order. Markers on filtered-out entries are skipped.
pub fn visible_marker_positions(row_ids: &[u64], markers: &LogMarkers) -> Vec<(usize, u64)> {
    markers
        .iter()
        .filter_map(|(id, _)| row_ids.binary_search(&id).ok().map(|index| (index, id)))
        .collect()
}

/// Relative position `0.0..=1.0` of row `index` among `total` rows, for the mini map.
pub fn minimap_fraction(index: usize, total: usize) -> f32 {
    if total == 0 {
        return 0.0;
    }
    ((index as f32 + 0.5) / total as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
