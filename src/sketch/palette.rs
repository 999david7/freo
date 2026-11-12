//! Skizzierer-Palette: the predefined shapes of Creo's sketcher palette (Polygone, Profile,
//! Formen, Sterne) plus the school library tabs (Allgemeines, HTL, steel sections).
//! Every shape is a small sketch around the origin, which is the placement point.

use super::sketch::*;
use crate::kernel::math::*;
use std::sync::OnceLock;

pub struct PaletteItem {
    pub name: String,
    pub sketch: Sketch,
    /// true: drawn in real millimetres (steel sections); false: scaled to the view when placed
    pub real: bool,
}

pub struct PaletteTab {
    pub name: &'static str,
    pub items: Vec<PaletteItem>,
}

/// The tabs in the order of Creo's dialog, three per row.
pub fn tabs() -> &'static [PaletteTab] {
    static TABS: OnceLock<Vec<PaletteTab>> = OnceLock::new();
    TABS.get_or_init(build)
}

impl PaletteItem {
    /// Bounding box of the profile geometry (centerlines and construction geometry left out).
    pub fn bounds(&self, all: bool) -> (V2, V2) {
        let (mut mn, mut mx) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
        for e in self.sketch.entities.iter().filter(|e| all || !(e.centerline || e.construction)) {
            for q in self.sketch.sample(e, 48) { mn = mn.min(q); mx = mx.max(q); }
        }
        (mn, mx)
    }

    /// Largest extent of the profile geometry.
    pub fn extent(&self) -> f64 {
        let (mn, mx) = self.bounds(false);
        (mx.x - mn.x).max(mx.y - mn.y).max(1e-9)
    }

    /// The shape scaled about its placement point.
    pub fn scaled(&self, k: f64) -> Sketch {
        let mut s = self.sketch.clone();
        for p in &mut s.points { p.p = p.p * k; }
        for e in &mut s.entities { e.r *= k; }
        s
    }
}

struct B {
    sk: Sketch,
    pts: Vec<(V2, i64)>,
}

fn v(x: f64, y: f64) -> V2 { V2::new(x, y) }
fn polar(c: V2, r: f64, deg: f64) -> V2 { let t = deg.to_radians(); c + V2::new(t.cos(), t.sin()) * r }

impl B {
    fn new() -> B { B { sk: Sketch::default(), pts: vec![] } }

    /// Profile point; points at the same place are shared so chains stay connected.
    fn p(&mut self, q: V2) -> i64 {
        if let Some(f) = self.pts.iter().find(|f| (f.0 - q).length() < 1e-6) { return f.1; }
        let id = self.sk.add_point(q);
        self.pts.push((q, id));
        id
    }

    fn line(&mut self, a: V2, b: V2) -> i64 { let (ia, ib) = (self.p(a), self.p(b)); self.sk.add_line(ia, ib) }

    fn chain(&mut self, pts: &[V2], closed: bool) -> Vec<i64> {
        let n = pts.len();
        let m = if closed { n } else { n - 1 };
        (0..m).map(|i| self.line(pts[i], pts[(i + 1) % n])).collect()
    }

    /// Counter-clockwise arc from angle `a0` to `a1` (degrees).
    fn arc(&mut self, c: V2, r: f64, a0: f64, a1: f64) -> i64 {
        let ic = self.sk.add_point(c);
        let (s, e) = (self.p(polar(c, r, a0)), self.p(polar(c, r, a1)));
        self.sk.add_arc(ic, s, e)
    }

    fn circle(&mut self, c: V2, r: f64, construction: bool) -> i64 {
        let ic = self.sk.add_point(c);
        self.sk.add_entity(SkKind::Circle, vec![ic], r, construction, false)
    }

    fn centerline(&mut self, a: V2, b: V2) -> i64 {
        let (ia, ib) = (self.sk.add_point(a), self.sk.add_point(b));
        let id = self.sk.add_entity(SkKind::Line, vec![ia, ib], 0.0, true, true);
        let d = b - a;
        if d.y.abs() < 1e-9 { self.con(SkCType::Horizontal, vec![], vec![id]); } else if d.x.abs() < 1e-9 { self.con(SkCType::Vertical, vec![], vec![id]); }
        id
    }

