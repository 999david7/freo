//! Voxel mesh of a solid: the part is sampled on a regular grid of box cells (as Creo
//! Simulation Live does on the GPU), every solid cell becomes one 8-node element.

use crate::kernel::math::*;
use crate::kernel::topology::Topology;
use std::collections::HashMap;

pub const NONE: u32 = u32::MAX;

pub fn threads() -> usize { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 12) }

/// Runs `f(start, chunk)` on slices of `data` in parallel; chunk lengths are multiples of `unit`.
pub fn par_chunks<T: Send>(data: &mut [T], unit: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let units = data.len() / unit.max(1);
    let t = threads().min(units.max(1));
    if t <= 1 || data.len() < 16_384 { f(0, data); return; }
    let per = units.div_ceil(t) * unit;
    std::thread::scope(|s| {
        for (i, c) in data.chunks_mut(per).enumerate() {
            let f = &f;
            s.spawn(move || f(i * per, c));
        }
    });
}

pub fn tris_of(topo: &Topology) -> Vec<[V3; 3]> {
    let mut out = vec![];
    for p in &topo.polys {
        for k in 1..p.len() - 1 { out.push([topo.positions[p[0]], topo.positions[p[k]], topo.positions[p[k + 1]]]); }
    }
    out
}

/// Splits a triangle (longest edge first) until no edge is longer than `max_edge`.
pub fn subdivide(t: [V3; 3], max_edge: f64, depth: u32, f: &mut impl FnMut([V3; 3])) {
    let l = [(t[1] - t[0]).length_squared(), (t[2] - t[1]).length_squared(), (t[0] - t[2]).length_squared()];
    let k = if l[0] >= l[1] && l[0] >= l[2] { 0 } else if l[1] >= l[2] { 1 } else { 2 };
    if l[k] <= max_edge * max_edge || depth == 0 { f(t); return; }
    let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
    let m = (a + b) * 0.5;
    subdivide([a, m, c], max_edge, depth - 1, f);
    subdivide([m, b, c], max_edge, depth - 1, f);
}

#[derive(Clone)]
pub struct Grid {
    pub origin: V3,
    pub h: V3,
    pub n: [usize; 3],
    pub solid: Vec<bool>,
}

impl Grid {
    pub fn cell(&self, i: usize, j: usize, k: usize) -> usize { i + self.n[0] * (j + self.n[1] * k) }
    pub fn cells(&self) -> usize { self.n[0] * self.n[1] * self.n[2] }
    pub fn center(&self, i: usize, j: usize, k: usize) -> V3 { self.origin + V3::new((i as f64 + 0.5) * self.h.x, (j as f64 + 0.5) * self.h.y, (k as f64 + 0.5) * self.h.z) }
    /// Cell containing `p`, if inside the grid.
    pub fn cell_at(&self, p: V3) -> Option<[usize; 3]> {
        let r = (p - self.origin) / self.h;
        if r.x < 0.0 || r.y < 0.0 || r.z < 0.0 { return None; }
        let c = [r.x as usize, r.y as usize, r.z as usize];
        (c[0] < self.n[0] && c[1] < self.n[1] && c[2] < self.n[2]).then_some(c)
    }

    /// Cell counts for a box of `size` with about `target` cells. Thin directions get up to
    /// four layers as long as the cells stay within an aspect ratio of 4.
    pub fn plan(size: V3, target: usize, cubic: bool) -> [usize; 3] {
        let s = size.max(V3::splat(1e-6));
        let mut h = (s.x * s.y * s.z / target.max(1) as f64).cbrt();
        let mut n = [1usize; 3];
        for _ in 0..16 {
            for a in 0..3 {
                let min_l = if cubic { 1.0 } else { (s[a] / (h / 4.0)).ceil().clamp(1.0, 4.0) };
                n[a] = ((s[a] / h).round().max(min_l) as usize).clamp(1, 600);
            }
            let total = (n[0] * n[1] * n[2]) as f64;
            h *= (total / target.max(1) as f64).powf(1.0 / 3.0).clamp(0.7, 1.4);
        }
        n
    }

