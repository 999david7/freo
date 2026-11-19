//! Triangle meshes in printer coordinates (Z up, origin at the front left of the bed) and the
//! placement of a part on the build plate: orientation, lay flat on a face, scale, copies.

use glam::{DMat3, DVec3};

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub v: Vec<DVec3>,
    pub t: Vec<[u32; 3]>,
}

/// Freo's world (Y up, Z towards the viewer) to the printer frame (Z up): x → x,
/// −z → y, y → z. A proper rotation, so the part is not mirrored.
pub fn world_to_printer(p: DVec3) -> DVec3 {
    DVec3::new(p.x, -p.z, p.y)
}

pub fn printer_to_world(p: DVec3) -> DVec3 {
    DVec3::new(p.x, p.z, -p.y)
}

impl Mesh {
    /// Welds coincident corners of loose triangles (world coordinates) and turns them into
    /// the printer frame.
    pub fn from_world_triangles(tris: &[[DVec3; 3]]) -> Mesh {
        let mut m = Mesh::default();
        let mut map: std::collections::HashMap<[i64; 3], u32> = std::collections::HashMap::new();
        let q = |p: DVec3| [(p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64];
        for tri in tris {
            let mut idx = [0u32; 3];
            for k in 0..3 {
                let p = world_to_printer(tri[k]);
                if !p.is_finite() {
                    continue;
                }
                let key = q(p);
                idx[k] = *map.entry(key).or_insert_with(|| {
                    m.v.push(p);
                    (m.v.len() - 1) as u32
                });
            }
            if idx[0] != idx[1] && idx[1] != idx[2] && idx[0] != idx[2] {
                m.t.push(idx);
            }
        }
        m
    }

    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }

    pub fn bbox(&self) -> (DVec3, DVec3) {
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for p in &self.v {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        (lo, hi)
    }

    pub fn tri(&self, i: usize) -> [DVec3; 3] {
        let t = self.t[i];
        [self.v[t[0] as usize], self.v[t[1] as usize], self.v[t[2] as usize]]
    }

    /// Enclosed volume (mm³), from the divergence theorem.
    pub fn volume(&self) -> f64 {
        (0..self.t.len())
            .map(|i| {
                let [a, b, c] = self.tri(i);
                a.dot(b.cross(c)) / 6.0
            })
            .sum::<f64>()
            .abs()
    }

    pub fn transformed(&self, m: DMat3) -> Mesh {
        let flip = m.determinant() < 0.0;
        Mesh {
            v: self.v.iter().map(|p| m * *p).collect(),
            t: if flip { self.t.iter().map(|t| [t[0], t[2], t[1]]).collect() } else { self.t.clone() },
        }
    }

    pub fn translated(&self, d: DVec3) -> Mesh {
        Mesh { v: self.v.iter().map(|p| *p + d).collect(), t: self.t.clone() }
    }

    pub fn append(&mut self, other: &Mesh) {
        let base = self.v.len() as u32;
        self.v.extend_from_slice(&other.v);
        self.t.extend(other.t.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }

    /// Ray–triangle intersection (Möller–Trumbore): nearest triangle hit and its distance.
    pub fn raycast(&self, o: DVec3, d: DVec3) -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64)> = None;
        for i in 0..self.t.len() {
            let [a, b, c] = self.tri(i);
            let (e1, e2) = (b - a, c - a);
            let p = d.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1e-12 {
                continue;
            }
            let inv = 1.0 / det;
            let s = o - a;
            let u = s.dot(p) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let v = d.dot(q) * inv;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let t = e2.dot(q) * inv;
            if t > 1e-9 && best.is_none_or(|b| t < b.1) {
                best = Some((i, t));
            }
        }
        best
    }

    pub fn normal(&self, i: usize) -> DVec3 {
        let [a, b, c] = self.tri(i);
        (b - a).cross(c - a).normalize_or_zero()
    }

    /// Planar regions of the convex hull's directions, largest area first: candidates for
    /// "Auf Fläche legen". Returns (outward normal, total area of coplanar outer triangles).
    pub fn flat_faces(&self) -> Vec<(DVec3, f64)> {
        let mut groups: Vec<(DVec3, f64, f64)> = vec![]; // normal, area, support distance
        for i in 0..self.t.len() {
            let [a, b, c] = self.tri(i);
            let cr = (b - a).cross(c - a);
            let area = cr.length() * 0.5;
            if area < 1e-9 {
                continue;
            }
            let n = cr / (2.0 * area);
            let d = n.dot(a);
            if let Some(g) = groups.iter_mut().find(|g| g.0.dot(n) > 0.9999 && (g.2 - d).abs() < 1e-4) {
                g.1 += area;
            } else {
                groups.push((n, area, d));
            }
        }
        // only faces that touch the hull in their direction can rest on the bed
        let mut out: Vec<(DVec3, f64)> = groups
            .into_iter()
            .filter(|(n, _, d)| self.v.iter().all(|p| n.dot(*p) <= d + 1e-4))
            .map(|(n, a, _)| (n, a))
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        out
    }
}