    /// Horizontal and/or vertical centerline through the placement point.
    fn axes(&mut self, hx: f64, hy: f64) {
        if hx > 0.0 { self.centerline(v(-hx, 0.0), v(hx, 0.0)); }
        if hy > 0.0 { self.centerline(v(0.0, -hy), v(0.0, hy)); }
    }

    fn con(&mut self, ty: SkCType, pts: Vec<i64>, ents: Vec<i64>) { self.sk.add_constraint(SkConstraint::new(ty, pts, ents)); }

    fn done(self, name: &str, real: bool) -> PaletteItem { PaletteItem { name: name.to_string(), sketch: self.sk, real } }
}

/// Regular polygon inscribed in a construction circle, sides equal (as Creo's palette polygons).
fn polygon_into(b: &mut B, n: usize) {
    let r = 10.0;
    let a0 = if n % 2 == 1 { 90.0 } else { 90.0 + 180.0 / n as f64 };
    let circ = b.circle(V2::ZERO, r, true);
    let pts: Vec<V2> = (0..n).map(|k| polar(V2::ZERO, r, a0 + 360.0 * k as f64 / n as f64)).collect();
    let ids: Vec<i64> = pts.iter().map(|q| b.p(*q)).collect();
    let lines = b.chain(&pts, true);
    for id in ids { b.con(SkCType::PointOn, vec![id], vec![circ]); }
    for k in 1..n { b.con(SkCType::Equal, vec![], vec![lines[0], lines[k]]); }
}

fn polygon(name: &str, n: usize) -> PaletteItem {
    let mut b = B::new();
    polygon_into(&mut b, n);
    b.done(name, false)
}

fn star(name: &str, n: usize) -> PaletteItem {
    let mut b = B::new();
    let ro = 10.0;
    // the regular star {n/2}; three and four tips have no such star and get slim tips
    let ri = match n { 3 => 2.5, 4 => 3.0, _ => ro * (360.0 / n as f64).to_radians().cos() / (180.0 / n as f64).to_radians().cos() };
    let pts: Vec<V2> = (0..2 * n).map(|k| polar(V2::ZERO, if k % 2 == 0 { ro } else { ri }, 90.0 + 180.0 * k as f64 / n as f64)).collect();
    b.chain(&pts, true);
    b.axes(0.0, 13.0);
    b.done(name, false)
}

/// Slot: two lines and two half circles.
fn racetrack(name: &str) -> PaletteItem {
    let mut b = B::new();
    let (a, r) = (14.0, 6.0);
    b.line(v(-a, -r), v(a, -r));
    b.arc(v(a, 0.0), r, -90.0, 90.0);
    b.line(v(a, r), v(-a, r));
    b.arc(v(-a, 0.0), r, 90.0, 270.0);
    b.axes(26.0, 12.0);
    b.done(name, false)
}

/// Slot along an arc.
fn curved_racetrack(name: &str) -> PaletteItem {
    let mut b = B::new();
    let (rm, w, half) = (30.0, 4.0, 32.0);
    let c = v(-rm, 0.0);
    b.arc(c, rm + w, -half, half);
    b.arc(polar(c, rm, half), w, half, half + 180.0);
    b.arc(c, rm - w, -half, half);
    b.arc(polar(c, rm, -half), w, 180.0 - half, 360.0 - half);
    b.centerline(v(-rm - 4.0, 0.0), v(10.0, 0.0));
    b.centerline(c, polar(c, rm + 10.0, half));
    b.centerline(c, polar(c, rm + 10.0, -half));
    b.done(name, false)
}

fn cross(name: &str) -> PaletteItem {
    let mut b = B::new();
    let (a, w) = (15.0, 5.0);
    b.chain(&[v(-w, -a), v(w, -a), v(w, -w), v(a, -w), v(a, w), v(w, w), v(w, a), v(-w, a), v(-w, w), v(-a, w), v(-a, -w), v(-w, -w)], true);
    b.axes(20.0, 20.0);
    b.done(name, false)
}

fn oval(name: &str) -> PaletteItem {
    let mut b = B::new();
    let (c, a) = (b.sk.add_point(V2::ZERO), b.sk.add_point(v(20.0, 0.0)));
    b.sk.add_entity(SkKind::Ellipse, vec![c, a], 12.0, false, false);
    b.axes(26.0, 17.0);
    b.done(name, false)
}