    /// Marks the cells whose centre lies inside the closed triangle mesh (ray parity along X).
    pub fn voxelize(tris: &[[V3; 3]], origin: V3, h: V3, n: [usize; 3]) -> Grid {
        let mut hits: Vec<Vec<f32>> = vec![vec![]; n[1] * n[2]];
        // sample rays sit a hair off the cell centres so they never run exactly along a mesh edge
        let (oy, oz) = (origin.y + h.y * 0.5 + h.y * 3.7e-7, origin.z + h.z * 0.5 + h.z * 7.1e-7);
        for t in tris {
            let (y0, y1) = (t[0].y.min(t[1].y).min(t[2].y), t[0].y.max(t[1].y).max(t[2].y));
            let (z0, z1) = (t[0].z.min(t[1].z).min(t[2].z), t[0].z.max(t[1].z).max(t[2].z));
            let j0 = (((y0 - oy) / h.y).ceil().max(0.0)) as usize;
            let k0 = (((z0 - oz) / h.z).ceil().max(0.0)) as usize;
            let j1 = ((y1 - oy) / h.y).floor();
            let k1 = ((z1 - oz) / h.z).floor();
            if j1 < 0.0 || k1 < 0.0 { continue; }
            let (j1, k1) = ((j1 as usize).min(n[1] - 1), (k1 as usize).min(n[2] - 1));
            let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
            let det = e1.y * e2.z - e1.z * e2.y;
            if det.abs() < 1e-14 { continue; }
            for k in k0..=k1 {
                for j in j0..=j1 {
                    let (py, pz) = (oy + j as f64 * h.y - t[0].y, oz + k as f64 * h.z - t[0].z);
                    let u = (py * e2.z - pz * e2.y) / det;
                    let v = (e1.y * pz - e1.z * py) / det;
                    if u < 0.0 || v < 0.0 || u + v > 1.0 { continue; }
                    hits[j + n[1] * k].push((t[0].x + u * e1.x + v * e2.x) as f32);
                }
            }
        }
        let mut solid = vec![false; n[0] * n[1] * n[2]];
        for k in 0..n[2] {
            for j in 0..n[1] {
                let row = &mut hits[j + n[1] * k];
                if row.len() < 2 { continue; }
                row.sort_by(|a, b| a.partial_cmp(b).unwrap());
                for pair in row.chunks(2) {
                    if pair.len() < 2 { break; }
                    let i0 = (((pair[0] as f64 - origin.x) / h.x - 0.5).ceil().max(0.0)) as usize;
                    let i1 = ((pair[1] as f64 - origin.x) / h.x - 0.5).floor();
                    if i1 < 0.0 { continue; }
                    for i in i0..=(i1 as usize).min(n[0] - 1) { solid[i + n[0] * (j + n[1] * k)] = true; }
                }
            }
        }
        Grid { origin, h, n, solid }
    }
}

/// Hexahedral mesh of the solid cells. Local corner `a` of an element sits at the cell corner
/// (a & 1, a >> 1 & 1, a >> 2 & 1).
pub struct Mesh {
    pub grid: Grid,
    pub elems: Vec<[u32; 8]>,
    pub elem_ijk: Vec<[u16; 3]>,
    /// element of each grid cell, NONE where empty
    pub cell_elem: Vec<u32>,
    pub nodes: Vec<V3>,
    /// per node: the element that has it as local corner s (NONE if that cell is empty)
    pub node_elems: Vec<[u32; 8]>,
    /// share of the solid cells left out because they hang loose from the main body
    pub dropped: f64,
}

/// Node weights of a face: how much of the face's area each mesh node carries.
#[derive(Default)]
pub struct FaceMap {
    /// (node, area share mm², area-weighted outward normal)
    pub nodes: Vec<(u32, f64, V3)>,
    pub area: f64,
    pub centroid: V3,
}

impl Mesh {
    /// Meshes `topo` with about `target` elements.
    pub fn build(topo: &Topology, target: usize) -> Result<Mesh, String> {
        if topo.polys.is_empty() || topo.bbox.is_empty() { return Err("Das Modell enthält kein Volumen.".into()); }
        let tris = tris_of(topo);
        let (origin, size) = (topo.bbox.min, topo.bbox.size());
        let mut want = target.max(64);
        let mut grid = None;
        // the target counts solid cells: correct once for the fill ratio of the bounding box
        for _ in 0..2 {
            let n = Grid::plan(size, want, false);
            let h = V3::new(size.x / n[0] as f64, size.y / n[1] as f64, size.z / n[2] as f64);
            let g = Grid::voxelize(&tris, origin, h, n);
            let filled = g.solid.iter().filter(|s| **s).count();
            let ratio = filled as f64 / g.cells() as f64;
            grid = Some(g);
            if filled == 0 || ratio > 0.7 || (filled as f64) > target as f64 * 0.7 { break; }
            want = ((target as f64 / ratio.max(0.02)) as usize).min(target * 40);
        }
        Mesh::from_grid(grid.unwrap())
    }

