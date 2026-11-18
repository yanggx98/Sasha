use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::log::{SendPayloadError, parse_hex_bytes};
use super::session::SendLineEnding;

pub const MAX_QUICK_COMMANDS: usize = 200;
pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_GROUP_CHARS: usize = 64;
pub const MAX_CONTENT_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickCommand {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
    pub content: String,
    #[serde(default)]
    pub hex: bool,
    #[serde(default)]
    pub line_ending: SendLineEnding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickCommandError {
    EmptyName,
    NameTooLong,
    GroupTooLong,
    EmptyContent,
    ContentTooLong,
    InvalidHex,
    TooMany,
}

impl QuickCommand {
    pub fn validate(&self) -> Result<(), QuickCommandError> {
        if self.name.trim().is_empty() {
            return Err(QuickCommandError::EmptyName);
        }
        if self.name.chars().count() > MAX_NAME_CHARS {
            return Err(QuickCommandError::NameTooLong);
        }
        if self.group.chars().count() > MAX_GROUP_CHARS {
            return Err(QuickCommandError::GroupTooLong);
        }
        if self.content.is_empty() {
            return Err(QuickCommandError::EmptyContent);
        }
        if self.content.len() > MAX_CONTENT_BYTES {
            return Err(QuickCommandError::ContentTooLong);
        }
        if self.hex {
            match parse_hex_bytes(&self.content) {
                Ok(_) => {}
                Err(SendPayloadError::EmptyInput) => return Err(QuickCommandError::EmptyContent),
                Err(SendPayloadError::InvalidHex(_)) => return Err(QuickCommandError::InvalidHex),
                Err(SendPayloadError::Disconnected) => {}
            }
        }
        Ok(())
    }

    pub fn payload(&self) -> Result<Vec<u8>, SendPayloadError> {
        if self.hex {
            return parse_hex_bytes(&self.content);
        }
        if self.content.is_empty() {
            return Err(SendPayloadError::EmptyInput);
        }
        let mut bytes = self.content.as_bytes().to_vec();
        bytes.extend_from_slice(self.line_ending.suffix_bytes());
        Ok(bytes)
    }

    pub fn preview(&self) -> String {
        if self.hex {
            return self.content.clone();
        }
        match self.line_ending {
            SendLineEnding::None => self.content.clone(),
            SendLineEnding::CrLf => format!("{}\\r\\n", self.content),
            SendLineEnding::Lf => format!("{}\\n", self.content),
        }
    }

    pub fn matches_query(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        self.name.to_lowercase().contains(&q)
            || self.group.to_lowercase().contains(&q)
            || self.content.to_lowercase().contains(&q)
    }
}

pub fn next_command_id(existing: &[QuickCommand]) -> String {
    let mut seq = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    loop {
        let candidate = format!("qc-{seq:x}");
        if !existing.iter().any(|cmd| cmd.id == candidate) {
            return candidate;
        }
        seq = seq.wrapping_add(1);
    }
}

pub fn display_order(commands: &[QuickCommand]) -> Vec<usize> {
    let mut groups: Vec<&str> = Vec::new();
    for cmd in commands {
        if !groups.contains(&cmd.group.as_str()) {
            groups.push(cmd.group.as_str());
        }
    }
    let mut order = Vec::with_capacity(commands.len());
    for group in groups {
        for (idx, cmd) in commands.iter().enumerate() {
            if cmd.group == group {
                order.push(idx);
            }
        }
    }
    order
}

pub fn command_for_shortcut(commands: &[QuickCommand], digit: usize) -> Option<&QuickCommand> {
    if !(1..=9).contains(&digit) {
        return None;
    }
    let order = display_order(commands);
    order.get(digit - 1).and_then(|&idx| commands.get(idx))
}

pub fn quick_shortcut_digit(
    control: bool,
    alt: bool,
    shift: bool,
    platform: bool,
    key: &str,
) -> Option<usize> {
    if !alt || control || shift || platform {
        return None;
    }
    match key {
        "1" => Some(1),
        "2" => Some(2),
        "3" => Some(3),
        "4" => Some(4),
        "5" => Some(5),
        "6" => Some(6),
        "7" => Some(7),
        "8" => Some(8),
        "9" => Some(9),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickRow {
    Group {
        name: String,
        count: usize,
        collapsed: bool,
    },
    Command {
        index: usize,
        shortcut: Option<usize>,
    },
}

pub fn visible_rows(
    commands: &[QuickCommand],
    query: &str,
    collapsed_groups: &HashSet<String>,
) -> Vec<QuickRow> {
    let order = display_order(commands);
    let mut shortcut_by_index = vec![None; commands.len()];
    for (pos, &idx) in order.iter().take(9).enumerate() {
        shortcut_by_index[idx] = Some(pos + 1);
    }

    if !query.trim().is_empty() {
        let mut rows = Vec::new();
        for &idx in &order {
            if commands[idx].matches_query(query) {
                rows.push(QuickRow::Command {
                    index: idx,
                    shortcut: shortcut_by_index[idx],
                });
            }
        }
        return rows;
    }

    let mut rows = Vec::new();
    let mut i = 0;
    while i < order.len() {
        let group_name = &commands[order[i]].group;
        let mut end = i + 1;
        while end < order.len() && &commands[order[end]].group == group_name {
            end += 1;
        }
        let count = end - i;
        let collapsed = collapsed_groups.contains(group_name);
        rows.push(QuickRow::Group {
            name: group_name.clone(),
            count,
            collapsed,
        });
        if !collapsed {
            for &idx in &order[i..end] {
                rows.push(QuickRow::Command {
                    index: idx,
                    shortcut: shortcut_by_index[idx],
                });
            }
        }
        i = end;
    }
    rows
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickEditorDraft {
    pub editing_id: Option<String>,
    pub name: String,
    pub group: String,
    pub content: String,
    pub hex: bool,
    pub line_ending: SendLineEnding,
}

impl QuickEditorDraft {
    pub fn blank() -> Self {
        Self {
            editing_id: None,
            name: String::new(),
            group: String::new(),
            content: String::new(),
            hex: false,
            line_ending: SendLineEnding::None,
        }
    }

    pub fn from_command(cmd: &QuickCommand) -> Self {
        Self {
            editing_id: Some(cmd.id.clone()),
            name: cmd.name.clone(),
            group: cmd.group.clone(),
            content: cmd.content.clone(),
            hex: cmd.hex,
            line_ending: cmd.line_ending,
        }
    }

    pub fn to_command(&self, existing: &[QuickCommand]) -> Result<QuickCommand, QuickCommandError> {
        let id = self
            .editing_id
            .clone()
            .unwrap_or_else(|| next_command_id(existing));
        let cmd = QuickCommand {
            id,
            name: self.name.trim().to_string(),
            group: self.group.trim().to_string(),
            content: self.content.clone(),
            hex: self.hex,
            line_ending: self.line_ending,
        };
        cmd.validate()?;
        Ok(cmd)
    }
}

pub fn upsert_command(
    commands: &mut Vec<QuickCommand>,
    cmd: QuickCommand,
) -> Result<(), QuickCommandError> {
    if let Some(pos) = commands.iter().position(|c| c.id == cmd.id) {
        commands[pos] = cmd;
        return Ok(());
    }
    if commands.len() >= MAX_QUICK_COMMANDS {
        return Err(QuickCommandError::TooMany);
    }
    commands.push(cmd);
    Ok(())
}

pub fn remove_command(commands: &mut Vec<QuickCommand>, id: &str) -> bool {
    if let Some(pos) = commands.iter().position(|c| c.id == id) {
        commands.remove(pos);
        true
    } else {
        false
    }
}

pub fn move_command(commands: &mut [QuickCommand], id: &str, delta: i32) -> bool {
    if delta == 0 {
        return false;
    }
    let Some(pos) = commands.iter().position(|c| c.id == id) else {
        return false;
    };
    let target = if delta < 0 {
        (0..pos)
            .rev()
            .find(|&i| commands[i].group == commands[pos].group)
    } else {
        ((pos + 1)..commands.len()).find(|&i| commands[i].group == commands[pos].group)
    };
    if let Some(other) = target {
        commands.swap(pos, other);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_cmd(id: &str, name: &str, group: &str, content: &str) -> QuickCommand {
        QuickCommand {
            id: id.to_string(),
            name: name.to_string(),
            group: group.to_string(),
            content: content.to_string(),
            hex: false,
            line_ending: SendLineEnding::None,
        }
    }

    #[test]
    fn payload_text_appends_line_ending() {
        let mut cmd = make_cmd("c1", "AT", "", "AT");
        cmd.line_ending = SendLineEnding::CrLf;
        assert_eq!(cmd.payload(), Ok(b"AT\r\n".to_vec()));
        cmd.line_ending = SendLineEnding::Lf;
        assert_eq!(cmd.payload(), Ok(b"AT\n".to_vec()));
        cmd.line_ending = SendLineEnding::None;
        assert_eq!(cmd.payload(), Ok(b"AT".to_vec()));
    }

    #[test]
    fn payload_text_is_utf8() {
        let cmd = make_cmd("c1", "cn", "", "你好");
        assert_eq!(cmd.payload(), Ok("你好".as_bytes().to_vec()));
    }

    #[test]
    fn payload_hex_ignores_line_ending() {
        let cmd = QuickCommand {
            id: "c1".to_string(),
            name: "hex".to_string(),
            group: String::new(),
            content: "41 54".to_string(),
            hex: true,
            line_ending: SendLineEnding::CrLf,
        };
        assert_eq!(cmd.payload(), Ok(vec![0x41, 0x54]));
    }

    #[test]
    fn payload_hex_invalid_and_empty() {
        let mut cmd = QuickCommand {
            id: "c1".to_string(),
            name: "hex".to_string(),
            group: String::new(),
            content: "4142".to_string(),
            hex: true,
            line_ending: SendLineEnding::None,
        };
        assert!(matches!(cmd.payload(), Err(SendPayloadError::InvalidHex(_))));

        cmd.content = "  ".to_string();
        assert_eq!(cmd.payload(), Err(SendPayloadError::EmptyInput));

        cmd.hex = false;
        cmd.content = String::new();
        assert_eq!(cmd.payload(), Err(SendPayloadError::EmptyInput));
    }

    #[test]
    fn validate_enforces_limits() {
        let base = make_cmd("c1", "AT", "", "AT");
        assert_eq!(base.validate(), Ok(()));

        let mut empty_name = base.clone();
        empty_name.name = "   ".to_string();
        assert_eq!(empty_name.validate(), Err(QuickCommandError::EmptyName));

        let mut long_name = base.clone();
        long_name.name = "a".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(long_name.validate(), Err(QuickCommandError::NameTooLong));

        let mut long_group = base.clone();
        long_group.group = "g".repeat(MAX_GROUP_CHARS + 1);
        assert_eq!(long_group.validate(), Err(QuickCommandError::GroupTooLong));

        let mut empty_content = base.clone();
        empty_content.content = String::new();
        assert_eq!(
            empty_content.validate(),
            Err(QuickCommandError::EmptyContent)
        );

        let mut long_content = base.clone();
        long_content.content = "x".repeat(MAX_CONTENT_BYTES + 1);
        assert_eq!(
            long_content.validate(),
            Err(QuickCommandError::ContentTooLong)
        );

        let mut bad_hex = base;
        bad_hex.hex = true;
        bad_hex.content = "ZZ".to_string();
        assert_eq!(bad_hex.validate(), Err(QuickCommandError::InvalidHex));
    }

    #[test]
    fn display_order_groups_by_first_appearance() {
        let cmds = vec![
            make_cmd("1", "a", "g1", "a"),
            make_cmd("2", "b", "g2", "b"),
            make_cmd("3", "c", "g1", "c"),
            make_cmd("4", "d", "", "d"),
        ];
        assert_eq!(display_order(&cmds), vec![0, 2, 1, 3]);
    }

    #[test]
    fn shortcut_follows_display_order_not_filter() {
        let cmds: Vec<QuickCommand> = (1..=10)
            .map(|i| make_cmd(&format!("c{i}"), &format!("cmd{i}"), "", "x"))
            .collect();
        assert_eq!(
            command_for_shortcut(&cmds, 9).map(|c| c.id.as_str()),
            Some("c9")
        );
        assert_eq!(command_for_shortcut(&cmds, 10), None);
        assert_eq!(command_for_shortcut(&cmds, 0), None);

        let collapsed = HashSet::new();
        let rows_10 = visible_rows(&cmds, "cmd10", &collapsed);
        assert_eq!(
            rows_10,
            vec![QuickRow::Command {
                index: 9,
                shortcut: None
            }]
        );

        let rows_2 = visible_rows(&cmds, "cmd2", &collapsed);
        assert_eq!(
            rows_2,
            vec![QuickRow::Command {
                index: 1,
                shortcut: Some(2)
            }]
        );
    }

    #[test]
    fn visible_rows_collapse_and_search() {
        let cmds = vec![
            make_cmd("1", "Alpha", "Init", "AT"),
            make_cmd("2", "Beta", "Init", "AT+RST"),
            make_cmd("3", "Gamma", "Ping", "PING"),
        ];
        let mut collapsed = HashSet::new();
        collapsed.insert("Init".to_string());

        let rows = visible_rows(&cmds, "", &collapsed);
        assert_eq!(
            rows,
            vec![
                QuickRow::Group {
                    name: "Init".to_string(),
                    count: 2,
                    collapsed: true,
                },
                QuickRow::Group {
                    name: "Ping".to_string(),
                    count: 1,
                    collapsed: false,
                },
                QuickRow::Command {
                    index: 2,
                    shortcut: Some(3),
                },
            ]
        );

        let filtered = visible_rows(&cmds, "alpha", &collapsed);
        assert_eq!(
            filtered,
            vec![QuickRow::Command {
                index: 0,
                shortcut: Some(1),
            }]
        );
    }

    #[test]
    fn quick_shortcut_digit_requires_alt_only() {
        assert_eq!(quick_shortcut_digit(false, true, false, false, "3"), Some(3));
        assert_eq!(quick_shortcut_digit(true, true, false, false, "3"), None);
        assert_eq!(quick_shortcut_digit(false, true, true, false, "3"), None);
        assert_eq!(quick_shortcut_digit(false, true, false, true, "3"), None);
        assert_eq!(quick_shortcut_digit(true, false, false, false, "3"), None);
        assert_eq!(quick_shortcut_digit(false, true, false, false, "0"), None);
        assert_eq!(quick_shortcut_digit(false, true, false, false, "a"), None);
    }

    #[test]
    fn editor_draft_roundtrip_and_validation() {
        let cmd = make_cmd("c1", "AT", "Init", "AT");
        let mut draft = QuickEditorDraft::from_command(&cmd);
        draft.name = "  AT  ".to_string();
        draft.group = "  Init  ".to_string();
        let saved = draft.to_command(std::slice::from_ref(&cmd)).expect("valid");
        assert_eq!(saved.id, "c1");
        assert_eq!(saved.name, "AT");
        assert_eq!(saved.group, "Init");

        let mut new_draft = QuickEditorDraft::blank();
        new_draft.name = "New".to_string();
        new_draft.content = "OK".to_string();
        let created = new_draft.to_command(&[cmd]).expect("valid new");
        assert_ne!(created.id, "c1");
        assert!(!created.id.is_empty());

        let mut bad_hex = QuickEditorDraft::blank();
        bad_hex.name = "Hex".to_string();
        bad_hex.content = "ZZ".to_string();
        bad_hex.hex = true;
        assert_eq!(bad_hex.to_command(&[]), Err(QuickCommandError::InvalidHex));
    }

    #[test]
    fn upsert_remove_move() {
        let mut cmds = vec![
            make_cmd("c1", "A", "G1", "1"),
            make_cmd("c2", "B", "G2", "2"),
            make_cmd("c3", "C", "G1", "3"),
        ];

        // Replace in place
        let updated_c2 = make_cmd("c2", "B2", "G2", "22");
        assert_eq!(upsert_command(&mut cmds, updated_c2), Ok(()));
        assert_eq!(cmds[1].name, "B2");

        // Move within group G1: c3 up (-1) swaps with c1
        assert!(move_command(&mut cmds, "c3", -1));
        assert_eq!(cmds[0].id, "c3");
        assert_eq!(cmds[2].id, "c1");
        // No neighbor in direction
        assert!(!move_command(&mut cmds, "c3", -1));
        assert!(!move_command(&mut cmds, "c2", 1));

        // Remove
        assert!(remove_command(&mut cmds, "c2"));
        assert!(!remove_command(&mut cmds, "missing"));

        // Max limit
        let mut full: Vec<QuickCommand> = (0..MAX_QUICK_COMMANDS)
            .map(|i| make_cmd(&format!("id-{i}"), "n", "", "c"))
            .collect();
        assert_eq!(
            upsert_command(&mut full, make_cmd("extra", "n", "", "c")),
            Err(QuickCommandError::TooMany)
        );
    }

    #[test]
    fn edit_group_changes_order_and_delete_shifts_shortcut() {
        let mut cmds = vec![
            make_cmd("c1", "A", "G1", "1"),
            make_cmd("c2", "B", "G2", "2"),
            make_cmd("c3", "C", "G1", "3"),
        ];
        // Initial display_order: c1(0), c3(2), c2(1) -> Alt+2 is c3
        assert_eq!(command_for_shortcut(&cmds, 2).map(|c| c.id.as_str()), Some("c3"));

        // Edit c3's group to G2 via draft -> id preserved, display_order becomes c1(0), c2(1), c3(2)
        let mut draft = QuickEditorDraft::from_command(&cmds[2]);
        draft.group = "G2".to_string();
        let edited = draft.to_command(&cmds).expect("valid");
        assert_eq!(edited.id, "c3");
        upsert_command(&mut cmds, edited).expect("upsert");
        assert_eq!(command_for_shortcut(&cmds, 2).map(|c| c.id.as_str()), Some("c2"));

        // Delete c1 -> Alt+1 shifts to c2
        assert!(remove_command(&mut cmds, "c1"));
        assert_eq!(command_for_shortcut(&cmds, 1).map(|c| c.id.as_str()), Some("c2"));

        // Draft with missing editing_id keeps id and appends on upsert
        let mut ghost = QuickEditorDraft::blank();
        ghost.editing_id = Some("custom-id".to_string());
        ghost.name = "Ghost".to_string();
        ghost.content = "OK".to_string();
        let ghost_cmd = ghost.to_command(&cmds).expect("valid");
        assert_eq!(ghost_cmd.id, "custom-id");
        upsert_command(&mut cmds, ghost_cmd).expect("append");
        assert!(cmds.iter().any(|c| c.id == "custom-id"));
    }
}