fn rounded_rect(name: &str) -> PaletteItem {
    let mut b = B::new();
    let (hw, hh, r) = (20.0, 14.0, 6.0);
    b.line(v(-hw + r, -hh), v(hw - r, -hh));
    b.arc(v(hw - r, -hh + r), r, 270.0, 360.0);
    b.line(v(hw, -hh + r), v(hw, hh - r));
    b.arc(v(hw - r, hh - r), r, 0.0, 90.0);
    b.line(v(hw - r, hh), v(-hw + r, hh));
    b.arc(v(-hw + r, hh - r), r, 90.0, 180.0);
    b.line(v(-hw, hh - r), v(-hw, -hh + r));
    b.arc(v(-hw + r, -hh + r), r, 180.0, 270.0);
    b.done(name, false)
}

/// Wave of two tangent arcs: crest, then trough.
fn wave_arcs(name: &str) -> PaletteItem {
    let mut b = B::new();
    let r = 10.0 * std::f64::consts::SQRT_2;
    b.arc(v(-10.0, -10.0), r, 45.0, 135.0);
    b.arc(v(10.0, 10.0), r, 225.0, 315.0);
    b.axes(26.0, 10.0);
    b.centerline(v(-20.0, -10.0), v(-20.0, 10.0));
    b.done(name, false)
}

/// Wave as a sine spline over one period.
fn wave_spline(name: &str) -> PaletteItem {
    let mut b = B::new();
    let ids: Vec<i64> = (0..=8).map(|k| { let x = -20.0 + 5.0 * k as f64; b.p(v(x, -5.0 * (x * std::f64::consts::PI / 20.0).sin())) }).collect();
    b.sk.add_entity(SkKind::Spline, ids, 0.0, false, false);
    b.axes(26.0, 10.0);
    b.centerline(v(-20.0, -10.0), v(-20.0, 10.0));
    b.done(name, false)
}

/// Half section of a drilled hole (118° tip) for a revolved cut, optionally countersunk.
fn bore(name: &str, sink: Option<f64>) -> PaletteItem {
    let mut b = B::new();
    let (r, depth) = (5.0, 22.0);
    let tip = r / 59f64.to_radians().tan();
    let mut pts = vec![v(0.0, 0.0)];
    match sink {
        Some(angle) => { let rs = 2.0 * r; pts.push(v(rs, 0.0)); pts.push(v(r, -(rs - r) / (angle / 2.0).to_radians().tan())); }
        None => pts.push(v(r, 0.0)),
    }
    pts.push(v(r, -depth));
    pts.push(v(0.0, -depth - tip));
    b.chain(&pts, true);
    b.centerline(v(0.0, 6.0), v(0.0, -depth - tip - 6.0));
    b.done(name, false)
}

fn dovetail(name: &str) -> PaletteItem {
    let mut b = B::new();
    let (hw, h) = (20.0, 13.0);
    let d = h / 60f64.to_radians().tan();
    b.chain(&[v(-hw, 0.0), v(hw, 0.0), v(hw - d, h), v(-hw + d, h)], true);
    b.centerline(v(-hw - 6.0, 0.0), v(hw + 6.0, 0.0));
    b.centerline(v(0.0, -8.0), v(0.0, h + 8.0));
    b.done(name, false)
}

fn hexagon_axes(name: &str) -> PaletteItem {
    let mut b = B::new();
    polygon_into(&mut b, 6);
    b.axes(14.0, 14.0);
    b.done(name, false)
}

/// Channel open to the right, web thickness `tw`, flange thickness `tf`.
fn channel(name: &str, h: f64, w: f64, tw: f64, tf: f64, real: bool, horizontal_axis: bool) -> PaletteItem {
    let mut b = B::new();
    let (x0, y) = (if horizontal_axis { -w / 2.0 } else { -w / 3.0 }, h / 2.0);
    b.chain(&[v(x0, -y), v(x0 + w, -y), v(x0 + w, -y + tf), v(x0 + tw, -y + tf), v(x0 + tw, y - tf), v(x0 + w, y - tf), v(x0 + w, y), v(x0, y)], true);
    if horizontal_axis { b.axes(w * 0.75, 0.0); } else { b.axes(0.0, h * 0.6); }
    b.done(name, real)
}

