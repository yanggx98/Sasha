use std::collections::BTreeMap;

use super::*;

const CTRL: Modifiers = Modifiers::new(true, false, false, false);
const ALT: Modifiers = Modifiers::new(false, true, false, false);
const CTRL_SHIFT: Modifiers = Modifiers::new(true, false, true, false);
const CTRL_ALT: Modifiers = Modifiers::new(true, true, false, false);
const SHIFT: Modifiers = Modifiers::new(false, false, true, false);
const NONE: Modifiers = Modifiers::new(false, false, false, false);

fn chord(mods: Modifiers, key: &str) -> Binding {
    Binding::Chord(KeyChord {
        mods,
        key: key.to_string(),
    })
}

fn raw(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn defaults_resolve_like_the_old_hardcoded_keys() {
    let s = Shortcuts::default();
    assert_eq!(
        s.resolve("t", CTRL),
        Some((ShortcutAction::NewSession, None))
    );
    assert_eq!(
        s.resolve("w", CTRL),
        Some((ShortcutAction::CloseSession, None))
    );
    assert_eq!(
        s.resolve("tab", CTRL),
        Some((ShortcutAction::NextSession, None))
    );
    assert_eq!(
        s.resolve("tab", CTRL_SHIFT),
        Some((ShortcutAction::PrevSession, None))
    );
    if cfg!(target_os = "macos") {
        let cmd = Modifiers::new(false, false, false, true);
        assert_eq!(s.resolve("f", cmd), Some((ShortcutAction::Search, None)));
    } else {
        assert_eq!(s.resolve("f", CTRL), Some((ShortcutAction::Search, None)));
    }
    assert_eq!(
        s.resolve("3", CTRL),
        Some((ShortcutAction::JumpSession, Some(3)))
    );
    assert_eq!(
        s.resolve("3", ALT),
        Some((ShortcutAction::QuickSend, Some(3)))
    );
    assert_eq!(s.resolve("0", ALT), None);
    assert_eq!(s.resolve("a", ALT), None);
    assert_eq!(s.resolve("t", CTRL_SHIFT), None);
    assert_eq!(s.resolve("3", CTRL_ALT), None);
    assert_eq!(
        s.resolve("T", CTRL),
        Some((ShortcutAction::NewSession, None))
    );
}

#[test]
fn set_rejects_conflicts_and_keeps_old_value() {
    let mut s = Shortcuts::default();
    let before = s.binding(ShortcutAction::NewSession);
    assert_eq!(
        s.set(ShortcutAction::NewSession, chord(CTRL, "w")),
        Err(ShortcutError::Conflict(ShortcutAction::CloseSession))
    );
    assert_eq!(s.binding(ShortcutAction::NewSession), before);
    assert_eq!(s.set(ShortcutAction::NewSession, chord(CTRL, "n")), Ok(()));
    assert_eq!(s.resolve("t", CTRL), None);
    assert_eq!(
        s.resolve("n", CTRL),
        Some((ShortcutAction::NewSession, None))
    );
}

#[test]
fn setting_an_action_to_its_current_key_is_not_a_conflict() {
    let mut s = Shortcuts::default();
    assert_eq!(s.set(ShortcutAction::NewSession, chord(CTRL, "t")), Ok(()));
}

#[test]
fn set_requires_a_primary_modifier() {
    let mut s = Shortcuts::default();
    assert_eq!(
        s.set(ShortcutAction::NewSession, chord(SHIFT, "t")),
        Err(ShortcutError::NeedsModifier)
    );
    assert_eq!(
        s.set(ShortcutAction::NewSession, chord(NONE, "t")),
        Err(ShortcutError::NeedsModifier)
    );
    assert_eq!(
        s.set(ShortcutAction::QuickSend, Binding::Mods(SHIFT)),
        Err(ShortcutError::NeedsModifier)
    );
    assert_eq!(
        s.set(ShortcutAction::QuickSend, Binding::Mods(NONE)),
        Err(ShortcutError::NeedsModifier)
    );
}

#[test]
fn single_chord_conflicts_with_digit_families() {
    let mut s = Shortcuts::default();
    assert_eq!(
        s.set(ShortcutAction::Search, chord(CTRL, "1")),
        Err(ShortcutError::Conflict(ShortcutAction::JumpSession))
    );
    assert_eq!(
        s.set(ShortcutAction::JumpSession, Binding::Mods(ALT)),
        Err(ShortcutError::Conflict(ShortcutAction::QuickSend))
    );
    // A different modifier set does not clash with the families.
    assert_eq!(s.set(ShortcutAction::Search, chord(CTRL_ALT, "1")), Ok(()));
}

#[test]
fn set_rejects_wrong_binding_kind_and_bad_keys() {
    let mut s = Shortcuts::default();
    assert_eq!(
        s.set(ShortcutAction::NewSession, Binding::Mods(CTRL_ALT)),
        Err(ShortcutError::WrongBindingKind)
    );
    assert_eq!(
        s.set(ShortcutAction::QuickSend, chord(CTRL, "1")),
        Err(ShortcutError::WrongBindingKind)
    );
    assert_eq!(
        s.set(ShortcutAction::NewSession, chord(CTRL, "")),
        Err(ShortcutError::InvalidKey)
    );
    assert_eq!(
        s.set(ShortcutAction::NewSession, chord(CTRL, "shift")),
        Err(ShortcutError::InvalidKey)
    );
}

#[test]
fn from_keystroke_ignores_bare_modifiers() {
    assert_eq!(KeyChord::from_keystroke("shift", SHIFT), None);
    assert_eq!(KeyChord::from_keystroke("control", CTRL), None);
    assert_eq!(KeyChord::from_keystroke("alt", ALT), None);
    assert_eq!(KeyChord::from_keystroke("", CTRL), None);
    assert_eq!(
        KeyChord::from_keystroke("T", CTRL),
        Some(KeyChord {
            mods: CTRL,
            key: "t".into()
        })
    );
}

#[test]
fn chord_round_trips_through_text() {
    let parsed = KeyChord::parse("ctrl-shift-tab").unwrap();
    assert_eq!(parsed.mods, CTRL_SHIFT);
    assert_eq!(parsed.key, "tab");
    assert_eq!(parsed.to_config_string(), "ctrl-shift-tab");
    assert_eq!(parsed.display(), "Ctrl+Shift+Tab");
    assert_eq!(KeyChord::parse("ctrl-t").unwrap().display(), "Ctrl+T");
    assert_eq!(KeyChord::parse("alt-f3").unwrap().display(), "Alt+F3");
    assert_eq!(KeyChord::parse("ctrl-"), None);
    assert_eq!(KeyChord::parse(""), None);
    assert_eq!(KeyChord::parse("ctrl-shift"), None);
    assert_eq!(KeyChord::parse("bogus-t"), None);
}

#[test]
fn modifiers_parse_and_format() {
    assert_eq!(Modifiers::parse("ctrl-alt"), Some(CTRL_ALT));
    assert_eq!(Modifiers::parse("alt"), Some(ALT));
    assert_eq!(Modifiers::parse(""), None);
    assert_eq!(Modifiers::parse("ctrl-ctrl"), None);
    assert_eq!(Modifiers::parse("ctrl-x"), None);
    assert_eq!(CTRL_ALT.to_config_string(), "ctrl-alt");
    assert_eq!(CTRL_ALT.display(), "Ctrl+Alt");
    assert!(CTRL.has_primary());
    assert!(!SHIFT.has_primary());
}

#[test]
fn display_of_digit_families() {
    let s = Shortcuts::default();
    assert_eq!(s.display(ShortcutAction::JumpSession), "Ctrl+1\u{2026}9");
    assert_eq!(s.display(ShortcutAction::QuickSend), "Alt+1\u{2026}9");
    assert_eq!(s.display(ShortcutAction::NewSession), "Ctrl+T");
    assert_eq!(s.modifiers_for(ShortcutAction::QuickSend), Some(ALT));
    assert_eq!(s.modifiers_for(ShortcutAction::NewSession), None);
}

#[test]
fn from_raw_falls_back_per_item() {
    let s = Shortcuts::from_raw(&raw(&[
        ("new_session", "garbage"),
        ("search", "ctrl-g"),
        ("quick_send", "ctrl-alt"),
        ("jump_session", "shift"),
    ]));
    assert_eq!(
        s.binding(ShortcutAction::NewSession),
        Shortcuts::default().binding(ShortcutAction::NewSession)
    );
    assert_eq!(s.binding(ShortcutAction::Search), chord(CTRL, "g"));
    assert_eq!(
        s.binding(ShortcutAction::QuickSend),
        Binding::Mods(CTRL_ALT)
    );
    assert_eq!(
        s.binding(ShortcutAction::JumpSession),
        Shortcuts::default().binding(ShortcutAction::JumpSession)
    );
    assert_eq!(Shortcuts::from_raw(&BTreeMap::new()), Shortcuts::default());
}

#[test]
fn from_raw_never_produces_a_clashing_table() {
    let s = Shortcuts::from_raw(&raw(&[
        ("new_session", "ctrl-w"),
        ("close_session", "ctrl-w"),
    ]));
    assert_ne!(
        s.binding(ShortcutAction::NewSession),
        s.binding(ShortcutAction::CloseSession)
    );
    assert_eq!(s, Shortcuts::default());
}

#[test]
fn from_raw_allows_a_legitimate_swap() {
    let s = Shortcuts::from_raw(&raw(&[
        ("new_session", "ctrl-w"),
        ("close_session", "ctrl-t"),
    ]));
    assert_eq!(s.binding(ShortcutAction::NewSession), chord(CTRL, "w"));
    assert_eq!(s.binding(ShortcutAction::CloseSession), chord(CTRL, "t"));
}

#[test]
fn from_raw_rejects_a_chord_that_clashes_with_a_default_family() {
    let s = Shortcuts::from_raw(&raw(&[("search", "ctrl-1")]));
    assert_eq!(s, Shortcuts::default());
}

#[test]
fn to_raw_round_trips() {
    let mut s = Shortcuts::default();
    s.set(ShortcutAction::NewSession, chord(CTRL, "n")).unwrap();
    s.set(ShortcutAction::PrevSession, chord(CTRL_ALT, "left"))
        .unwrap();
    s.set(ShortcutAction::QuickSend, Binding::Mods(CTRL_ALT))
        .unwrap();
    let raw = s.to_raw();
    assert_eq!(raw.get("new_session").map(String::as_str), Some("ctrl-n"));
    assert_eq!(raw.get("quick_send").map(String::as_str), Some("ctrl-alt"));
    assert_eq!(raw.len(), 7);
    assert_eq!(Shortcuts::from_raw(&raw), s);
}

#[test]
fn reset_restores_defaults() {
    let mut s = Shortcuts::default();
    s.set(ShortcutAction::NewSession, chord(CTRL, "n")).unwrap();
    s.set(ShortcutAction::Search, chord(CTRL, "g")).unwrap();
    s.reset(ShortcutAction::NewSession);
    assert_eq!(
        s.binding(ShortcutAction::NewSession),
        Shortcuts::default().binding(ShortcutAction::NewSession)
    );
    assert_eq!(s.binding(ShortcutAction::Search), chord(CTRL, "g"));
    s.reset_all();
    assert_eq!(s, Shortcuts::default());
}
