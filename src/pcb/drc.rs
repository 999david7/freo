//! Design rules check: copper clearances and shorts, track widths, drills and annular rings,
//! hole spacing, copper to board edge, courtyard overlaps, unrouted connections and parity
//! with the schematic.

use super::board::{all_layers, Board, Copper, Item};
use super::geom::{in_poly, p, pt_seg, seg_seg, Hull, P};
use super::schematic::Schematic;
use crate::slicer::poly;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Short,
    Clearance,
    TrackWidth,
    Drill,
    Annular,
    HoleClearance,
    Edge,
    OutsideBoard,
    Courtyard,
    Unrouted,
    NoOutline,
    Parity,
    Skew,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Short => "Kurzschluss",
            Kind::Clearance => "Abstand",
            Kind::TrackWidth => "Leiterbahnbreite",
            Kind::Drill => "Bohrung",
            Kind::Annular => "Restring",
            Kind::HoleClearance => "Bohrungsabstand",
            Kind::Edge => "Abstand zum Platinenrand",
            Kind::OutsideBoard => "Außerhalb der Platine",
            Kind::Courtyard => "Bauteile überlappen",
            Kind::Unrouted => "Nicht verbunden",
            Kind::NoOutline => "Platinenumriss",
            Kind::Parity => "Schaltplan-Abgleich",
            Kind::Skew => "Längenunterschied",
        }
    }
    pub fn is_error(self) -> bool {
        !matches!(self, Kind::Courtyard | Kind::Parity | Kind::Skew)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    pub kind: Kind,
    pub msg: String,
    pub pos: P,
}

fn describe(b: &Board, c: &Copper) -> String {
    match c.what {
        Item::Pad(f, k) => format!("Pad {} von {}", b.footprints[f].fp.pads[k].num, b.footprints[f].reference),
        Item::Track(_) => format!("Leiterbahn [{}]", if c.net.is_empty() { "ohne Netz" } else { &c.net }),
        Item::Via(_) => format!("Via [{}]", if c.net.is_empty() { "ohne Netz" } else { &c.net }),
    }
}

fn mid_between(a: &Hull, b: &Hull) -> P {
    let mut best = (f64::MAX, P::ZERO);
    let ea: Vec<(P, P)> = edges(a);
    let eb: Vec<(P, P)> = edges(b);
    for (a0, a1) in &ea {
        for (b0, b1) in &eb {
            let d = seg_seg(*a0, *a1, *b0, *b1);
            if d < best.0 {
                let pa = super::geom::closest_on_seg((*b0 + *b1) / 2.0, *a0, *a1);
                let pb = super::geom::closest_on_seg(pa, *b0, *b1);
                best = (d, (pa + pb) / 2.0);
            }
        }
    }
    best.1
}

/// Gap between a track and other copper, ignoring the parts of the track inside pads of its
/// own net (None when neither item is a track).
fn exposed_gap(b: &Board, a: &Copper, c: &Copper) -> Option<f64> {
    let (t, o) = match (a.what, c.what) {
        (Item::Track(_), _) => (a, c),
        (_, Item::Track(_)) => (c, a),
        _ => return None,
    };
    let (p0, p1) = (t.hull.pts[0], t.hull.pts[1]);
    let own: Vec<Hull> = b.copper().into_iter().filter(|x| matches!(x.what, Item::Pad(..)) && x.net == t.net && x.layers & t.layers != 0).map(|x| x.hull).collect();
    if own.is_empty() {
        return None;
    }
    // sample the centre line; keep pieces outside own pads (shrunk by the track half width)
    let n = 40;
    let mut best = f64::MAX;
    let mut any = false;
    for i in 0..n {
        let (s0, s1) = (p0 + (p1 - p0) * (i as f64 / n as f64), p0 + (p1 - p0) * ((i + 1) as f64 / n as f64));
        let mid = (s0 + s1) / 2.0;
        if own.iter().any(|h| inside_by(h, mid, t.hull.r)) {
            continue;
        }
        any = true;
        best = best.min(Hull::capsule(s0, s1, t.hull.r).gap(&o.hull));
    }
    Some(if any { best } else { f64::MAX })
}

/// True when a disc of radius r at q lies completely inside the hull.
fn inside_by(h: &Hull, q: P, r: f64) -> bool {
    (0..8).all(|k| {
        let a = k as f64 * std::f64::consts::TAU / 8.0;
        h.contains(q + p(a.cos(), a.sin()) * r)
    })
}

fn edges(h: &Hull) -> Vec<(P, P)> {
    let n = h.pts.len();
    match n {
        1 => vec![(h.pts[0], h.pts[0])],
        2 => vec![(h.pts[0], h.pts[1])],
        _ => (0..n).map(|i| (h.pts[i], h.pts[(i + 1) % n])).collect(),
    }
}