fn i_beam(name: &str, h: f64, w: f64, tw: f64, tf: f64, real: bool) -> PaletteItem {
    let mut b = B::new();
    let (x, y, s) = (w / 2.0, h / 2.0, tw / 2.0);
    b.chain(&[v(-x, -y), v(x, -y), v(x, -y + tf), v(s, -y + tf), v(s, y - tf), v(x, y - tf), v(x, y), v(-x, y), v(-x, y - tf), v(-s, y - tf), v(-s, -y + tf), v(-x, -y + tf)], true);
    b.axes(w * 0.7, h * 0.6);
    b.done(name, real)
}

fn t_beam(name: &str) -> PaletteItem {
    let mut b = B::new();
    let (x, h, t) = (20.0, 40.0, 8.0);
    b.chain(&[v(-t / 2.0, -h / 2.0), v(t / 2.0, -h / 2.0), v(t / 2.0, h / 2.0 - t), v(x, h / 2.0 - t), v(x, h / 2.0), v(-x, h / 2.0), v(-x, h / 2.0 - t), v(-t / 2.0, h / 2.0 - t)], true);
    b.axes(0.0, h * 0.6);
    b.done(name, false)
}

/// Angle: vertical leg `a`, horizontal leg `bw`, thickness `t`, root radius `r` (0 = sharp).
fn angle(name: &str, a: f64, bw: f64, t: f64, r: f64, real: bool) -> PaletteItem {
    let mut b = B::new();
    if r > 0.0 {
        b.chain(&[v(t, a), v(0.0, a), v(0.0, 0.0), v(bw, 0.0), v(bw, t), v(t + r, t)], false);
        b.arc(v(t + r, t + r), r, 180.0, 270.0);
        b.line(v(t, t + r), v(t, a));
    } else {
        b.chain(&[v(0.0, 0.0), v(bw, 0.0), v(bw, t), v(t, t), v(t, a), v(0.0, a)], true);
    }
    b.done(name, real)
}

/// Rectangular hollow section, height × width × wall.
fn hollow(name: &str, h: f64, w: f64, t: f64) -> PaletteItem {
    let mut b = B::new();
    let (x, y) = (w / 2.0, h / 2.0);
    b.chain(&[v(-x, -y), v(x, -y), v(x, y), v(-x, y)], true);
    b.chain(&[v(-x + t, -y + t), v(x - t, -y + t), v(x - t, y - t), v(-x + t, y - t)], true);
    b.axes(w * 0.8, h * 0.6);
    b.done(name, true)
}