    pub fn from_grid(mut grid: Grid) -> Result<Mesh, String> {
        let n = grid.n;
        let total = grid.solid.iter().filter(|s| **s).count();
        if total == 0 { return Err("Das Modell ist für das Simulationsnetz zu dünn oder enthält kein Volumen.".into()); }
        // keep the largest face-connected body: loose cells would float freely
        let mut comp = vec![0u32; grid.cells()];
        let mut sizes = vec![0usize];
        let mut stack = vec![];
        for start in 0..grid.cells() {
            if !grid.solid[start] || comp[start] != 0 { continue; }
            let id = sizes.len() as u32;
            sizes.push(0);
            comp[start] = id;
            stack.push(start);
            while let Some(c) = stack.pop() {
                sizes[id as usize] += 1;
                let (i, j, k) = (c % n[0], (c / n[0]) % n[1], c / (n[0] * n[1]));
                let mut visit = |q: usize| if grid.solid[q] && comp[q] == 0 { comp[q] = id; stack.push(q); };
                if i > 0 { visit(c - 1); }
                if i + 1 < n[0] { visit(c + 1); }
                if j > 0 { visit(c - n[0]); }
                if j + 1 < n[1] { visit(c + n[0]); }
                if k > 0 { visit(c - n[0] * n[1]); }
                if k + 1 < n[2] { visit(c + n[0] * n[1]); }
            }
        }
        let main = (1..sizes.len()).max_by_key(|&i| sizes[i]).unwrap() as u32;
        for c in 0..grid.cells() { grid.solid[c] = comp[c] == main; }
        let kept = sizes[main as usize];

        let nn = [n[0] + 1, n[1] + 1, n[2] + 1];
        let mut node_id = vec![NONE; nn[0] * nn[1] * nn[2]];
        let mut cell_elem = vec![NONE; grid.cells()];
        let (mut elems, mut elem_ijk, mut nodes, mut node_elems) = (Vec::with_capacity(kept), Vec::with_capacity(kept), vec![], vec![]);
        for k in 0..n[2] {
            for j in 0..n[1] {
                for i in 0..n[0] {
                    let c = grid.cell(i, j, k);
                    if !grid.solid[c] { continue; }
                    let e = elems.len() as u32;
                    let mut en = [0u32; 8];
                    for a in 0..8 {
                        let (ni, nj, nk) = (i + (a & 1), j + (a >> 1 & 1), k + (a >> 2 & 1));
                        let g = ni + nn[0] * (nj + nn[1] * nk);
                        if node_id[g] == NONE {
                            node_id[g] = nodes.len() as u32;
                            nodes.push(grid.origin + V3::new(ni as f64 * grid.h.x, nj as f64 * grid.h.y, nk as f64 * grid.h.z));
                            node_elems.push([NONE; 8]);
                        }
                        en[a] = node_id[g];
                        node_elems[node_id[g] as usize][a] = e;
                    }
                    cell_elem[c] = e;
                    elems.push(en);
                    elem_ijk.push([i as u16, j as u16, k as u16]);
                }
            }
        }
        Ok(Mesh { grid, elems, elem_ijk, cell_elem, nodes, node_elems, dropped: 1.0 - kept as f64 / total as f64 })
    }

    pub fn elem_volume(&self) -> f64 { self.grid.h.x * self.grid.h.y * self.grid.h.z }

