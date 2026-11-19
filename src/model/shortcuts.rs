//! Configurable keyboard shortcuts: key chords, defaults, conflict detection and
//! matching. No UI code lives here.

use std::collections::BTreeMap;

/// Key names that are modifiers themselves and never a chord's main key.
const MODIFIER_KEYS: [&str; 12] = [
    "control", "ctrl", "shift", "alt", "meta", "platform", "cmd", "super", "win", "windows",
    "function", "fn",
];

fn is_modifier_key(key: &str) -> bool {
    MODIFIER_KEYS.contains(&key)
}

/// `Some(1..=9)` when `key` is a single digit 1 to 9.
fn digit_of(key: &str) -> Option<usize> {
    let mut chars = key.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    match first.to_digit(10) {
        Some(d @ 1..=9) => Some(d as usize),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub platform: bool,
}

impl Modifiers {
    pub const fn new(ctrl: bool, alt: bool, shift: bool, platform: bool) -> Self {
        Self {
            ctrl,
            alt,
            shift,
            platform,
        }
    }

    /// Parses `"ctrl-alt"` style text. Empty text, unknown names and repeats are rejected.
    pub fn parse(text: &str) -> Option<Modifiers> {
        if text.is_empty() {
            return None;
        }
        Self::parse_tokens(&text.split('-').collect::<Vec<_>>())
    }

    /// Like [`Modifiers::parse`] but accepts zero tokens (no modifiers).
    fn parse_tokens(tokens: &[&str]) -> Option<Modifiers> {
        let mut mods = Modifiers::default();
        for token in tokens {
            let slot = match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => &mut mods.ctrl,
                "alt" => &mut mods.alt,
                "shift" => &mut mods.shift,
                "cmd" | "meta" | "super" | "win" => &mut mods.platform,
                _ => return None,
            };
            if *slot {
                return None;
            }
            *slot = true;
        }
        Some(mods)
    }

    fn names(self, labels: [&'static str; 4]) -> Vec<&'static str> {
        [self.ctrl, self.alt, self.shift, self.platform]
            .into_iter()
            .zip(labels)
            .filter_map(|(on, label)| on.then_some(label))
            .collect()
    }

    pub fn to_config_string(self) -> String {
        self.names(["ctrl", "alt", "shift", "cmd"]).join("-")
    }

    pub fn display(self) -> String {
        self.names(["Ctrl", "Alt", "Shift", "Cmd"]).join("+")
    }

    /// Whether Ctrl, Alt or Cmd is held (Shift alone is not enough for a shortcut).
    pub fn has_primary(self) -> bool {
        self.ctrl || self.alt || self.platform
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    pub mods: Modifiers,
    /// Lower-case key name as GPUI reports it: `"t"`, `"tab"`, `"f3"`, `"1"`.
    pub key: String,
}

impl KeyChord {
    /// Parses `"ctrl-shift-tab"`. The last token is the key; it must not be a modifier name.
    pub fn parse(text: &str) -> Option<KeyChord> {
        let tokens: Vec<&str> = text.split('-').collect();
        let (key, mod_tokens) = tokens.split_last()?;
        let key = key.to_ascii_lowercase();
        if key.is_empty() || is_modifier_key(&key) {
            return None;
        }
        Some(KeyChord {
            mods: Modifiers::parse_tokens(mod_tokens)?,
            key,
        })
    }

    pub fn to_config_string(&self) -> String {
        if self.mods == Modifiers::default() {
            self.key.clone()
        } else {
            format!("{}-{}", self.mods.to_config_string(), self.key)
        }
    }

    pub fn display(&self) -> String {
        let mut chars = self.key.chars();
        let label = match chars.next() {
            Some(first) if self.key.chars().count() == 1 => first.to_uppercase().collect(),
            Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
            None => String::new(),
        };
        if self.mods == Modifiers::default() {
            label
        } else {
            format!("{}+{label}", self.mods.display())
        }
    }

    /// The chord a key press forms. Presses of a bare modifier key return `None`.
    pub fn from_keystroke(key: &str, mods: Modifiers) -> Option<KeyChord> {
        let key = key.to_ascii_lowercase();
        if key.is_empty() || is_modifier_key(&key) {
            return None;
        }
        Some(KeyChord { mods, key })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    NewSession,
    CloseSession,
    NextSession,
    PrevSession,
    Search,
    JumpSession,
    QuickSend,
}

impl ShortcutAction {
    pub const ALL: [ShortcutAction; 7] = [
        Self::NewSession,
        Self::CloseSession,
        Self::NextSession,
        Self::PrevSession,
        Self::Search,
        Self::JumpSession,
        Self::QuickSend,
    ];

    /// `JumpSession` and `QuickSend` cover the keys 1 to 9 and only configure a modifier set.
    pub fn is_digit_family(self) -> bool {
        matches!(self, Self::JumpSession | Self::QuickSend)
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|a| *a == self).unwrap_or(0)
    }

    /// Key used in the `[shortcuts]` table of `config.toml`.
    pub fn config_key(self) -> &'static str {
        match self {
            Self::NewSession => "new_session",
            Self::CloseSession => "close_session",
            Self::NextSession => "next_session",
            Self::PrevSession => "prev_session",
            Self::Search => "search",
            Self::JumpSession => "jump_session",
            Self::QuickSend => "quick_send",
        }
    }

    fn default_binding(self) -> Binding {
        let ctrl = Modifiers::new(true, false, false, false);
        let chord = |mods, key: &str| {
            Binding::Chord(KeyChord {
                mods,
                key: key.to_string(),
            })
        };
        match self {
            Self::NewSession => chord(ctrl, "t"),
            Self::CloseSession => chord(ctrl, "w"),
            Self::NextSession => chord(ctrl, "tab"),
            Self::PrevSession => chord(Modifiers::new(true, false, true, false), "tab"),
            Self::Search if cfg!(target_os = "macos") => {
                chord(Modifiers::new(false, false, false, true), "f")
            }
            Self::Search => chord(ctrl, "f"),
            Self::JumpSession => Binding::Mods(ctrl),
            Self::QuickSend => Binding::Mods(Modifiers::new(false, true, false, false)),
        }
    }
}

/// What one action is bound to: a full chord, or (for the 1 to 9 families) just modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    Chord(KeyChord),
    Mods(Modifiers),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutError {
    NeedsModifier,
    InvalidKey,
    WrongBindingKind,
    Conflict(ShortcutAction),
}

/// Whether two bindings would both fire for some key press.
fn clash(a: &Binding, b: &Binding) -> bool {
    match (a, b) {
        (Binding::Chord(x), Binding::Chord(y)) => x == y,
        (Binding::Mods(x), Binding::Mods(y)) => x == y,
        (Binding::Chord(c), Binding::Mods(m)) | (Binding::Mods(m), Binding::Chord(c)) => {
            c.mods == *m && digit_of(&c.key).is_some()
        }
    }
}

/// Checks the binding is valid for the action on its own, returning it normalized.
fn validate(action: ShortcutAction, binding: Binding) -> Result<Binding, ShortcutError> {
    match (action.is_digit_family(), binding) {
        (true, Binding::Mods(mods)) => {
            if mods.has_primary() {
                Ok(Binding::Mods(mods))
            } else {
                Err(ShortcutError::NeedsModifier)
            }
        }
        (false, Binding::Chord(chord)) => {
            let key = chord.key.to_ascii_lowercase();
            if key.is_empty() || is_modifier_key(&key) {
                return Err(ShortcutError::InvalidKey);
            }
            if !chord.mods.has_primary() {
                return Err(ShortcutError::NeedsModifier);
            }
            Ok(Binding::Chord(KeyChord {
                mods: chord.mods,
                key,
            }))
        }
        _ => Err(ShortcutError::WrongBindingKind),
    }
}

fn parse_binding(action: ShortcutAction, text: &str) -> Option<Binding> {
    if action.is_digit_family() {
        Modifiers::parse(text).map(Binding::Mods)
    } else {
        KeyChord::parse(text).map(Binding::Chord)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcuts {
    entries: [Binding; 7],
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            entries: ShortcutAction::ALL.map(ShortcutAction::default_binding),
        }
    }
}

impl Shortcuts {
    pub fn binding(&self, action: ShortcutAction) -> Binding {
        self.entries[action.index()].clone()
    }

    /// Rebinds `action`. On any error the table is left unchanged.
    pub fn set(&mut self, action: ShortcutAction, binding: Binding) -> Result<(), ShortcutError> {
        let binding = validate(action, binding)?;
        for other in ShortcutAction::ALL {
            if other != action && clash(&binding, &self.entries[other.index()]) {
                return Err(ShortcutError::Conflict(other));
            }
        }
        self.entries[action.index()] = binding;
        Ok(())
    }

    pub fn reset(&mut self, action: ShortcutAction) {
        self.entries[action.index()] = action.default_binding();
    }

    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    /// `"Ctrl+T"` for a single key, `"Ctrl+1\u{2026}9"` for a digit family.
    pub fn display(&self, action: ShortcutAction) -> String {
        match &self.entries[action.index()] {
            Binding::Chord(chord) => chord.display(),
            Binding::Mods(mods) => format!("{}+1\u{2026}9", mods.display()),
        }
    }

    /// The configured modifiers of a digit family; `None` for single-key actions.
    pub fn modifiers_for(&self, action: ShortcutAction) -> Option<Modifiers> {
        match &self.entries[action.index()] {
            Binding::Mods(mods) => Some(*mods),
            Binding::Chord(_) => None,
        }
    }

    /// The action a key press triggers, with the digit for the 1 to 9 families. Modifiers
    /// must match the binding exactly, Shift included.
    pub fn resolve(&self, key: &str, mods: Modifiers) -> Option<(ShortcutAction, Option<usize>)> {
        let key = key.to_ascii_lowercase();
        ShortcutAction::ALL
            .into_iter()
            .find_map(|action| match &self.entries[action.index()] {
                Binding::Chord(chord) if chord.mods == mods && chord.key == key => {
                    Some((action, None))
                }
                Binding::Mods(bound) if *bound == mods => {
                    digit_of(&key).map(|digit| (action, Some(digit)))
                }
                _ => None,
            })
    }

    fn has_clash(&self) -> bool {
        self.entries
            .iter()
            .enumerate()
            .any(|(i, a)| self.entries[i + 1..].iter().any(|b| clash(a, b)))
    }

    /// Builds the table from `[shortcuts]` text. Invalid entries keep their default. An
    /// entry that clashes with the rest is ignored; if the table still clashes at the end
    /// (a default now collides with a custom key), everything falls back to the defaults.
    pub fn from_raw(raw: &BTreeMap<String, String>) -> Shortcuts {
        let candidates: Vec<(ShortcutAction, Binding)> = ShortcutAction::ALL
            .into_iter()
            .filter_map(|action| {
                let text = raw.get(action.config_key())?;
                let binding = validate(action, parse_binding(action, text)?).ok()?;
                Some((action, binding))
            })
            .collect();
        let is_candidate = |action| candidates.iter().any(|(a, _)| *a == action);

        let mut table = Shortcuts::default();
        let mut processed: Vec<ShortcutAction> = Vec::new();
        for (action, binding) in &candidates {
            // Compare with every other action except candidates that are still waiting for
            // their own turn: their entry in `table` is only a placeholder.
            let clashes = ShortcutAction::ALL.into_iter().any(|other| {
                other != *action
                    && (!is_candidate(other) || processed.contains(&other))
                    && clash(binding, &table.entries[other.index()])
            });
            if !clashes {
                table.entries[action.index()] = binding.clone();
            }
            processed.push(*action);
        }
        if table.has_clash() {
            Shortcuts::default()
        } else {
            table
        }
    }

    pub fn to_raw(&self) -> BTreeMap<String, String> {
        ShortcutAction::ALL
            .into_iter()
            .map(|action| {
                let text = match &self.entries[action.index()] {
                    Binding::Chord(chord) => chord.to_config_string(),
                    Binding::Mods(mods) => mods.to_config_string(),
                };
                (action.config_key().to_string(), text)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