fn build() -> Vec<PaletteTab> {
    const NS: [usize; 11] = [3, 4, 5, 6, 7, 8, 9, 10, 12, 16, 20];
    let german = ["Dreieck", "Viereck", "Fünfeck", "Sechseck", "Siebeneck", "Achteck", "Neuneck", "Zehneck", "Zwölfeck", "Sechzehneck", "Zwanzigeck"];
    let english = [("decagon", 10), ("dodecagon", 12), ("dreieck", 3), ("heptagon", 7), ("hexagon", 6), ("hexdecagon", 16), ("icosagon", 20), ("nonagon", 9), ("octagon", 8), ("pentagon", 5), ("quadrat", 4)];
    let mut sections: Vec<PaletteItem> = [("50x30", 50.0, 30.0, 3.0), ("60x40", 60.0, 40.0, 4.0), ("70x40", 70.0, 40.0, 4.0), ("80x40", 80.0, 40.0, 4.0), ("100x50", 100.0, 50.0, 5.0)]
        .iter().map(|s| hollow(s.0, s.1, s.2, s.3)).collect();
    sections.push(i_beam("I-Profil", 100.0, 55.0, 4.1, 5.7, true));
    // angles as Creo sorts them (by name), leg × leg × thickness
    for (a, bw, t) in [(100.0, 100.0, 10.0), (100.0, 50.0, 8.0), (20.0, 20.0, 3.0), (30.0, 30.0, 3.0), (40.0, 40.0, 4.0), (50.0, 40.0, 5.0), (50.0, 50.0, 5.0), (60.0, 40.0, 5.0), (60.0, 60.0, 6.0),
        (70.0, 70.0, 7.0), (80.0, 60.0, 7.0), (80.0, 80.0, 8.0), (90.0, 90.0, 9.0)] {
        let name = if a == bw { format!("L{}x{}", a, t) } else { format!("L{}x{}x{}", a, bw, t) };
        sections.push(angle(&name, a, bw, t, t, true));
    }
    // U channels (DIN 1026): height, flange width, web, flange
    for (n, h, w, tw, tf) in [("u100", 100.0, 50.0, 6.0, 8.5), ("u160", 160.0, 65.0, 7.5, 10.5), ("u180", 180.0, 70.0, 8.0, 11.0), ("u200", 200.0, 75.0, 8.5, 11.5), ("u300", 300.0, 100.0, 10.0, 16.0),
        ("u50", 50.0, 38.0, 5.0, 7.0), ("u60", 60.0, 30.0, 6.0, 6.0), ("u80", 80.0, 45.0, 6.0, 8.0)] {
        sections.push(channel(n, h, w, tw, tf, true, false));
    }
    vec![
        PaletteTab { name: "Profile", items: vec![channel("C-Profil", 40.0, 26.0, 8.0, 8.0, false, true), i_beam("I-Profil", 40.0, 30.0, 8.0, 8.0, false), angle("L-Profil", 40.0, 30.0, 8.0, 0.0, false), t_beam("T-Profil")] },
        PaletteTab { name: "Formen", items: vec![curved_racetrack("Gebogene Rennbahn"), cross("Quer"), oval("Oval"), racetrack("Rennbahn"), rounded_rect("Abgerundetes Rechteck"), wave_arcs("Welle 1"), wave_spline("Welle 2")] },
        PaletteTab { name: "Sterne", items: NS.iter().map(|n| star(&format!("{}-zackiger Stern", n), *n)).collect() },
        PaletteTab { name: "Profile", items: sections },
        PaletteTab { name: "Sterne", items: NS.iter().map(|n| star(&format!("{}_tip_star", n), *n)).collect() },
        PaletteTab { name: "Polygone", items: english.iter().map(|(n, k)| polygon(n, *k)).collect() },
        PaletteTab { name: "Allgemeines", items: vec![bore("bo_senk60", Some(60.0)), bore("bo_senk90", Some(90.0)), curved_racetrack("bogen"), bore("bohrung", None), cross("kreuz"), racetrack("racetrack"),
            rounded_rect("rechteck-gerundet"), dovetail("schwalbenschwanz"), wave_arcs("welle_1"), wave_spline("welle_2")] },
        PaletteTab { name: "HTL", items: vec![hexagon_axes("6_eck"), racetrack("langloch")] },
        PaletteTab { name: "Polygone", items: NS.iter().zip(german).map(|(n, name)| polygon(name, *n)).collect() },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_is_a_valid_sketch() {
        let tabs = tabs();
        assert_eq!(tabs.len(), 9);
        assert_eq!(tabs.iter().map(|t| t.items.len()).collect::<Vec<_>>(), vec![4, 7, 11, 27, 11, 11, 10, 2, 11]);
        for t in tabs {
            for it in &t.items {
                let open = it.name.to_lowercase().starts_with("welle");
                assert_eq!(it.sketch.open_ends().len(), if open { 2 } else { 0 }, "{} / {}", t.name, it.name);
                if !open {
                    let regions = it.sketch.profile_regions().unwrap_or_else(|e| panic!("{} / {}: {}", t.name, it.name, e));
                    assert_eq!(regions.len(), 1, "{} / {}", t.name, it.name);
                }
                assert!(it.extent() > 1.0);
            }
        }
    }

    #[test]
    fn steel_sections_keep_their_real_size() {
        let it = tabs()[3].items.iter().find(|i| i.name == "L100x50x8").unwrap();
        let (mn, mx) = it.bounds(false);
        assert!(it.real && (mx.x - mn.x - 50.0).abs() < 1e-9 && (mx.y - mn.y - 100.0).abs() < 1e-9);
    }
}
