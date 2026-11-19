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

#[test]
fn marker_errors_map_to_locale_keys() {
    assert_eq!(MarkerError::EmptyName.locale_key(), "marker.err_empty_name");
    assert_eq!(
        MarkerError::NameTooLong.locale_key(),
        "marker.err_name_long"
    );
}

#[test]
fn visible_marker_positions_only_includes_visible_rows() {
    let mut markers = LogMarkers::default();
    for id in [11, 13, 99] {
        markers.set(id, "m", MarkerColor::Red).unwrap();
    }
    let rows = [10, 11, 12, 13];
    assert_eq!(
        visible_marker_positions(&rows, &markers),
        vec![(1, 11), (3, 13)]
    );
}

#[test]
fn visible_marker_positions_is_empty_without_markers_or_rows() {
    let markers = LogMarkers::default();
    assert!(visible_marker_positions(&[1, 2, 3], &markers).is_empty());
    let mut markers = LogMarkers::default();
    markers.set(1, "m", MarkerColor::Red).unwrap();
    assert!(visible_marker_positions(&[], &markers).is_empty());
}

#[test]
fn visible_marker_positions_handles_many_rows_in_order() {
    let mut markers = LogMarkers::default();
    for id in (0..1000u64).step_by(100) {
        markers.set(id, "m", MarkerColor::Blue).unwrap();
    }
    let rows: Vec<u64> = (0..1000).collect();
    let positions = visible_marker_positions(&rows, &markers);
    assert_eq!(positions.len(), 10);
    assert!(positions.windows(2).all(|w| w[0].0 < w[1].0));
    assert_eq!(positions[3], (300, 300));
}

#[test]
fn panel_items_marks_filtered_entries() {
    let mut markers = LogMarkers::default();
    for id in [3, 7, 9] {
        markers.set(id, "m", MarkerColor::Green).unwrap();
    }
    assert_eq!(
        panel_items(&markers, &[3, 9]),
        vec![(3, true), (7, false), (9, true)]
    );
}

#[test]
fn panel_items_orders_by_entry_id_and_handles_no_visible_rows() {
    let mut markers = LogMarkers::default();
    for id in [9, 2, 5] {
        markers.set(id, "m", MarkerColor::Purple).unwrap();
    }
    let ids: Vec<u64> = panel_items(&markers, &[])
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids, vec![2, 5, 9]);
    assert!(
        panel_items(&markers, &[])
            .iter()
            .all(|(_, visible)| !visible)
    );
    assert!(panel_items(&LogMarkers::default(), &[1, 2]).is_empty());
}
