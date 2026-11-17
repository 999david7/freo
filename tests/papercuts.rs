//! Small things a user notices: deleting takes its real children along, Undo back to the
//! saved part is not an unsaved change, and failure messages go away once fixed.

use freo::app::App;
use freo::kernel::math::*;
use freo::model::features::*;
use freo::sketch::sketch::Sketch;

fn feat(id: u64, kind: FeatureKind) -> Feature {
    Feature { id, name: format!("F{}", id), suppressed: false, hidden: false, kind }
}

/// A 40×30×10 block on TOP with a through hole placed on its top face (a face reference, not a datum).
fn block_with_hole() -> App {
    let mut m = ModelData::new_part("B");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = Sketch::rect(40.0, 30.0, 0.0, 0.0);
    e.depth = Dim::v(10.0);
    m.features.push(feat(10, FeatureKind::Extrude(e)));
    let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
    h.plane = Some(PlaneRef::Face(FaceRef { point: V3::new(0.0, 10.0, 0.0), normal: V3::Y }));
    h.through_all = true;
    m.features.push(feat(11, FeatureKind::Hole(h)));
    m.next_id = 12;
    let mut a = App::new(None);
    a.data = m;
    a.regenerate(false);
    assert!(a.result.errors.is_empty(), "{:?}", a.result.errors);
    a.mark_saved();
    a
}

#[test]
fn deleting_a_feature_takes_features_on_its_faces_along() {
    let mut a = block_with_hole();
    assert_eq!(a.delete_set(&[10]), vec![10, 11]);
    a.delete_features(&[10]);
    assert!(a.data.feature(10).is_none() && a.data.feature(11).is_none());
    assert!(a.result.errors.is_empty(), "nothing is left failing: {:?}", a.result.errors);
    // a feature nothing depends on goes alone
    let mut a = block_with_hole();
    assert_eq!(a.delete_set(&[11]), vec![11]);
}

#[test]
fn undo_back_to_the_saved_part_is_not_unsaved() {
    let mut a = block_with_hole();
    a.rename(11, "BOHRUNG_A");
    assert!(a.dirty);
    a.do_undo();
    assert!(!a.dirty, "undo returned to exactly the saved part");
    a.do_redo();
    assert!(a.dirty);
}

#[test]
fn failure_messages_go_away_once_the_feature_works_again() {
    let mut a = block_with_hole();
    // breaking the block's sketch plane makes the hole lose its face
    a.commit("", |d| d.features.retain(|f| f.id != 10));
    assert!(a.messages.iter().any(|m| m.failed == Some(11)));
    a.do_undo();
    assert!(a.result.errors.is_empty());
    assert!(a.messages.iter().all(|m| m.failed.is_none()), "no stale red message after Undo");
}
