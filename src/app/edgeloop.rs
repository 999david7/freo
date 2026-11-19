//! Fase / Rundung: when one edge of an opening (or of a face's outline) is picked, Freo finds the
//! whole loop of edges it belongs to and asks whether all of it should be chamfered or rounded.

use super::dlgkit::{btn, cwin_top, hline, lbl, pad};
use super::*;
use crate::kernel::topology::{EdgeRef, Topology};
use eframe::egui::{self, pos2, Color32, Pos2, Stroke};

/// A loop of edges offered for the feature being defined.
#[derive(Clone)]
pub struct LoopOffer {
    /// the edges of the loop that are not in the feature yet
    pub edges: Vec<EdgeRef>,
    /// outline of the loop, for the highlight
    pub lines: Vec<Vec<V3>>,
    /// the loop is an opening in its face (else the outline of the face)
    pub opening: bool,
    pub count: usize,
}

type Key = (i64, i64, i64);
fn key(p: V3) -> Key {
    ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64, (p.z * 1e4).round() as i64)
}

/// Area enclosed by a closed run of points.
fn loop_area(pts: &[V3]) -> f64 {
    let mut n = V3::ZERO;
    for i in 1..pts.len().saturating_sub(1) {
        n += (pts[i] - pts[0]).cross(pts[i + 1] - pts[0]);
    }
    n.length() * 0.5
}

/// The closed loop of edges on face `f` that contains chain `start`, in order.
pub fn loop_on_face(t: &Topology, f: usize, start: usize) -> Option<Vec<usize>> {
    let c0 = &t.chains[start];
    if c0.closed {
        return Some(vec![start]);
    }
    let on_face: Vec<usize> = (0..t.chains.len()).filter(|&i| !t.chains[i].closed && (t.chains[i].face_a == f || t.chains[i].face_b == f)).collect();
    let mut out = vec![start];
    let first = key(c0.points[0]);
    let mut at = key(*c0.points.last()?);
    let mut cur = start;
    while at != first {
        // the other edge of this face that starts or ends here
        let next = on_face
            .iter()
            .copied()
            .find(|&i| i != cur && !out[1..].contains(&i) && i != start && (key(t.chains[i].points[0]) == at || key(*t.chains[i].points.last().unwrap()) == at))?;
        let c = &t.chains[next];
        at = if key(c.points[0]) == at { key(*c.points.last().unwrap()) } else { key(c.points[0]) };
        out.push(next);
        cur = next;
        if out.len() > t.chains.len() {
            return None;
        }
    }
    Some(out)
}

/// Points of a loop of chains, run end to end.
fn loop_points(t: &Topology, l: &[usize]) -> Vec<V3> {
    let mut pts: Vec<V3> = vec![];
    for &i in l {
        let c = &t.chains[i];
        let rev = pts.last().is_some_and(|p| key(*p) != key(c.points[0]));
        if rev {
            pts.extend(c.points.iter().rev().skip(1));
        } else if pts.is_empty() {
            pts.extend(c.points.iter());
        } else {
            pts.extend(c.points.iter().skip(1));
        }
    }
    // the first chain may have been taken the wrong way round for the second one
    pts
}

/// The loop to offer for the picked chain: on one of its two faces, all edges bending the same
/// way as the picked one (so an opening's rim is taken, not the wall beside it).
pub fn find_loop(t: &Topology, chain: usize) -> Option<(Vec<usize>, bool)> {
    let c = t.chains.get(chain)?;
    if c.closed {
        return None;
    }
    let mut best: Option<(Vec<usize>, bool, f64)> = None;
    for f in [c.face_a, c.face_b] {
        let Some(l) = loop_on_face(t, f, chain) else { continue };
        if l.len() < 2 || l.iter().any(|&i| t.chains[i].convex != c.convex) {
            continue;
        }
        // an opening: the face has another, larger loop around this one
        let area = loop_area(&loop_points(t, &l));
        let mut opening = false;
        for i in (0..t.chains.len()).filter(|&i| !l.contains(&i) && (t.chains[i].face_a == f || t.chains[i].face_b == f)) {
            if let Some(o) = loop_on_face(t, f, i) {
                if loop_area(&loop_points(t, &o)) > area {
                    opening = true;
                    break;
                }
            }
        }
        let fa = t.faces.get(f).map(|x| x.area).unwrap_or(0.0);
        // an opening is the more likely wish; else the larger face
        let score = if opening { 1e18 } else { fa };
        if best.as_ref().is_none_or(|b| score > b.2) {
            best = Some((l, opening, score));
        }
    }
    best.map(|b| (b.0, b.1))
}

