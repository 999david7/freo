//! Interactive routing helpers: shoving tracks of other nets aside, length tuning with
//! meanders, and differential pairs (two tracks at a fixed gap along one centre line).

use super::board::{Board, Track};
use super::geom::{p, pt_seg, Hull, P};

/// How the interactive router treats copper in the way (KiCad's router modes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// show the collision and refuse
    Highlight,
    /// go around obstacles
    Walkaround,
    /// push tracks of other nets aside
    Shove,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Highlight => "Kollisionen anzeigen",
            Mode::Walkaround => "Ausweichen",
            Mode::Shove => "Schieben",
        }
    }
}

fn perp(d: P) -> P {
    p(-d.y, d.x)
}

/// Clearance problems of a set of tracks against the rest of the board (ignoring `skip`).
fn collides(b: &Board, segs: &[Track], skip: &[usize]) -> bool {
    let cu = b.copper();
    for t in segs {
        let h = Hull::capsule(t.a, t.b, t.width / 2.0);
        for c in &cu {
            if c.net == t.net && !t.net.is_empty() || c.layers & (1 << t.layer) == 0 {
                continue;
            }
            if let super::board::Item::Track(k) = c.what {
                if skip.contains(&k) {
                    continue;
                }
            }
            if h.gap(&c.hull) < b.rules.clearance_between(&t.net, &c.net) - 1e-6 {
                return true;
            }
        }
    }
    false
}

/// Pushes tracks of other nets out of the way of the new segment(s) `new`. Each blocking
/// track is moved sideways (its neighbours stretch along so it stays connected); pads and
/// vias never move. Returns how many tracks moved, or an error when the shove would create
/// new clearance problems (then nothing changes).
pub fn shove(b: &mut Board, new: &[Track]) -> Result<usize, String> {
    let orig = b.tracks.clone();
    let mut moved: Vec<usize> = vec![];
    for t in new {
        let hn = Hull::capsule(t.a, t.b, t.width / 2.0);
        let dir = (t.b - t.a).normalize_or_zero();
        for k in 0..b.tracks.len() {
            let o = b.tracks[k].clone();
            if o.layer != t.layer || o.net == t.net {
                continue;
            }
            let need = b.rules.clearance_between(&t.net, &o.net);
            let ho = Hull::capsule(o.a, o.b, o.width / 2.0);
            let gap = hn.gap(&ho);
            if gap >= need - 1e-6 {
                continue;
            }
            // push perpendicular to the new track, towards the side the other track is on
            let mid = (o.a + o.b) / 2.0;
            let side = if dir.length() < 1e-9 { (mid - t.a).normalize_or_zero() } else { perp(dir) * perp(dir).dot(mid - t.a).signum() };
            if side.length() < 1e-9 {
                b.tracks = orig;
                return Err("Leiterbahn liegt genau auf der neuen – kann nicht geschoben werden".into());
            }
            let push = side * (need - gap + 0.01);
            // ends on pads or vias stay put: the track leaves them with a 45° jog
            let fixed = |q: P, b: &Board| b.vias.iter().any(|v| (v.pos - q).length() < 1e-6) || b.footprints.iter().any(|f| (0..f.fp.pads.len()).any(|i| f.pad_hull(i).contains(q)));
            let (fa, fb) = (fixed(o.a, b), fixed(o.b, b));
            if fa || fb {
                let s = push.length();
                let d = (o.b - o.a).normalize_or_zero();
                let len = (o.b - o.a).length();
                if len < s * (if fa && fb { 2.0 } else { 1.0 }) + 0.05 {
                    b.tracks = orig;
                    return Err(format!("Leiterbahn {} ist zu kurz zum Ausweichen", o.net));
                }
                let mut pts = vec![o.a];
                if fa {
                    pts.push(o.a + d * s + push);
                } else {
                    pts[0] = o.a + push;
                }
                if fb {
                    pts.push(o.b - d * s + push);
                    pts.push(o.b);
                } else {
                    pts.push(o.b + push);
                }
                let (oa, ob) = (o.a, o.b);
                // neighbours at the free end follow
                for (j, x) in b.tracks.iter_mut().enumerate() {
                    if j == k || x.net != o.net || x.layer != o.layer {
                        continue;
                    }
                    if !fa && (x.a - oa).length() < 1e-6 {
                        x.a += push;
                    }
                    if !fa && (x.b - oa).length() < 1e-6 {
                        x.b += push;
                    }
                    if !fb && (x.a - ob).length() < 1e-6 {
                        x.a += push;
                    }
                    if !fb && (x.b - ob).length() < 1e-6 {
                        x.b += push;
                    }
                }
                let segs: Vec<Track> = pts.windows(2).map(|w| Track { a: w[0], b: w[1], width: o.width, layer: o.layer, net: o.net.clone() }).collect();
                b.tracks[k] = segs[0].clone();
                for sgm in segs.into_iter().skip(1) {
                    moved.push(b.tracks.len());
                    b.tracks.push(sgm);
                }
                moved.push(k);
                continue;
            }
            let (oa, ob) = (o.a, o.b);
            for (j, x) in b.tracks.iter_mut().enumerate() {
                if x.net != o.net || x.layer != o.layer {
                    continue;
                }
                if j == k {
                    x.a += push;
                    x.b += push;
                } else {
                    if (x.a - oa).length() < 1e-6 || (x.a - ob).length() < 1e-6 {
                        x.a += push;
                    }
                    if (x.b - oa).length() < 1e-6 || (x.b - ob).length() < 1e-6 {
                        x.b += push;
                    }
                }
            }
            moved.push(k);
        }
    }
    let affected: Vec<Track> = b.tracks.iter().enumerate().filter(|(i, x)| moved.contains(i) || orig.get(*i).is_some_and(|o| o != *x)).map(|(_, x)| x.clone()).collect();
    if collides(b, &affected, &[]) || collides(b, new, &[]) {
        b.tracks = orig;
        return Err("Geschobene Leiterbahnen würden neue Abstandsfehler erzeugen".into());
    }
    Ok(moved.len())
}