/// Runs the check; `sch` adds the schematic parity test.
pub fn run(b: &Board, sch: Option<&Schematic>) -> Vec<Violation> {
    let r = &b.rules;
    let mut out = vec![];
    let mut add = |kind: Kind, msg: String, pos: P| out.push(Violation { kind, msg, pos });
    let outer = b.outer().cloned();
    if outer.is_none() {
        add(Kind::NoOutline, "Kein Platinenumriss (Edge.Cuts) vorhanden".into(), P::ZERO);
    }
    let cu = b.copper();
    let max_clear = r.classes.iter().map(|c| c.clearance).fold(r.clearance, f64::max);
    // ---- copper to copper
    let boxes: Vec<(P, P)> = cu.iter().map(|c| c.hull.bbox()).collect();
    let mut order: Vec<usize> = (0..cu.len()).collect();
    order.sort_by(|&a, &c| boxes[a].0.x.partial_cmp(&boxes[c].0.x).unwrap());
    for (oi, &i) in order.iter().enumerate() {
        for &j in &order[oi + 1..] {
            if boxes[j].0.x > boxes[i].1.x + max_clear {
                break;
            }
            let (a, c) = (&cu[i], &cu[j]);
            if a.layers & c.layers == 0 {
                continue;
            }
            let same = !a.net.is_empty() && a.net == c.net;
            if same {
                continue;
            }
            // pads of one footprint with the same number (e.g. shield) are one pad
            // pads of one footprint are not checked against each other (KiCad does the same:
            // the land pattern defines their spacing)
            if let (Item::Pad(f1, _), Item::Pad(f2, _)) = (a.what, c.what) {
                if f1 == f2 {
                    continue;
                }
            }
            if boxes[j].0.y > boxes[i].1.y + max_clear || boxes[i].0.y > boxes[j].1.y + max_clear {
                continue;
            }
            let gap = a.hull.gap(&c.hull);
            let need = r.clearance_between(&a.net, &c.net);
            // the piece of a track lying on its own pad is pad copper: measure from the exposed part
            let gap = if gap < need - 1e-6 { exposed_gap(b, a, c).unwrap_or(gap) } else { gap };
            if gap < need - 1e-6 {
                let pos = mid_between(&a.hull, &c.hull);
                if gap <= 1e-6 {
                    add(Kind::Short, format!("{} berührt {} (Netze {} / {})", describe(b, a), describe(b, c), show(&a.net), show(&c.net)), pos);
                } else {
                    add(Kind::Clearance, format!("{} – {}: Abstand {} mm < {} mm", describe(b, a), describe(b, c), super::geom::mm(gap), super::geom::mm(need)), pos);
                }
            }
        }
    }
    // ---- zones against foreign copper
    for z in &b.zones {
        for c in &cu {
            if c.net == z.net || c.layers & (1 << z.layer) == 0 {
                continue;
            }
            let probe = c.hull.polygon(16);
            if z.fill.iter().any(|s| probe.iter().any(|q| poly::in_shape(s, [q.x, q.y]))) {
                add(Kind::Short, format!("Zone {} ({}) überdeckt {} – Zonen neu füllen", z.net, super::board::layer_name(z.layer), describe(b, c)), super::board::center(&c.hull));
            }
        }
    }
    // ---- widths, drills, annular rings
    for t in &b.tracks {
        if t.width < r.min_track - 1e-9 {
            add(Kind::TrackWidth, format!("Leiterbahn {} mm schmaler als {} mm", super::geom::mm(t.width), super::geom::mm(r.min_track)), (t.a + t.b) / 2.0);
        }
    }
    let mut holes: Vec<(Hull, String)> = vec![];
    for v in &b.vias {
        if v.drill < r.min_drill - 1e-9 {
            add(Kind::Drill, format!("Via-Bohrung {} mm kleiner als {} mm", super::geom::mm(v.drill), super::geom::mm(r.min_drill)), v.pos);
        }
        if (v.dia - v.drill) / 2.0 < r.min_annular - 1e-9 {
            add(Kind::Annular, format!("Via-Restring {} mm kleiner als {} mm", super::geom::mm((v.dia - v.drill) / 2.0), super::geom::mm(r.min_annular)), v.pos);
        }
        holes.push((Hull::disc(v.pos, v.drill / 2.0), format!("Via [{}]", v.net)));
    }
    for f in &b.footprints {
        for (k, pad) in f.fp.pads.iter().enumerate() {
            if pad.drill <= 0.0 {
                continue;
            }
            let at = f.pad_pos(k);
            if !pad.npth {
                let ring = (pad.size.x.min(pad.size.y) - pad.drill) / 2.0;
                if ring < r.min_annular - 1e-9 {
                    add(Kind::Annular, format!("Pad {} von {}: Restring {} mm", pad.num, f.reference, super::geom::mm(ring)), at);
                }
            }
            let (ha, he) = pad.hole_ends(f.pos, f.rot, f.bottom);
            holes.push((Hull::capsule(ha, he, pad.drill / 2.0), format!("{} Pad {} von {}", if pad.slot > pad.drill { "Langloch" } else { "Bohrung" }, pad.num, f.reference)));
        }
    }
    for i in 0..holes.len() {
        for j in i + 1..holes.len() {
            let (ci, cj) = (super::board::center(&holes[i].0), super::board::center(&holes[j].0));
            let d = holes[i].0.gap(&holes[j].0);
            if d < r.hole_clearance - 1e-6 && (ci - cj).length() > 1e-6 {
                add(Kind::HoleClearance, format!("{} – {}: {} mm", holes[i].1, holes[j].1, super::geom::mm(d.max(0.0))), (ci + cj) / 2.0);
            }
        }
    }
    // ---- board edge
    if let Some(outer) = &outer {
        let edges: Vec<(P, P)> = b.outline.iter().flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()]))).collect();
        for c in &cu {
            let ctr = super::board::center(&c.hull);
            let inside = in_poly(ctr, outer) && !b.outline.iter().filter(|l| *l != outer).any(|l| in_poly(ctr, l));
            if !inside {
                add(Kind::OutsideBoard, format!("{} liegt außerhalb der Platine", describe(b, c)), ctr);
                continue;
            }
            let d = edges.iter().map(|(a, e)| Hull::capsule(*a, *e, 0.0).gap(&c.hull)).fold(f64::MAX, f64::min);
            if d < r.edge_clearance - 1e-6 {
                add(Kind::Edge, format!("{}: {} mm zum Platinenrand (mind. {} mm)", describe(b, c), super::geom::mm(d.max(0.0)), super::geom::mm(r.edge_clearance)), ctr);
            }
        }
        for f in &b.footprints {
            if !in_poly(f.pos, outer) {
                add(Kind::OutsideBoard, format!("Bauteil {} liegt außerhalb der Platine", f.reference), f.pos);
            }
        }
    }
    // ---- courtyards
    for i in 0..b.footprints.len() {
        for j in i + 1..b.footprints.len() {
            let (a, c) = (&b.footprints[i], &b.footprints[j]);
            if a.bottom != c.bottom && a.fp.smd() && c.fp.smd() {
                continue;
            }
            let (pa, pc) = (a.courtyard(), c.courtyard());
            if (Hull { pts: pa.clone(), r: 0.0 }).gap(&Hull { pts: pc.clone(), r: 0.0 }) < -1e-6 {
                add(Kind::Courtyard, format!("Sperrflächen von {} und {} überlappen", a.reference, c.reference), (a.pos + c.pos) / 2.0);
            }
        }
    }
    // ---- differential pairs: length difference
    let mut done: Vec<String> = vec![];
    for net in b.nets() {
        if done.contains(&net) {
            continue;
        }
        if let Some(other) = super::board::diff_partner(&net) {
            if b.nets().contains(&other) {
                done.push(other.clone());
                let (la, lb) = (b.track_length(&net), b.track_length(&other));
                if la > 0.0 && lb > 0.0 && (la - lb).abs() > r.max_skew {
                    add(Kind::Skew, format!("Differenzielles Paar {} / {}: Längen {} / {} mm (Unterschied > {} mm)", net, other, super::geom::mm((la * 100.0).round() / 100.0), super::geom::mm((lb * 100.0).round() / 100.0), super::geom::mm(r.max_skew)), P::ZERO);
                }
            }
        }
    }
    // ---- unrouted
    for (a, e, net) in b.ratsnest() {
        add(Kind::Unrouted, format!("Netz {}: Verbindung fehlt ({:.2} mm)", net, (a - e).length()), (a + e) / 2.0);
    }
    // ---- schematic parity
    if let Some(s) = sch {
        let flat = s.flatten();
        let s = &flat;
        let nl = s.netlist();
        let mut seen: Vec<&str> = vec![];
        for (i, sym) in s.symbols.iter().enumerate() {
            if s.is_power(i) || seen.contains(&sym.reference.as_str()) {
                continue;
            }
            seen.push(&sym.reference);
            match b.find_fp(&sym.reference) {
                None => add(Kind::Parity, format!("{} fehlt auf der Platine – Platine aus Schaltplan aktualisieren", sym.reference), P::ZERO),
                Some(fi) => {
                    let f = &b.footprints[fi];
                    for (k, pad) in f.fp.pads.iter().enumerate() {
                        let want = nl.net_of(&sym.reference, &pad.num).unwrap_or("");
                        if !pad.npth && f.nets.get(k).map(|s| s.as_str()).unwrap_or("") != want {
                            add(Kind::Parity, format!("Pad {} von {}: Netz {} statt {}", pad.num, f.reference, show(f.nets.get(k).map(|s| s.as_str()).unwrap_or("")), show(want)), f.pad_pos(k));
                        }
                    }
                }
            }
        }
        for f in &b.footprints {
            if !s.symbols.iter().any(|x| x.reference == f.reference) {
                add(Kind::Parity, format!("{} ist nicht im Schaltplan", f.reference), f.pos);
            }
        }
    }
    let _ = (all_layers, pt_seg, p);
    out
}

fn show(n: &str) -> &str {
    if n.is_empty() {
        "ohne Netz"
    } else {
        n
    }
}

pub fn errors(v: &[Violation]) -> usize {
    v.iter().filter(|x| x.kind.is_error()).count()
}
