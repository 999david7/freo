//! Helpers of the Werkzeuge dialogs: relation ordering, word search, saved parameter flags.
use freo::app::toolsdlg::{has_word, rel_lines, reorder_relations};
use freo::model::features::{ArTarget, ModelData};

#[test]
fn word_search_respects_boundaries() {
    assert!(has_word("HOEHE = breite / 3", "BREITE"));
    assert!(!has_word("HOEHE = BREITE2 / 3", "BREITE"));
    assert!(!has_word("X = MY_BREITE", "BREITE"));
    assert!(has_word("ceil(BREITE/25)", "breite"));
}

#[test]
fn relations_are_reordered_by_dependency() {
    let rel = "C = B * 2\n/* Grundmaß\nB = A + 1\nA = 5";
    let out = reorder_relations(rel);
    let order: Vec<String> = rel_lines(&out).into_iter().map(|l| l.0).collect();
    assert_eq!(order, ["A", "B", "C"]);
    // the comment stays above the line it belonged to
    let lines: Vec<&str> = out.lines().collect();
    let b = lines.iter().position(|l| l.starts_with("B =")).unwrap();
    assert_eq!(lines[b - 1], "/* Grundmaß");
    // already ordered relations are left alone
    assert_eq!(reorder_relations("A = 5\nB = A + 1"), "A = 5\nB = A + 1");
}

#[test]
fn designated_parameters_and_targets_are_saved_with_the_part() {
    let mut d = ModelData::new_part("T");
    d.designated = vec!["BREITE".into()];
    d.restricted = vec!["DICKE".into()];
    d.ar_targets.push(ArTarget { thing: true, side: 1, offset: 2.5, width: 40.0, name: "THINGMARK_1".into() });
    let json = serde_json::to_string(&d).unwrap();
    let back: ModelData = serde_json::from_str(&json).unwrap();
    assert_eq!(back.designated, ["BREITE"]);
    assert_eq!(back.restricted, ["DICKE"]);
    assert_eq!(back.ar_targets.len(), 1);
    assert!(back.ar_targets[0].thing);
    // a part saved before these fields existed still opens
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    for k in ["designated", "restricted", "ar_targets", "published"] {
        v.as_object_mut().unwrap().remove(k);
    }
    let old: ModelData = serde_json::from_value(v).unwrap();
    assert!(old.designated.is_empty() && old.ar_targets.is_empty() && old.published.is_empty());
}
