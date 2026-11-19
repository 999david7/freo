//! Board: outline, placed footprints, tracks, vias and copper zones; netlist import from the
//! schematic, copper connectivity, ratsnest and zone filling.

use super::footprints::{self, Footprint, Pad};
use super::geom::{bbox, p, pt_seg, rot, Hull, P};
use super::schematic::{natural, Netlist, Schematic};
use crate::slicer::poly::{self, Shapes};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Copper layers: 0 = F.Cu (oben), 1 = B.Cu (unten), 2 = In1.Cu, 3 = In2.Cu.
pub const F_CU: u8 = 0;
pub const B_CU: u8 = 1;

pub fn layer_name(l: u8) -> &'static str {
    match l {
        0 => "F.Cu",
        1 => "B.Cu",
        2 => "In1.Cu",
        3 => "In2.Cu",
        _ => "?",
    }
}

pub fn layer_title(l: u8) -> &'static str {
    match l {
        0 => "Oben (F.Cu)",
        1 => "Unten (B.Cu)",
        2 => "Innen 1 (In1.Cu)",
        3 => "Innen 2 (In2.Cu)",
        _ => "?",
    }
}

pub fn layer_from_name(s: &str) -> Option<u8> {
    match s {
        "F.Cu" => Some(0),
        "B.Cu" => Some(1),
        "In1.Cu" => Some(2),
        "In2.Cu" => Some(3),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FpInst {
    pub reference: String,
    pub value: String,
    pub fp: Footprint,
    pub pos: P,
    pub rot: f64,
    /// placed on the bottom side (mirrored)
    pub bottom: bool,
    /// net of each pad (same order as `fp.pads`)
    pub nets: Vec<String>,
    #[serde(default)]
    pub locked: bool,
}

impl FpInst {
    pub fn xform(&self, q: P) -> P {
        let q = if self.bottom { p(-q.x, q.y) } else { q };
        self.pos + rot(q, self.rot)
    }
    pub fn pad_pos(&self, k: usize) -> P {
        self.xform(self.fp.pads[k].pos)
    }
    pub fn pad_hull(&self, k: usize) -> Hull {
        self.fp.pads[k].hull(self.pos, self.rot, self.bottom)
    }
    /// Copper layers of pad k as a bit mask.
    pub fn pad_layers(&self, k: usize, layers: u8) -> u8 {
        let pad = &self.fp.pads[k];
        if pad.npth {
            0
        } else if pad.smd() {
            1 << if self.bottom { B_CU } else { F_CU }
        } else {
            all_layers(layers)
        }
    }
    pub fn courtyard(&self) -> Vec<P> {
        let (lo, hi) = self.fp.court;
        [lo, p(hi.x, lo.y), hi, p(lo.x, hi.y)].iter().map(|q| self.xform(*q)).collect()
    }
    pub fn side(&self) -> u8 {
        if self.bottom {
            B_CU
        } else {
            F_CU
        }
    }
}

pub fn all_layers(layers: u8) -> u8 {
    if layers >= 4 {
        0b1111
    } else {
        0b11
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub a: P,
    pub b: P,
    pub width: f64,
    pub layer: u8,
    pub net: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Via {
    pub pos: P,
    pub dia: f64,
    pub drill: f64,
    pub net: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub net: String,
    pub layer: u8,
    pub outline: Vec<P>,
    pub clearance: f64,
    pub min_width: f64,
    /// thermal relief spokes to pads of the zone's net
    pub thermal: bool,
    #[serde(default)]
    pub fill: Vec<Vec<Vec<[f64; 2]>>>,
    /// keep the fill as read (KiCad teardrops): not refilled
    #[serde(default)]
    pub fixed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    pub clearance: f64,
    pub track: f64,
    pub power_track: f64,
    pub via_dia: f64,
    pub via_drill: f64,
    pub min_track: f64,
    pub min_drill: f64,
    pub min_annular: f64,
    pub edge_clearance: f64,
    pub hole_clearance: f64,
    /// net classes (KiCad: Netzklassen); the first one whose pattern matches a net wins,
    /// nets without a match use the "Default" values above
    #[serde(default = "default_classes")]
    pub classes: Vec<NetClass>,
    /// allowed length difference inside a differential pair
    #[serde(default = "d_skew")]
    pub max_skew: f64,
}

fn d_skew() -> f64 {
    1.0
}

/// A net class: rules for the nets matching its patterns ("GND", "+*", "USB_D*").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetClass {
    pub name: String,
    pub patterns: Vec<String>,
    pub clearance: f64,
    pub track: f64,
    pub via_dia: f64,
    pub via_drill: f64,
    /// differential pairs: track width and gap between the two tracks
    pub diff_width: f64,
    pub diff_gap: f64,
}

pub fn default_classes() -> Vec<NetClass> {
    vec![
        NetClass { name: "Versorgung".into(), patterns: vec!["GND*".into(), "+*".into(), "VCC*".into(), "VBUS*".into(), "VIN*".into(), "-*".into()], clearance: 0.2, track: 0.5, via_dia: 0.8, via_drill: 0.4, diff_width: 0.5, diff_gap: 0.25 },
        NetClass { name: "USB".into(), patterns: vec!["USB_D*".into(), "D+".into(), "D-".into(), "*_P".into(), "*_N".into()], clearance: 0.2, track: 0.3, via_dia: 0.6, via_drill: 0.3, diff_width: 0.3, diff_gap: 0.2 },
    ]
}

/// `*` matches any text (anywhere in the pattern), case-insensitive.
pub fn pattern_match(pat: &str, net: &str) -> bool {
    let (p, n) = (pat.to_uppercase(), net.to_uppercase());
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == n;
    }
    let mut rest = n.as_str();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !rest.starts_with(part) {
                return false;
            }
            rest = &rest[part.len()..];
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(k) => rest = &rest[k + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// Resolved rules for one net.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassRules {
    pub name: String,
    pub clearance: f64,
    pub track: f64,
    pub via_dia: f64,
    pub via_drill: f64,
    pub diff_width: f64,
    pub diff_gap: f64,
}

impl Rules {
    pub fn class_of(&self, net: &str) -> ClassRules {
        if !net.is_empty() {
            for c in &self.classes {
                if c.patterns.iter().any(|p| pattern_match(p, net)) {
                    return ClassRules { name: c.name.clone(), clearance: c.clearance.max(self.clearance), track: c.track, via_dia: c.via_dia, via_drill: c.via_drill, diff_width: c.diff_width, diff_gap: c.diff_gap };
                }
            }
        }
        ClassRules { name: "Default".into(), clearance: self.clearance, track: self.track, via_dia: self.via_dia, via_drill: self.via_drill, diff_width: self.track, diff_gap: self.clearance }
    }
    /// Clearance required between copper of two nets (the larger class value).
    pub fn clearance_between(&self, a: &str, b: &str) -> f64 {
        self.class_of(a).clearance.max(self.class_of(b).clearance)
    }
}

/// The partner of a differential-pair net (USB_D+ ↔ USB_D-, X_P ↔ X_N), if any.
pub fn diff_partner(net: &str) -> Option<String> {
    for (a, b) in [("_P", "_N"), ("_N", "_P"), ("+", "-"), ("-", "+"), ("P", "N"), ("N", "P")] {
        if let Some(stem) = net.strip_suffix(a) {
            if !stem.is_empty() && (a.len() > 1 || stem.ends_with('_') || stem.ends_with("D")) {
                return Some(format!("{}{}", stem, b));
            }
        }
    }
    None
}

impl Default for Rules {
    fn default() -> Self {
        Rules { clearance: 0.2, track: 0.25, power_track: 0.5, via_dia: 0.8, via_drill: 0.4, min_track: 0.15, min_drill: 0.3, min_annular: 0.13, edge_clearance: 0.3, hole_clearance: 0.25, classes: default_classes(), max_skew: 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Board {
    /// closed outlines on Edge.Cuts: the largest is the board, the others are cut-outs
    pub outline: Vec<Vec<P>>,
    pub footprints: Vec<FpInst>,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub zones: Vec<Zone>,
    pub rules: Rules,
    /// number of copper layers (2 or 4)
    pub layers: u8,
    pub thickness: f64,
    /// silkscreen texts (F.SilkS)
    #[serde(default)]
    pub texts: Vec<(P, String, f64)>,
}

impl Default for Board {
    fn default() -> Self {
        Board { outline: vec![], footprints: vec![], tracks: vec![], vias: vec![], zones: vec![], rules: Rules::default(), layers: 2, thickness: 1.6, texts: vec![] }
    }
}

/// A piece of copper for clearance and connectivity checks.
#[derive(Clone, Debug)]
pub struct Copper {
    pub hull: Hull,
    pub layers: u8,
    pub net: String,
    pub what: Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Pad(usize, usize),
    Track(usize),
    Via(usize),
}

#[derive(Clone, Debug, Default)]
pub struct UpdateReport {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    pub missing_fp: Vec<String>,
}

impl Board {
    pub fn rect_outline(&mut self, x0: f64, y0: f64, w: f64, h: f64) {
        self.outline = vec![vec![p(x0, y0), p(x0 + w, y0), p(x0 + w, y0 + h), p(x0, y0 + h)]];
    }

    /// The board outline (largest loop).
    pub fn outer(&self) -> Option<&Vec<P>> {
        self.outline.iter().max_by(|a, b| super::geom::area(a).abs().partial_cmp(&super::geom::area(b).abs()).unwrap())
    }

    pub fn region(&self) -> Shapes {
        let outer = match self.outer() {
            Some(o) => o,
            None => return vec![],
        };
        let mut s = poly::from_loops(&[to_c(outer)]);
        for l in &self.outline {
            if !std::ptr::eq(l, outer) {
                s = poly::diff(&s, &poly::from_loops(&[to_c(l)]));
            }
        }
        s
    }

    pub fn size(&self) -> Option<(P, P)> {
        bbox(self.outline.iter().flatten().copied())
    }

    /// Brings footprints and pad nets in line with the schematic (KiCad: "Platine aus
    /// Schaltplan aktualisieren"). New parts are lined up below the board.
    pub fn update_from(&mut self, sch: &Schematic) -> UpdateReport {
        let flat = sch.flatten();
        let sch = &flat;
        let net = sch.netlist();
        let mut rep = UpdateReport::default();
        let mut wanted: Vec<String> = vec![];
        for (i, s) in sch.symbols.iter().enumerate() {
            // one footprint per part, also for multi-unit parts placed several times
            if sch.is_power(i) || s.reference.ends_with('?') || wanted.contains(&s.reference) {
                continue;
            }
            wanted.push(s.reference.clone());
            let Some(fp) = footprints::find(&s.footprint).cloned() else {
                rep.missing_fp.push(format!("{} ({})", s.reference, if s.footprint.is_empty() { "kein Footprint" } else { &s.footprint }));
                continue;
            };
            match self.footprints.iter_mut().find(|f| f.reference == s.reference) {
                Some(f) => {
                    let mut changed = false;
                    if f.fp.id != fp.id {
                        f.fp = fp;
                        changed = true;
                    }
                    if f.value != s.value {
                        f.value = s.value.clone();
                        changed = true;
                    }
                    let nets = pad_nets(&f.fp, &s.reference, &net);
                    if f.nets != nets {
                        f.nets = nets;
                        changed = true;
                    }
                    if changed {
                        rep.changed.push(s.reference.clone());
                    }
                }
                None => {
                    let nets = pad_nets(&fp, &s.reference, &net);
                    self.footprints.push(FpInst { reference: s.reference.clone(), value: s.value.clone(), fp, pos: P::ZERO, rot: 0.0, bottom: false, nets, locked: false });
                    rep.added.push(s.reference.clone());
                }
            }
        }
        let before = self.footprints.len();
        let mut removed = vec![];
        self.footprints.retain(|f| {
            let keep = wanted.contains(&f.reference);
            if !keep {
                removed.push(f.reference.clone());
            }
            keep
        });
        rep.removed = removed;
        let _ = before;
        // line up the new parts below the board (or at the origin without one)
        if !rep.added.is_empty() {
            let (lo, hi) = self.size().unwrap_or((P::ZERO, p(50.0, 0.0)));
            let mut x = lo.x;
            let mut y = hi.y + 8.0;
            let mut row_h: f64 = 0.0;
            let mut added: Vec<usize> = (0..self.footprints.len()).filter(|&i| rep.added.contains(&self.footprints[i].reference)).collect();
            added.sort_by(|&a, &b| natural(&self.footprints[a].reference).cmp(&natural(&self.footprints[b].reference)));
            for i in added {
                let f = &mut self.footprints[i];
                let (clo, chi) = f.fp.court;
                let w = chi.x - clo.x;
                if x + w > lo.x + (hi.x - lo.x).max(60.0) && x > lo.x {
                    x = lo.x;
                    y += row_h + 2.0;
                    row_h = 0.0;
                }
                f.pos = p(x - clo.x, y - clo.y);
                x += w + 2.0;
                row_h = row_h.max(chi.y - clo.y);
            }
        }
        rep.added.sort_by_key(|a| natural(a));
        rep
    }

    pub fn nets(&self) -> Vec<String> {
        let mut v: Vec<String> = self.footprints.iter().flat_map(|f| f.nets.iter().cloned()).filter(|n| !n.is_empty()).collect();
        v.sort_by_key(|a| natural(a));
        v.dedup();
        v
    }

    pub fn copper(&self) -> Vec<Copper> {
        let mut out = vec![];
        for (fi, f) in self.footprints.iter().enumerate() {
            for k in 0..f.fp.pads.len() {
                let layers = f.pad_layers(k, self.layers);
                if layers == 0 {
                    continue;
                }
                out.push(Copper { hull: f.pad_hull(k), layers, net: f.nets.get(k).cloned().unwrap_or_default(), what: Item::Pad(fi, k) });
            }
        }
        for (i, t) in self.tracks.iter().enumerate() {
            out.push(Copper { hull: Hull::capsule(t.a, t.b, t.width / 2.0), layers: 1 << t.layer, net: t.net.clone(), what: Item::Track(i) });
        }
        for (i, v) in self.vias.iter().enumerate() {
            out.push(Copper { hull: Hull::disc(v.pos, v.dia / 2.0), layers: all_layers(self.layers), net: v.net.clone(), what: Item::Via(i) });
        }
        out
    }

    /// Groups of same-net copper that touch (pads, tracks, vias and filled zones).
    pub fn clusters(&self, cu: &[Copper]) -> Vec<usize> {
        let n = cu.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while p[r] != r {
                r = p[r];
            }
            let mut k = i;
            while p[k] != r {
                let nx = p[k];
                p[k] = r;
                k = nx;
            }
            r
        }
        let boxes: Vec<(P, P)> = cu.iter().map(|c| c.hull.bbox()).collect();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| boxes[a].0.x.partial_cmp(&boxes[b].0.x).unwrap());
        for (oi, &i) in order.iter().enumerate() {
            for &j in &order[oi + 1..] {
                if boxes[j].0.x > boxes[i].1.x + 1e-6 {
                    break;
                }
                if cu[i].net != cu[j].net || cu[i].net.is_empty() || cu[i].layers & cu[j].layers == 0 {
                    continue;
                }
                if boxes[j].0.y > boxes[i].1.y + 1e-6 || boxes[i].0.y > boxes[j].1.y + 1e-6 {
                    continue;
                }
                if cu[i].hull.gap(&cu[j].hull) <= 1e-6 {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
        // zones join everything of their net they touch on their layer
        for z in &self.zones {
            if z.fill.is_empty() {
                continue;
            }
            for island in &z.fill {
                let hits: Vec<usize> = (0..n).filter(|&i| cu[i].net == z.net && cu[i].layers & (1 << z.layer) != 0 && touches_island(&cu[i].hull, island)).collect();
                for w in hits.windows(2) {
                    let (a, b) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
        (0..n).map(|i| find(&mut parent, i)).collect()
    }

    /// Unrouted connections: per net, a minimum spanning tree between the copper clusters
    /// that are not yet joined (KiCad's ratsnest).
    pub fn ratsnest(&self) -> Vec<(P, P, String)> {
        let cu = self.copper();
        let cl = self.clusters(&cu);
        let mut by_net: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, c) in cu.iter().enumerate() {
            if !c.net.is_empty() && matches!(c.what, Item::Pad(..)) {
                by_net.entry(&c.net).or_default().push(i);
            }
        }
        let mut out = vec![];
        let mut nets: Vec<&&str> = by_net.keys().collect();
        nets.sort();
        for net in nets {
            let pads = &by_net[*net];
            // representative points per cluster: pads plus track ends and vias of the net
            let mut groups: HashMap<usize, Vec<P>> = HashMap::new();
            for &i in pads {
                groups.entry(cl[i]).or_default().push(center(&cu[i].hull));
            }
            if groups.len() < 2 {
                continue;
            }
            for (i, c) in cu.iter().enumerate() {
                if c.net == **net && !matches!(c.what, Item::Pad(..)) {
                    if let Some(g) = groups.get_mut(&cl[i]) {
                        g.extend(c.hull.pts.iter().copied());
                    }
                }
            }
            let mut keys: Vec<usize> = groups.keys().copied().collect();
            keys.sort();
            let pts: Vec<&Vec<P>> = keys.iter().map(|k| &groups[k]).collect();
            // Prim's algorithm on cluster distances
            let m = pts.len();
            let mut inside = vec![false; m];
            inside[0] = true;
            for _ in 1..m {
                let mut best = (f64::MAX, P::ZERO, P::ZERO, 0);
                for a in 0..m {
                    if !inside[a] {
                        continue;
                    }
                    for b in 0..m {
                        if inside[b] {
                            continue;
                        }
                        for pa in pts[a] {
                            for pb in pts[b] {
                                let d = (*pa - *pb).length_squared();
                                if d < best.0 {
                                    best = (d, *pa, *pb, b);
                                }
                            }
                        }
                    }
                }
                inside[best.3] = true;
                out.push((best.1, best.2, net.to_string()));
            }
        }
        out
    }

    /// Fills every zone: zone outline ∩ board (inset by the edge clearance) minus all copper
    /// of other nets grown by the clearance; pads of the zone's own net get thermal reliefs;
    /// islands without a connection to the net are removed.
    pub fn fill_zones(&mut self) {
        let cu = self.copper();
        let board = poly::offset(&self.region(), -self.rules.edge_clearance);
        let mut done: Vec<(u8, String, Shapes)> = vec![];
        for zi in 0..self.zones.len() {
            let z = self.zones[zi].clone();
            if z.fixed {
                continue;
            }
            let mut area = poly::inter(&poly::from_loops(&[to_c(&z.outline)]), &board);
            let clear = z.clearance.max(self.rules.clearance);
            let mut obstacles: Vec<Vec<[f64; 2]>> = vec![];
            let mut rings: Vec<Vec<[f64; 2]>> = vec![];
            for c in &cu {
                let on = c.layers & (1 << z.layer) != 0;
                if c.net != z.net || c.net.is_empty() {
                    if on {
                        let cl = clear.max(self.rules.clearance_between(&z.net, &c.net));
                        obstacles.push(to_c(&Hull { pts: c.hull.pts.clone(), r: c.hull.r + cl }.polygon(24)));
                    }
                } else if on && z.thermal && matches!(c.what, Item::Pad(..)) {
                    rings.push(to_c(&Hull { pts: c.hull.pts.clone(), r: c.hull.r + 0.5 }.polygon(24)));
                }
            }
            // holes of other nets on any layer (also NPTH)
            for f in &self.footprints {
                for (k, pad) in f.fp.pads.iter().enumerate() {
                    if pad.drill > 0.0 && (pad.npth || f.nets.get(k).is_none_or(|n| n != &z.net)) {
                        let (ha, he) = pad.hole_ends(f.pos, f.rot, f.bottom);
                        obstacles.push(to_c(&Hull::capsule(ha, he, pad.drill / 2.0 + self.rules.hole_clearance.max(clear)).polygon(24)));
                    }
                }
            }
            // earlier zones of other nets on the same layer keep their copper
            for (l, n, s) in &done {
                if *l == z.layer && *n != z.net {
                    for c in poly::offset(s, clear) {
                        obstacles.extend(c);
                    }
                }
            }
            let obst = poly::from_loops(&obstacles);
            area = poly::diff(&poly::diff(&area, &obst), &poly::from_loops(&rings));
            if z.thermal {
                let mut spokes = vec![];
                for c in &cu {
                    if c.net == z.net && !c.net.is_empty() && c.layers & (1 << z.layer) != 0 {
                        if let Item::Pad(..) = c.what {
                            let (lo, hi) = c.hull.bbox();
                            let ctr = (lo + hi) / 2.0;
                            let (rx, ry) = ((hi.x - lo.x) / 2.0 + 0.6, (hi.y - lo.y) / 2.0 + 0.6);
                            let w = (z.min_width * 2.0).max(0.4) / 2.0;
                            spokes.push(to_c(&[ctr + p(-rx, -w), ctr + p(rx, -w), ctr + p(rx, w), ctr + p(-rx, w)]));
                            spokes.push(to_c(&[ctr + p(-w, -ry), ctr + p(w, -ry), ctr + p(w, ry), ctr + p(-w, ry)]));
                        }
                    }
                }
                // spokes stay clear of other copper
                let sp = poly::diff(&poly::inter(&poly::from_loops(&spokes), &poly::inter(&poly::from_loops(&[to_c(&z.outline)]), &board)), &obst);
                area = poly::union(&area, &sp);
            }
            if z.min_width > 0.0 {
                area = poly::open(&area, z.min_width / 2.0);
            }
            // keep islands that touch copper of the zone's net
            let own: Vec<&Copper> = cu.iter().filter(|c| c.net == z.net && !c.net.is_empty() && c.layers & (1 << z.layer) != 0).collect();
            area.retain(|island| own.iter().any(|c| touches_island(&c.hull, island)));
            done.push((z.layer, z.net.clone(), area.clone()));
            self.zones[zi].fill = area;
        }
    }

    pub fn find_fp(&self, reference: &str) -> Option<usize> {
        self.footprints.iter().position(|f| f.reference == reference)
    }

    /// Net of the copper under `q` on `layer` (pads, tracks, vias, zones).
    pub fn net_at(&self, q: P, layer: u8) -> Option<String> {
        for c in self.copper() {
            if c.layers & (1 << layer) != 0 && c.hull.contains(q) && !c.net.is_empty() {
                return Some(c.net);
            }
        }
        for z in &self.zones {
            if z.layer == layer && z.fill.iter().any(|s| poly::in_shape(s, [q.x, q.y])) {
                return Some(z.net.clone());
            }
        }
        None
    }

    /// Removes zero-length tracks and merges collinear continuations of the same net.
    pub fn tidy(&mut self) {
        self.tracks.retain(|t| (t.a - t.b).length() > 1e-6);
    }

    pub fn track_length(&self, net: &str) -> f64 {
        self.tracks.iter().filter(|t| t.net == net).map(|t| (t.a - t.b).length()).sum()
    }

    /// Takes the board outline from a closed sketch loop (sketch coordinates are y-up; the
    /// board is y-down).
    pub fn outline_from_loops(&mut self, loops: &[Vec<P>]) {
        self.outline = loops.iter().map(|l| l.iter().map(|q| p(q.x, -q.y)).collect()).collect();
    }
}

fn pad_nets(fp: &Footprint, reference: &str, net: &Netlist) -> Vec<String> {
    fp.pads.iter().map(|q: &Pad| if q.npth || q.num.is_empty() { String::new() } else { net.net_of(reference, &q.num).unwrap_or("").to_string() }).collect()
}

pub fn center(h: &Hull) -> P {
    let n = h.pts.len().max(1) as f64;
    h.pts.iter().copied().sum::<P>() / n
}

pub fn to_c(pts: &[P]) -> Vec<[f64; 2]> {
    pts.iter().map(|q| [q.x, q.y]).collect()
}

fn touches_island(h: &Hull, island: &[Vec<[f64; 2]>]) -> bool {
    let probe: Vec<P> = std::iter::once(center(h)).chain(h.polygon(12)).collect();
    if probe.iter().any(|q| poly::in_shape(&island.to_vec(), [q.x, q.y])) {
        return true;
    }
    // or an island edge passes within the shape
    island.iter().any(|c| c.iter().any(|q| h.contains(p(q[0], q[1]))) || c.windows(2).any(|w| h.pts.iter().any(|k| pt_seg(*k, p(w[0][0], w[0][1]), p(w[1][0], w[1][1])) <= h.r + 1e-6)))
}
