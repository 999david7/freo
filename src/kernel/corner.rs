//! Corner blend curves between two straight edges: the circular round and the curvature-
//! continuous shapes known from Apple hardware ("Apple corners"). Every shape is defined by how
//! its curvature runs along the curve, then integrated, so the continuity class is exact:
//! G2 = curvature continuous, G3 = the rate of change of curvature continuous as well.

use super::math::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CornerShape {
    /// Circular arc, tangent to both edges (G1)
    #[default]
    Circular,
    /// Lead-in, circular arc, lead-out; curvature rises linearly (G2)
    G2Three,
    /// Lead-in, circular arc, lead-out; curvature rises without a kink (G3)
    G3Three,
    /// As `G3Three` with longer lead-ins and a shorter arc
    G3ThreeRelief,
    /// One curve, curvature is a single smooth bump (G3)
    G3One,
    /// As `G3One` with a longer, gentler start
    G3OneRelief,
}

impl CornerShape {
    pub const ALL: [CornerShape; 6] = [CornerShape::Circular, CornerShape::G2Three, CornerShape::G3Three, CornerShape::G3ThreeRelief, CornerShape::G3One, CornerShape::G3OneRelief];
    /// The curvature-continuous shapes, in the order of the sketcher menu.
    pub const SMOOTH: [CornerShape; 5] = [CornerShape::G3One, CornerShape::G3OneRelief, CornerShape::G3Three, CornerShape::G3ThreeRelief, CornerShape::G2Three];

    pub fn label(self) -> &'static str {
        match self {
            CornerShape::Circular => "Kreisförmig",
            CornerShape::G2Three => "G2, dreiteilig",
            CornerShape::G3Three => "G3, dreiteilig",
            CornerShape::G3ThreeRelief => "G3, dreiteilig, Relief",
            CornerShape::G3One => "G3, einteilig",
            CornerShape::G3OneRelief => "G3, einteilig, Relief",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            CornerShape::Circular => "Kreisbogen, tangential an beide Seiten (G1): die Krümmung springt am Übergang.",
            CornerShape::G2Three => "Einlauf, Kreisbogen, Auslauf: die Krümmung steigt linear an (G2).",
            CornerShape::G3Three => "Einlauf, Kreisbogen, Auslauf: die Krümmung steigt ohne Knick an (G3).",
            CornerShape::G3ThreeRelief => "Wie G3 dreiteilig, mit längeren Einläufen und kürzerem Kreisbogen.",
            CornerShape::G3One => "Eine einzige Kurve, deren Krümmung weich an- und abschwillt (G3) – die typische Apple-Ecke.",
            CornerShape::G3OneRelief => "Wie G3 einteilig, mit längerem, sanfterem Anlauf.",
        }
    }

    /// Name used in files of other tools and by the MCP server.
    pub fn key(self) -> &'static str {
        match self {
            CornerShape::Circular => "circular", CornerShape::G2Three => "g2_three_piece", CornerShape::G3Three => "g3_three_piece",
            CornerShape::G3ThreeRelief => "g3_three_piece_relief", CornerShape::G3One => "g3_one_piece", CornerShape::G3OneRelief => "g3_one_piece_relief",
        }
    }

    pub fn from_key(k: &str) -> Option<CornerShape> { CornerShape::ALL.into_iter().find(|s| s.key() == k) }

    /// Stored in a sketch entity's `r` (see `SkKind::Corner`).
    pub fn code(self) -> f64 { CornerShape::ALL.iter().position(|s| *s == self).unwrap_or(0) as f64 }
    pub fn from_code(c: f64) -> CornerShape { CornerShape::ALL.get(c.round().max(0.0) as usize).copied().unwrap_or(CornerShape::G3One) }

    /// Curvature along the curve, `u` from 0 (first edge) to 1 (second edge); any positive scale.
    pub fn curvature(self, u: f64) -> f64 {
        let u = u.clamp(0.0, 1.0);
        let v = u.min(1.0 - u);
        // ramp over the first `w` of the length, then the plateau of the circular middle piece
        let ramp = |w: f64, smooth: bool| { let t = (v / w).min(1.0); if smooth { t * t * (3.0 - 2.0 * t) } else { t } };
        match self {
            CornerShape::Circular => 1.0,
            CornerShape::G2Three => ramp(0.3, false),
            CornerShape::G3Three => ramp(0.3, true),
            CornerShape::G3ThreeRelief => ramp(0.42, true),
            CornerShape::G3One => (4.0 * u * (1.0 - u)).powi(2),
            CornerShape::G3OneRelief => (4.0 * u * (1.0 - u)).powi(3),
        }
    }

    /// The curve as `n + 1` points (a, b): a point is corner + v1·a + v2·b, where v1 and v2 reach
    /// from the corner along the two edges to the tangent points of the circular round, and
    /// `half` is half the angle between the edges. Every shape passes through the middle of that
    /// round, so one radius means the same corner size for all of them; the smooth shapes start
    /// further out on the edges (a > 1 at the start).
    pub fn points(self, half: f64, n: usize) -> Vec<(f64, f64)> {
        let half = half.clamp(1e-3, std::f64::consts::FRAC_PI_2 - 1e-3);
        let n = n.max(2);
        const FINE: usize = 480;
        // integrate the heading from the curvature (midpoint rule), unit length, total turn π − 2·half
        let k: Vec<f64> = (0..FINE).map(|i| self.curvature((i as f64 + 0.5) / FINE as f64)).collect();
        let turn = std::f64::consts::PI - 2.0 * half;
        let scale = turn / k.iter().sum::<f64>();
        // walk from the first edge towards the corner (heading π) and turn clockwise to heading 2·half
        let mut pts = vec![V2::ZERO];
        let mut heading = std::f64::consts::PI;
        for ki in &k {
            let mid = heading - ki * scale * 0.5;
            heading -= ki * scale;
            let last = *pts.last().unwrap();
            pts.push(last + V2::new(mid.cos(), mid.sin()) / FINE as f64);
        }
        // the corner is where the two edge lines meet: the first runs along X through the start
        let u2 = V2::new((2.0 * half).cos(), (2.0 * half).sin());
        let end = pts[FINE];
        let t = -end.y / u2.y;
        let apex = end + u2 * t;
        // size it so the middle sits where the middle of the unit-radius round sits
        let want = 1.0 / half.sin() - 1.0;
        let f = want / (pts[FINE / 2] - apex).length().max(1e-12);
        let d = 1.0 / half.tan();
        (0..=n).map(|i| {
            let q = (pts[i * FINE / n] - apex) * f;
            let b = q.y / (d * u2.y);
            let a = (q.x - b * d * u2.x) / d;
            if i == 0 { (a, 0.0) } else if i == n { (0.0, b) } else { (a, b) }
        }).collect()
    }
}
