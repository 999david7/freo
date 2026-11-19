//! Cutting tools: geometry for toolpaths, simulation and collision checks.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ToolKind {
    Flat,
    Ball,
    Bull,
    VBit,
    Drill,
}

impl ToolKind {
    pub const ALL: [ToolKind; 5] = [ToolKind::Flat, ToolKind::Ball, ToolKind::Bull, ToolKind::VBit, ToolKind::Drill];
    pub fn name(&self) -> &'static str {
        match self {
            ToolKind::Flat => "Schaftfräser",
            ToolKind::Ball => "Kugelfräser",
            ToolKind::Bull => "Torusfräser",
            ToolKind::VBit => "V-Fräser / Fase",
            ToolKind::Drill => "Bohrer",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tool {
    pub number: u32,
    pub kind: ToolKind,
    /// cutting diameter
    pub diameter: f64,
    pub flutes: u32,
    /// length of the cutting edges
    pub flute_length: f64,
    pub shank: f64,
    /// length out of the holder (tip to collet nut)
    pub stickout: f64,
    /// collet nut / holder diameter
    pub holder: f64,
    pub holder_length: f64,
    /// Torus: corner radius
    pub corner: f64,
    /// V-bit and drill: included tip angle, degrees
    pub angle: f64,
}

impl Default for Tool {
    fn default() -> Self {
        Tool::flat(1, 6.0, 2)
    }
}

impl Tool {
    /// The longest the tool can stick out of its collet: the overall length of a long-series
    /// cutter of this diameter minus three shank diameters held in the collet.
    pub fn max_stickout(&self) -> f64 {
        let overall = (self.diameter * 12.5).clamp(38.0, 150.0).max(self.flute_length + 3.0 * self.shank + 5.0);
        (overall - 3.0 * self.shank).max(self.stickout)
    }

    pub fn flat(number: u32, d: f64, flutes: u32) -> Tool {
        let shank = if d < 3.0 { 3.175f64.max(d) } else { d };
        Tool {
            number,
            kind: ToolKind::Flat,
            diameter: d,
            flutes,
            flute_length: (d * 3.0).clamp(3.0, 40.0),
            shank,
            stickout: (d * 3.0).clamp(3.0, 40.0) + 10.0,
            holder: 22.0_f64.max(shank + 12.0),
            holder_length: 30.0,
            corner: 0.0,
            angle: 0.0,
        }
    }
    pub fn ball(number: u32, d: f64, flutes: u32) -> Tool {
        Tool { kind: ToolKind::Ball, corner: d / 2.0, ..Tool::flat(number, d, flutes) }
    }
    pub fn vbit(number: u32, d: f64, angle: f64) -> Tool {
        let h = d / 2.0 / (angle.to_radians() / 2.0).tan();
        Tool { kind: ToolKind::VBit, angle, flute_length: h, stickout: h + 15.0, ..Tool::flat(number, d, 2) }
    }
    pub fn drill(number: u32, d: f64) -> Tool {
        Tool { kind: ToolKind::Drill, angle: 118.0, flute_length: (d * 5.0).max(10.0), stickout: (d * 5.0).max(10.0) + 15.0, ..Tool::flat(number, d, 2) }
    }
    pub fn radius(&self) -> f64 {
        self.diameter / 2.0
    }
    /// Height of the cutting surface above the tip at distance `r` from the axis
    /// (None outside the tool).
    pub fn profile(&self, r: f64) -> Option<f64> {
        let rr = self.radius();
        if r > rr + 1e-9 {
            return None;
        }
        let r = r.min(rr);
        Some(match self.kind {
            ToolKind::Flat => 0.0,
            ToolKind::Ball => rr - (rr * rr - r * r).max(0.0).sqrt(),
            ToolKind::Bull => {
                let c = self.corner.clamp(0.0, rr);
                let flat = rr - c;
                if r <= flat {
                    0.0
                } else {
                    c - (c * c - (r - flat).powi(2)).max(0.0).sqrt()
                }
            }
            ToolKind::VBit | ToolKind::Drill => r / (self.angle.clamp(1.0, 179.0).to_radians() / 2.0).tan(),
        })
    }
    /// Can cut straight down into material (centre cutting).
    pub fn plunges(&self) -> bool {
        true
    }
    pub fn label(&self) -> String {
        match self.kind {
            ToolKind::VBit => format!("T{} {} {}° Ø{}", self.number, self.kind.name(), self.angle, fmt(self.diameter)),
            ToolKind::Bull => format!("T{} {} Ø{} R{}", self.number, self.kind.name(), fmt(self.diameter), fmt(self.corner)),
            _ => format!("T{} {} Ø{} {}Z", self.number, self.kind.name(), fmt(self.diameter), self.flutes),
        }
    }
    pub fn validate(&self) -> Option<String> {
        if !(self.diameter > 0.05 && self.diameter < 100.0) {
            return Some(format!("T{}: Durchmesser muss zwischen 0,05 und 100 mm liegen.", self.number));
        }
        if self.flutes == 0 {
            return Some(format!("T{}: mindestens eine Schneide.", self.number));
        }
        if self.flute_length <= 0.0 || self.stickout < self.flute_length {
            return Some(format!("T{}: die Auskraglänge muss mindestens so lang wie die Schneidenlänge sein.", self.number));
        }
        if self.holder < self.shank {
            return Some(format!("T{}: der Halter muss dicker als der Schaft sein.", self.number));
        }
        None
    }
}

fn fmt(v: f64) -> String {
    let s = format!("{:.3}", v);
    s.trim_end_matches('0').trim_end_matches('.').replace('.', ",")
}

/// A starter library: common carbide cutters and drills.
pub fn library() -> Vec<Tool> {
    let mut v = vec![];
    let mut n = 1;
    for (d, z) in [(1.0, 2), (2.0, 2), (3.0, 2), (3.175, 2), (4.0, 3), (6.0, 3), (6.35, 2), (8.0, 3), (10.0, 3), (12.0, 3)] {
        v.push(Tool::flat(n, d, z));
        n += 1;
    }
    for d in [1.0, 3.0, 6.0] {
        v.push(Tool::ball(n, d, 2));
        n += 1;
    }
    let mut t = Tool::flat(n, 6.0, 4);
    t.kind = ToolKind::Bull;
    t.corner = 0.5;
    v.push(t);
    n += 1;
    for a in [90.0, 60.0] {
        v.push(Tool::vbit(n, 12.0, a));
        n += 1;
    }
    for d in [2.5, 3.3, 4.2, 5.0, 6.8, 8.5] {
        v.push(Tool::drill(n, d));
        n += 1;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles() {
        let f = Tool::flat(1, 6.0, 2);
        assert_eq!(f.profile(2.9), Some(0.0));
        assert_eq!(f.profile(3.1), None);
        let b = Tool::ball(2, 6.0, 2);
        assert!((b.profile(3.0).unwrap() - 3.0).abs() < 1e-9);
        assert!(b.profile(0.0).unwrap().abs() < 1e-12);
        let v = Tool::vbit(3, 10.0, 90.0);
        assert!((v.profile(2.0).unwrap() - 2.0).abs() < 1e-9);
        for t in library() {
            assert!(t.validate().is_none(), "{:?}", t.validate());
        }
    }
}
