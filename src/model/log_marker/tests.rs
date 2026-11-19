use super::*;

#[test]
fn set_trims_and_overwrites_the_same_entry() {
    let mut markers = LogMarkers::default();
    assert_eq!(markers.set(5, "  err  ", MarkerColor::Red), Ok(()));
    assert_eq!(markers.get(5).unwrap().name, "err");
    assert_eq!(markers.set(5, "ok", MarkerColor::Blue), Ok(()));
    assert_eq!(markers.len(), 1);
    let marker = markers.get(5).unwrap();
    assert_eq!(marker.name, "ok");
    assert_eq!(marker.color, MarkerColor::Blue);
}

#[test]
fn set_rejects_empty_and_too_long_names_and_keeps_the_old_marker() {
    let mut markers = LogMarkers::default();
    markers.set(1, "keep", MarkerColor::Green).unwrap();
    assert_eq!(
        markers.set(1, "", MarkerColor::Red),
        Err(MarkerError::EmptyName)
    );
    assert_eq!(
        markers.set(1, "   ", MarkerColor::Red),
        Err(MarkerError::EmptyName)
    );
    assert_eq!(
        markers.set(1, &"x".repeat(MARKER_NAME_MAX_CHARS + 1), MarkerColor::Red),
        Err(MarkerError::NameTooLong)
    );
    assert_eq!(markers.get(1).unwrap().name, "keep");
    // The limit counts characters, not bytes.
    let cjk = "测".repeat(MARKER_NAME_MAX_CHARS);
    assert_eq!(markers.set(2, &cjk, MarkerColor::Pink), Ok(()));
    assert_eq!(
        markers.set(3, &format!("{cjk}测"), MarkerColor::Pink),
        Err(MarkerError::NameTooLong)
    );
}

#[test]
fn remove_and_clear() {
    let mut markers = LogMarkers::default();
    markers.set(1, "a", MarkerColor::Red).unwrap();
    markers.set(2, "b", MarkerColor::Red).unwrap();
    assert!(markers.remove(1));
    assert!(!markers.remove(1));
    assert_eq!(markers.len(), 1);
    markers.clear();
    assert!(markers.is_empty());
}

#[test]
fn prune_before_drops_evicted_entries() {
    let mut markers = LogMarkers::default();
    for id in [1, 5, 9] {
        markers.set(id, "m", MarkerColor::Orange).unwrap();
    }
    markers.prune_before(Some(5));
    let ids: Vec<u64> = markers.iter().map(|(id, _)| id).collect();
    assert_eq!(ids, vec![5, 9]);
    markers.prune_before(None);
    assert!(markers.is_empty());
}

#[test]
fn iter_is_ordered_by_entry_id() {
    let mut markers = LogMarkers::default();
    for id in [9, 2, 7] {
        markers.set(id, "m", MarkerColor::Cyan).unwrap();
    }
    let ids: Vec<u64> = markers.iter().map(|(id, _)| id).collect();
    assert_eq!(ids, vec![2, 7, 9]);
}

#[test]
fn colors_are_distinct_rgb_values() {
    assert_eq!(MarkerColor::ALL.len(), 8);
    let mut seen = std::collections::HashSet::new();
    for color in MarkerColor::ALL {
        assert!(color.rgb() <= 0xFFFFFF);
        assert!(seen.insert(color.rgb()), "{color:?} repeats a color");
    }
}

#[test]
fn minimap_fraction_is_within_bounds() {
    assert_eq!(minimap_fraction(0, 1), 0.5);
    assert!(minimap_fraction(0, 100) > 0.0);
    assert!(minimap_fraction(99, 100) < 1.0);
    assert_eq!(minimap_fraction(3, 0), 0.0);
    assert!(minimap_fraction(10, 100) < minimap_fraction(11, 100));
}