impl App {
    /// The loop around the picked edge, minus the edges the feature has already.
    pub fn edge_loop_offer(&self, item: Pick, have: &[EdgeRef]) -> Option<LoopOffer> {
        let Pick::Edge(i) = item else { return None };
        // The loop is looked for in the model before the feature being defined: its preview has
        // already cut the edges picked so far, and the feature's references belong to that model.
        let shown = self.displayed();
        let er = shown.topo.chains.get(i).map(|_| shown.topo.edge_ref(i))?;
        let r = if self.editor.is_some() { self.context.as_ref().unwrap_or(shown) } else { shown };
        let t = &r.topo;
        let i = t.match_chain(&er).filter(|&k| (t.chains[k].mid() - er.m).length() < 1e-6)?;
        let (l, opening) = find_loop(t, i)?;
        let has = |e: &EdgeRef| have.iter().any(|h| (h.m - e.m).length() < 1e-6);
        let edges: Vec<EdgeRef> = l.iter().filter(|&&k| k != i).map(|&k| t.edge_ref(k)).filter(|e| !has(e)).collect();
        if edges.is_empty() {
            return None;
        }
        // asked once: a loop that was declined is not offered again while this feature is open
        if self.loop_declined.iter().any(|m| l.iter().any(|&k| (t.chains[k].mid() - *m).length() < 1e-6)) {
            return None;
        }
        Some(LoopOffer { edges, lines: l.iter().map(|&k| t.chains[k].points.clone()).collect(), opening, count: l.len() })
    }

    /// Adds the offered loop to the Fase or Rundung being defined.
    pub fn accept_loop(&mut self) {
        let Some(o) = self.loop_offer.take() else { return };
        let rs = self.round_set;
        let n = o.edges.len();
        self.update_editor(|f| {
            let list = match &mut f.kind {
                FeatureKind::Round(d) => {
                    let i = rs.min(d.sets.len() - 1);
                    Some(&mut d.sets[i].edges)
                }
                FeatureKind::Chamfer(d) => {
                    let i = rs.min(d.additional_sets.len());
                    Some(&mut d.set_mut(i).edges)
                }
                _ => None,
            };
            if let Some(l) = list {
                for e in o.edges {
                    if !l.iter().any(|h| (h.m - e.m).length() < 1e-6) {
                        l.push(e);
                    }
                }
            }
        });
        self.say(format!("{} weitere Kante(n) hinzugefügt: {}.", n, if o.opening { "die gesamte Öffnung" } else { "die gesamte Umrandung" }), MsgKind::Info);
    }

    pub fn decline_loop(&mut self) {
        if let Some(o) = self.loop_offer.take() {
            self.loop_declined.extend(o.lines.iter().filter(|l| l.len() >= 2).map(|l| {
                let c =
                    crate::kernel::topology::EdgeChain { points: l.clone(), closed: false, face_a: 0, face_b: 0, n_a: vec![], n_b: vec![], f_a: vec![], f_b: vec![], convex: true };
                c.mid()
            }));
        }
    }

    /// The question "Soll die gesamte Öffnung gefast werden?".
    pub fn loop_offer_dialog(&mut self, ctx: &egui::Context) {
        let Some(o) = self.loop_offer.clone() else { return };
        let Some(ed) = &self.editor else {
            self.loop_offer = None;
            return;
        };
        let verb = match ed.feature.kind {
            FeatureKind::Round(_) => "verrundet",
            FeatureKind::Chamfer(_) => "gefast",
            _ => {
                self.loop_offer = None;
                return;
            }
        };
        let what = if o.opening { "die gesamte Öffnung" } else { "die gesamte Umrandung der Fläche" };
        let (mut yes, mut no) = (false, false);
        let open = cwin_top(ctx, "dlg_loop_offer", if verb == "gefast" { "Fase" } else { "Rundung" }, false, pos2(self.viewport_size.0 * 0.5 + 120.0, 210.0), 400.0, |ui| {
            pad(ui, 14.0, |ui| {
                lbl(ui, &format!("Soll {} {} werden?", what, verb));
                lbl(ui, &format!("Die gewählte Kante gehört zu einer geschlossenen Kette aus {} Kanten (im Modell hervorgehoben).", o.count));
            });
            hline(ui);
            pad(ui, 10.0, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if btn(ui, "Nein", 100.0, 0) {
                            no = true;
                        }
                        if btn(ui, "Ja", 100.0, 1) {
                            yes = true;
                        }
                    });
                });
            });
        });
        let (enter, esc) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if yes || enter {
            self.accept_loop();
        } else if no || esc || !open {
            self.decline_loop();
        }
    }

    /// The offered loop, drawn over the model while the question is open.
    pub fn draw_loop_offer(&self, painter: &egui::Painter, to_screen: &dyn Fn(V3) -> Option<Pos2>) {
        let Some(o) = &self.loop_offer else { return };
        for l in &o.lines {
            let pts: Vec<Pos2> = l.iter().filter_map(|p| to_screen(*p)).collect();
            if pts.len() >= 2 {
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(5.0_f32, Color32::WHITE)));
                painter.add(egui::Shape::line(pts, Stroke::new(3.0_f32, Color32::from_rgb(230, 40, 30))));
            }
        }
    }
}