/// Replaces the straight piece a–b by a centred meander of `waves` full waves: each half
/// wave rises to `amp` with 45° corners; `c` is a quarter of a half wave (the distance
/// between neighbouring legs is 2·c).
pub fn meander(a: P, b: P, waves: usize, amp: f64, c: f64) -> Vec<P> {
    let len = (b - a).length();
    let d = (b - a) / len.max(1e-9);
    let n = perp(d);
    let used = waves as f64 * 8.0 * c;
    if used > len || amp < 2.0 * c {
        return vec![a, b];
    }
    let start = (len - used) / 2.0;
    let at = |x: f64, y: f64| a + d * x + n * y;
    let mut pts = vec![a, at(start, 0.0)];
    for h in 0..2 * waves {
        let x = start + h as f64 * 4.0 * c;
        let s = if h % 2 == 0 { 1.0 } else { -1.0 };
        for (dx, dy) in [(c, c), (c, amp - c), (2.0 * c, amp), (3.0 * c, amp - c), (3.0 * c, c), (4.0 * c, 0.0)] {
            pts.push(at(x + dx, s * dy));
        }
    }
    pts.push(b);
    super::autoroute::simplify(&pts)
}

/// Length a meander adds: per half wave 4·c·√2 + 2·amp − 8·c.
pub fn meander_extra(waves: usize, amp: f64, c: f64) -> f64 {
    2.0 * waves as f64 * (4.0 * c * std::f64::consts::SQRT_2 + 2.0 * amp - 8.0 * c)
}

/// Tunes the length of `net` to `target` by turning one of its straight tracks into a
/// meander (longest first, as few waves as fit under 3 mm amplitude). Returns the new length.
pub fn tune_length(b: &mut Board, net: &str, target: f64) -> Result<f64, String> {
    let now = b.track_length(net);
    if now >= target - 0.05 {
        return Err(format!("Netz {} ist schon {} mm lang", net, super::geom::mm((now * 100.0).round() / 100.0)));
    }
    let extra = target - now;
    let mut idx: Vec<usize> = (0..b.tracks.len()).filter(|&i| b.tracks[i].net == net).collect();
    idx.sort_by(|&x, &y| (b.tracks[y].a - b.tracks[y].b).length().partial_cmp(&(b.tracks[x].a - b.tracks[x].b).length()).unwrap());
    let cr = b.rules.class_of(net);
    for k in idx {
        let t = b.tracks[k].clone();
        let c = (t.width + cr.clearance) / 2.0 + 0.01;
        let len = (t.b - t.a).length();
        for waves in 1..=((len / (8.0 * c)) as usize) {
            // amplitude for exactly the missing length
            let amp = (extra / (2.0 * waves as f64) - 4.0 * c * std::f64::consts::SQRT_2 + 8.0 * c) / 2.0;
            if amp > 3.0 {
                continue;
            }
            if amp < 2.0 * c {
                break;
            }
            for flip in [false, true] {
                let (a, e) = if flip { (t.b, t.a) } else { (t.a, t.b) };
                let pts = meander(a, e, waves, amp, c);
                if pts.len() <= 2 {
                    continue;
                }
                let segs: Vec<Track> = pts.windows(2).map(|w| Track { a: w[0], b: w[1], width: t.width, layer: t.layer, net: t.net.clone() }).collect();
                let mut trial = b.clone();
                trial.tracks.remove(k);
                trial.tracks.extend(segs.iter().cloned());
                let margin = t.width / 2.0 + trial.rules.edge_clearance;
                if !collides(&trial, &segs, &[]) && segs.iter().all(|s| trial.region_contains(s.a, margin) && trial.region_contains(s.b, margin)) {
                    *b = trial;
                    return Ok(b.track_length(net));
                }
            }
        }
    }
    Err(format!("Kein Platz für einen Mäander im Netz {}", net))
}

/// Offsets a polyline sideways by `d` (left of the direction of travel), joining the
/// pieces with mitres.
pub fn offset_polyline(pts: &[P], d: f64) -> Vec<P> {
    let n = pts.len();
    if n < 2 {
        return pts.to_vec();
    }
    let normal = |i: usize| perp((pts[i + 1] - pts[i]).normalize_or_zero());
    let mut out = vec![pts[0] + normal(0) * d];
    for i in 1..n - 1 {
        let (n0, n1) = (normal(i - 1), normal(i));
        let m = (n0 + n1).normalize_or_zero();
        let k = m.dot(n0).max(0.3);
        out.push(pts[i] + m * (d / k));
    }
    out.push(pts[n - 1] + normal(n - 2) * d);
    out
}

/// The two tracks of a differential pair along the centre line `center`.
pub fn diff_pair(center: &[P], width: f64, gap: f64) -> (Vec<P>, Vec<P>) {
    let off = (width + gap) / 2.0;
    (offset_polyline(center, off), offset_polyline(center, -off))
}

impl Board {
    /// True when `q` lies inside the board with at least `margin` to its edges.
    pub fn region_contains(&self, q: P, margin: f64) -> bool {
        let Some(outer) = self.outer() else { return false };
        if !super::geom::in_poly(q, outer) {
            return false;
        }
        self.outline.iter().all(|l| {
            let n = l.len();
            (0..n).all(|i| pt_seg(q, l[i], l[(i + 1) % n]) >= margin)
        }) && self.outline.iter().filter(|l| *l != outer).all(|l| !super::geom::in_poly(q, l))
    }
}