    /// The element that carries surface point `p` (outward normal `n`, zero if unknown) and
    /// the point's coordinates inside it (0..1 per axis, clamped).
    pub fn locate(&self, p: V3, n: V3) -> Option<(usize, [f64; 3])> {
        let g = &self.grid;
        let local = |c: [usize; 3]| {
            let r = (p - g.origin) / g.h;
            [(r.x - c[0] as f64).clamp(0.0, 1.0), (r.y - c[1] as f64).clamp(0.0, 1.0), (r.z - c[2] as f64).clamp(0.0, 1.0)]
        };
        if n != V3::ZERO {
            for s in [0.3, 0.8, 1.4] {
                if let Some(c) = g.cell_at(p - n * g.h * s) {
                    let e = self.cell_elem[g.cell(c[0], c[1], c[2])];
                    if e != NONE { return Some((e as usize, local(c))); }
                }
            }
        }
        let r = (p - g.origin) / g.h;
        let base = [r.x.floor() as i64, r.y.floor() as i64, r.z.floor() as i64];
        let mut best: Option<(f64, [usize; 3])> = None;
        for dk in -2..=2i64 {
            for dj in -2..=2i64 {
                for di in -2..=2i64 {
                    let c = [base[0] + di, base[1] + dj, base[2] + dk];
                    if (0..3).any(|a| c[a] < 0 || c[a] >= g.n[a] as i64) { continue; }
                    let c = [c[0] as usize, c[1] as usize, c[2] as usize];
                    if self.cell_elem[g.cell(c[0], c[1], c[2])] == NONE { continue; }
                    let d: f64 = (0..3).map(|a| { let x = r[a] - c[a] as f64; let o = if x < 0.0 { -x } else if x > 1.0 { x - 1.0 } else { 0.0 }; o * o }).sum();
                    if best.map_or(true, |b| d < b.0) { best = Some((d, c)); }
                }
            }
        }
        best.map(|(_, c)| (self.cell_elem[g.cell(c[0], c[1], c[2])] as usize, local(c)))
    }

    /// Trilinear weights of the 8 corners for local coordinates `l`.
    pub fn weights(l: [f64; 3]) -> [f64; 8] {
        let mut w = [0.0; 8];
        for (a, wa) in w.iter_mut().enumerate() {
            *wa = (if a & 1 == 1 { l[0] } else { 1.0 - l[0] }) * (if a >> 1 & 1 == 1 { l[1] } else { 1.0 - l[1] }) * (if a >> 2 & 1 == 1 { l[2] } else { 1.0 - l[2] });
        }
        w
    }

    /// Spreads the area of the given faces over the mesh nodes lying on them.
    pub fn map_faces(&self, topo: &Topology, faces: &[usize]) -> FaceMap {
        let g = &self.grid;
        let max_edge = g.h.min_element() * 0.5;
        let mut acc: HashMap<u32, (f64, V3)> = HashMap::new();
        let (mut area, mut cen) = (0.0, V3::ZERO);
        for &f in faces {
            let Some(face) = topo.faces.get(f) else { continue };
            for &pi in &face.polys {
                let p = &topo.polys[pi];
                let n = topo.planes[pi].n;
                let hn = n.x.abs() * g.h.x + n.y.abs() * g.h.y + n.z.abs() * g.h.z;
                for k in 1..p.len() - 1 {
                    subdivide([topo.positions[p[0]], topo.positions[p[k]], topo.positions[p[k + 1]]], max_edge, 14, &mut |t| {
                        let a = (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5;
                        if a <= 0.0 { return; }
                        let c = (t[0] + t[1] + t[2]) / 3.0;
                        area += a;
                        cen += c * a;
                        let Some((e, l)) = self.locate(c, n) else { return };
                        let w = Mesh::weights(l);
                        // only the element corners on the surface side carry the face
                        let mut sel = [0.0; 8];
                        let mut sum = 0.0;
                        for s in 0..8 {
                            let x = self.nodes[self.elems[e][s] as usize];
                            if (x - c).dot(n) >= -0.5 * hn { sel[s] = w[s] + 1e-9; sum += sel[s]; }
                        }
                        if sum <= 0.0 { sel = w; sum = 1.0; }
                        for s in 0..8 {
                            if sel[s] > 0.0 {
                                let en = acc.entry(self.elems[e][s]).or_insert((0.0, V3::ZERO));
                                en.0 += a * sel[s] / sum;
                                en.1 += n * (a * sel[s] / sum);
                            }
                        }
                    });
                }
            }
        }
        let mut nodes: Vec<(u32, f64, V3)> = acc.into_iter().map(|(n, (a, v))| (n, a, v)).collect();
        nodes.sort_by_key(|x| x.0);
        FaceMap { nodes, area, centroid: if area > 0.0 { cen / area } else { V3::ZERO } }
    }
}