/// Rotation turning `n` to point straight down (−Z): the face with normal `n` lies on the bed.
pub fn lay_flat_rotation(n: DVec3) -> DMat3 {
    let n = n.normalize();
    let down = DVec3::NEG_Z;
    let c = n.dot(down);
    if c > 1.0 - 1e-12 {
        return DMat3::IDENTITY;
    }
    if c < -1.0 + 1e-12 {
        return DMat3::from_rotation_x(std::f64::consts::PI);
    }
    let axis = n.cross(down).normalize();
    DMat3::from_axis_angle(axis, c.clamp(-1.0, 1.0).acos())
}

/// How the part sits on the plate.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Placement {
    /// rotation (column major 3×3) applied in the printer frame before everything else
    pub rot: [f64; 9],
    /// uniform scale, 1 = 100 %
    pub scale: f64,
    /// extra rotation about Z (degrees)
    pub turn: f64,
    /// position of the part centre on the bed; None = bed centre
    pub center: Option<[f64; 2]>,
    pub copies: u32,
    /// gap between copies (mm)
    pub spacing: f64,
}

impl Default for Placement {
    fn default() -> Self {
        Placement { rot: DMat3::IDENTITY.to_cols_array(), scale: 1.0, turn: 0.0, center: None, copies: 1, spacing: 6.0 }
    }
}

impl Placement {
    pub fn rotation(&self) -> DMat3 {
        DMat3::from_rotation_z(self.turn.to_radians()) * DMat3::from_cols_array(&self.rot)
    }

    pub fn rotate_by(&mut self, r: DMat3) {
        self.rot = (r * DMat3::from_cols_array(&self.rot)).to_cols_array();
    }

    /// Where every copy goes: (linear map, offset) taking the part's points (printer frame,
    /// as modelled) to the bed. The copies rest on the bed (z = 0), centred on `center` (or
    /// the bed centre), in a grid around it.
    pub fn transforms(&self, part: &Mesh, bed: (f64, f64)) -> Vec<(DMat3, DVec3)> {
        if part.is_empty() {
            return vec![];
        }
        let m = self.rotation() * DMat3::from_diagonal(DVec3::splat(self.scale.max(1e-6)));
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for p in &part.v {
            let q = m * *p;
            lo = lo.min(q);
            hi = hi.max(q);
        }
        let size = hi - lo;
        let n = self.copies.max(1) as usize;
        let cols = (n as f64).sqrt().ceil() as usize;
        let rows = n.div_ceil(cols);
        let step = (size.x + self.spacing, size.y + self.spacing);
        let total = (cols as f64 * step.0 - self.spacing, rows as f64 * step.1 - self.spacing);
        let c = self.center.unwrap_or([bed.0 / 2.0, bed.1 / 2.0]);
        (0..n)
            .map(|k| {
                let (i, j) = (k % cols, k / cols);
                let x0 = c[0] - total.0 / 2.0 + i as f64 * step.0;
                let y0 = c[1] + total.1 / 2.0 - (j as f64 + 1.0) * step.1 + self.spacing;
                (m, DVec3::new(x0 - lo.x, y0 - lo.y, -lo.z))
            })
            .collect()
    }

    /// The part (and its copies) where it will be printed.
    pub fn apply(&self, part: &Mesh, bed: (f64, f64)) -> Mesh {
        let mut out = Mesh::default();
        for (m, t) in self.transforms(part, bed) {
            out.append(&part.transformed(m).translated(t));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn cube(s: f64) -> Mesh {
        let v = [
            [0.0, 0.0, 0.0],
            [s, 0.0, 0.0],
            [s, s, 0.0],
            [0.0, s, 0.0],
            [0.0, 0.0, s],
            [s, 0.0, s],
            [s, s, s],
            [0.0, s, s],
        ]
        .map(|p| DVec3::from_array(p));
        let t = vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        Mesh { v: v.to_vec(), t }
    }

    #[test]
    fn volume_and_frames() {
        let c = cube(10.0);
        assert!((c.volume() - 1000.0).abs() < 1e-9);
        let w = DVec3::new(1.0, 2.0, 3.0);
        assert!((printer_to_world(world_to_printer(w)) - w).length() < 1e-12);
        // Y-up world point at height 2 is printer z = 2
        assert_eq!(world_to_printer(w).z, 2.0);
    }

    #[test]
    fn placement_rests_on_bed() {
        let c = cube(10.0);
        let mut pl = Placement::default();
        pl.rotate_by(lay_flat_rotation(DVec3::X));
        let m = pl.apply(&c, (220.0, 220.0));
        let (lo, hi) = m.bbox();
        assert!(lo.z.abs() < 1e-9);
        assert!(((lo.x + hi.x) / 2.0 - 110.0).abs() < 1e-9);
        pl.copies = 4;
        let m = pl.apply(&c, (220.0, 220.0));
        assert!((m.volume() - 4000.0).abs() < 1e-6);
        let (lo, hi) = m.bbox();
        assert!(((lo.x + hi.x) / 2.0 - 110.0).abs() < 1e-9 && ((lo.y + hi.y) / 2.0 - 110.0).abs() < 1e-9);
        let faces = c.flat_faces();
        assert_eq!(faces.len(), 6);
    }
}
