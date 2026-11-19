//! The board as Freo geometry: the laminate as a real extrude (outline sketch with the drill
//! holes) so enclosures can reference its faces, plus copper and the component bodies as
//! import features; together as an assembly for enclosure design.
//!
//! World frame: board x → X, board y (down) → Z, the top side faces +Y (the board lies on
//! the OBEN/TOP plane).

use super::board::{Board, B_CU, F_CU};
use super::footprints::Body3;
use super::geom::{p, P};
use crate::model::assembly::{AsmCType, AsmConstraint, AssemblyData, Component, Placement};
use crate::model::features::*;
use crate::sketch::sketch::Sketch;
use glam::DVec2;

pub const COPPER_T: f64 = 0.035;

/// Laminate part: outline and cut-outs extruded by the board thickness, with every drill.
pub fn board_part(b: &Board, name: &str) -> Result<ModelData, String> {
    if b.outline.is_empty() {
        return Err("Die Platine hat keinen Umriss".into());
    }
    let mut sk = Sketch::default();
    for lp in &b.outline {
        let ids: Vec<i64> = lp.iter().map(|q| sk.add_point(DVec2::new(q.x, -q.y))).collect();
        for i in 0..ids.len() {
            sk.add_line(ids[i], ids[(i + 1) % ids.len()]);
        }
    }
    let mut holes: Vec<(P, f64)> = b.vias.iter().map(|v| (v.pos, v.drill)).collect();
    let mut slots: Vec<(P, P, f64)> = vec![];
    for f in &b.footprints {
        for pad in f.fp.pads.iter() {
            if pad.drill > 0.0 {
                if pad.slot > pad.drill {
                    let (a, e) = pad.hole_ends(f.pos, f.rot, f.bottom);
                    slots.push((a, e, pad.drill / 2.0));
                } else {
                    holes.push((pad.hole_ends(f.pos, f.rot, f.bottom).0, pad.drill));
                }
            }
        }
    }
    holes.sort_by(|a, c| (a.0.x, a.0.y).partial_cmp(&(c.0.x, c.0.y)).unwrap());
    holes.dedup_by(|a, c| (a.0 - c.0).length() < 1e-6);
    for (q, d) in &holes {
        let c = sk.add_point(DVec2::new(q.x, -q.y));
        sk.add_circle(c, d / 2.0);
    }
    // slots as closed loops (two lines and two half circles)
    for (a, e, r) in &slots {
        let d = (*e - *a).normalize_or_zero();
        let n = p(-d.y, d.x) * *r;
        let w = |q: P| DVec2::new(q.x, -q.y);
        let pts = [sk.add_point(w(*a + n)), sk.add_point(w(*e + n)), sk.add_point(w(*e - n)), sk.add_point(w(*a - n))];
        sk.add_line(pts[0], pts[1]);
        sk.add_line(pts[2], pts[3]);
        // arcs counter-clockwise in sketch coordinates (y flipped)
        let ce = sk.add_point(w(*e));
        let ca = sk.add_point(w(*a));
        sk.add_arc(ce, pts[2], pts[1]);
        sk.add_arc(ca, pts[0], pts[3]);
    }
    let mut m = ModelData::new_part_template(name);
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(TOP_ID));
    e.sketch = sk;
    e.depth = Dim::v(b.thickness);
    m.features.push(Feature { id: 10, name: "Leiterplatte".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
    m.next_id = 20;
    m.appearance.color = [0.12, 0.42, 0.22];
    m.density = 1.85e-6;
    m.parameters.push(Parameter { name: "DICKE".into(), expr: format!("{}", b.thickness), note: "Leiterplattendicke".into() });
    Ok(m)
}

/// Triangles wound so their normal points up (+Y) or down; points are (x, z).
fn facing(tri: &[[usize; 3]], pts: &[P], up: bool) -> Vec<[u32; 3]> {
    tri.iter()
        .map(|t| {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            // y component of (b - a) × (c - a) in (x, y, z) with board y as z
            let ny = (b.y - a.y) * (c.x - a.x) - (b.x - a.x) * (c.y - a.y);
            if (ny > 0.0) == up {
                [t[0] as u32, t[1] as u32, t[2] as u32]
            } else {
                [t[0] as u32, t[2] as u32, t[1] as u32]
            }
        })
        .collect()
}

struct Mesh {
    faces: Vec<ImportFace>,
}

fn v3(q: P, y: f64) -> [f32; 3] {
    [q.x as f32, y as f32, q.y as f32]
}

impl Mesh {
    fn face(&mut self, verts: Vec<([f32; 3], [f32; 3])>, tris: Vec<[u32; 3]>) {
        self.faces.push(ImportFace { v: verts.into_iter().map(|(a, n)| [a[0], a[1], a[2], n[0], n[1], n[2]]).collect(), t: tris });
    }
    /// Vertical prism over the (convex or simple) ring between heights y0 and y1.
    fn prism(&mut self, ring: &[P], y0: f64, y1: f64, smooth: bool) {
        let n = ring.len();
        if n < 3 || (y1 - y0).abs() < 1e-9 {
            return;
        }
        // clockwise in (x, z): the left-hand normals and the side quads face outwards
        let a = super::geom::area(ring);
        let ring: Vec<P> = if a < 0.0 { ring.to_vec() } else { ring.iter().rev().copied().collect() };
        let tri = crate::kernel::triangulate::triangulate(&ring.iter().map(|q| DVec2::new(q.x, q.y)).collect::<Vec<_>>(), &[]);
        let up = [0.0, 1.0, 0.0];
        let down = [0.0, -1.0, 0.0];
        self.face(ring.iter().map(|q| (v3(*q, y1), up)).collect(), facing(&tri, &ring, true));
        self.face(ring.iter().map(|q| (v3(*q, y0), down)).collect(), facing(&tri, &ring, false));
        let normal = |i: usize| {
            let d = ring[(i + 1) % n] - ring[i];
            let nn = p(-d.y, d.x).normalize_or_zero();
            [nn.x as f32, 0.0, nn.y as f32]
        };
        if smooth {
            let mut verts = vec![];
            let mut tris = vec![];
            for i in 0..n {
                let pn = normal((i + n - 1) % n);
                let cn = normal(i);
                let nv = [(pn[0] + cn[0]) / 2.0, 0.0, (pn[2] + cn[2]) / 2.0];
                verts.push((v3(ring[i], y0), nv));
                verts.push((v3(ring[i], y1), nv));
            }
            for i in 0..n {
                let j = (i + 1) % n;
                let (a0, a1, b0, b1) = (2 * i as u32, 2 * i as u32 + 1, 2 * j as u32, 2 * j as u32 + 1);
                tris.push([a0, b0, b1]);
                tris.push([a0, b1, a1]);
            }
            self.face(verts, tris);
        } else {
            for i in 0..n {
                let j = (i + 1) % n;
                let nv = normal(i);
                self.face(vec![(v3(ring[i], y0), nv), (v3(ring[j], y0), nv), (v3(ring[j], y1), nv), (v3(ring[i], y1), nv)], vec![[0, 1, 2], [0, 2, 3]]);
            }
        }
    }
}

/// Places a footprint-local mesh (x, y down, z up from the board top) on the board in world
/// coordinates; triangles are re-wound where the placement mirrors (bottom side).
fn place_local(f: &super::board::FpInst, t: f64, verts: &[[f32; 6]], tris: &[[u32; 3]]) -> ImportFace {
    let o = f.xform(P::ZERO);
    let z = |h: f64| if f.bottom { -h } else { t + h };
    let v: Vec<[f32; 6]> = verts
        .iter()
        .map(|q| {
            let w = f.xform(p(q[0] as f64, q[1] as f64));
            let nw = f.xform(p(q[3] as f64, q[4] as f64)) - o;
            let ny = if f.bottom { -q[5] as f64 } else { q[5] as f64 };
            [w.x as f32, z(q[2] as f64) as f32, w.y as f32, nw.x as f32, ny as f32, nw.y as f32]
        })
        .collect();
    let t: Vec<[u32; 3]> = tris
        .iter()
        .map(|tr| {
            let (a, b, c) = (v[tr[0] as usize], v[tr[1] as usize], v[tr[2] as usize]);
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let vn = [a[3] + b[3] + c[3], a[4] + b[4] + c[4], a[5] + b[5] + c[5]];
            if n[0] * vn[0] + n[1] * vn[1] + n[2] * vn[2] < 0.0 {
                [tr[0], tr[2], tr[1]]
            } else {
                *tr
            }
        })
        .collect();
    ImportFace { v, t }
}

/// Component bodies grouped by material: STEP models where found, otherwise generated
/// models (leads, terminations, colour bands …), otherwise the simple footprint bodies.
pub fn component_meshes(b: &Board) -> Vec<(super::model3d::Mat, Vec<ImportFace>)> {
    use super::model3d::Mat;
    let mut out: Vec<(Mat, Vec<ImportFace>)> = vec![];
    let mut push = |m: Mat, f: ImportFace| match out.iter_mut().find(|x| x.0 == m) {
        Some(x) => x.1.push(f),
        None => out.push((m, vec![f])),
    };
    let t = b.thickness;
    for f in &b.footprints {
        let z = |h: f64| if f.bottom { -h } else { t + h };
        // a STEP model when the footprint names one that can be found
        if let Some(faces) = f.fp.model.as_deref().and_then(super::models::resolve).and_then(|p| super::models::load(&p)) {
            let (off, r) = (f.fp.model_offset, f.fp.model_rot);
            for face in faces.iter() {
                let local: Vec<[f32; 6]> = face
                    .v
                    .iter()
                    .map(|v| {
                        let q = super::models::rotate([v[0] as f64, v[1] as f64, v[2] as f64], r);
                        let nq = super::models::rotate([v[3] as f64, v[4] as f64, v[5] as f64], r);
                        [(q[0] + off[0]) as f32, -(q[1] + off[1]) as f32, (q[2] + off[2]) as f32, nq[0] as f32, -nq[1] as f32, nq[2] as f32]
                    })
                    .collect();
                push(Mat::Plastic, place_local(f, t, &local, &face.t));
            }
            continue;
        }
        if let Some(model) = super::model3d::generate(&f.fp, &f.value, t) {
            for (mat, v, tris) in &model.parts {
                push(*mat, place_local(f, t, v, tris));
            }
            continue;
        }
        let mut m = Mesh { faces: vec![] };
        for body in &f.fp.body {
            match body {
                Body3::Box { lo, hi, z0, z1, .. } => {
                    let ring: Vec<P> = [*lo, p(hi.x, lo.y), *hi, p(lo.x, hi.y)].iter().map(|q| f.xform(*q)).collect();
                    let (a, c) = (z(*z0), z(*z1));
                    m.prism(&ring, a.min(c), a.max(c), false);
                }
                Body3::Cyl { c, r, z0, z1, .. } => {
                    let ring: Vec<P> = (0..32).map(|i| f.xform(*c + p((i as f64 / 32.0 * std::f64::consts::TAU).cos(), (i as f64 / 32.0 * std::f64::consts::TAU).sin()) * *r)).collect();
                    let (a, cc) = (z(*z0), z(*z1));
                    m.prism(&ring, a.min(cc), a.max(cc), true);
                }
            }
        }
        for face in m.faces {
            push(Mat::Plastic, face);
        }
    }
    out
}

/// Component bodies as one import feature (all materials together).
pub fn parts_import(b: &Board) -> ImportDef {
    let faces = component_meshes(b).into_iter().flat_map(|x| x.1).collect();
    ImportDef { source: "Leiterplatte (Bauteile)".into(), format: "Freo-PCB".into(), faces }
}

/// Outer copper (pads and tracks on top and bottom) as one import feature.
pub fn copper_import(b: &Board) -> ImportDef {
    let mut m = Mesh { faces: vec![] };
    let t = b.thickness;
    let span = |l: u8| if l == F_CU { Some((t, t + COPPER_T)) } else if l == B_CU { Some((-COPPER_T, 0.0)) } else { None };
    for c in b.copper() {
        for l in [F_CU, B_CU] {
            if c.layers & (1 << l) == 0 {
                continue;
            }
            let (y0, y1) = span(l).unwrap();
            m.prism(&c.hull.polygon(16), y0, y1, false);
        }
    }
    for z in &b.zones {
        let Some((y0, y1)) = span(z.layer) else { continue };
        for shape in &z.fill {
            let outer: Vec<DVec2> = shape[0].iter().map(|q| DVec2::new(q[0], q[1])).collect();
            let holes: Vec<Vec<DVec2>> = shape[1..].iter().map(|h| h.iter().map(|q| DVec2::new(q[0], q[1])).collect()).collect();
            let all: Vec<DVec2> = outer.iter().chain(holes.iter().flatten()).copied().collect();
            let tri = crate::kernel::triangulate::triangulate(&outer, &holes);
            let up = [0.0, 1.0, 0.0];
            let pts: Vec<P> = all.iter().map(|q| p(q.x, q.y)).collect();
            m.face(pts.iter().map(|q| (v3(*q, y1), up)).collect(), facing(&tri, &pts, true));
            let _ = y0;
        }
    }
    ImportDef { source: "Leiterplatte (Kupfer)".into(), format: "Freo-PCB".into(), faces: m.faces }
}

fn import_part(name: &str, def: ImportDef, color: [f32; 3]) -> ModelData {
    let mut m = ModelData::new_part_template(name);
    // only the coordinate system: dozens of OBEN/VORNE/RECHTS planes would crowd the board
    m.features.retain(|f| matches!(f.kind, FeatureKind::Csys));
    m.features.push(Feature { id: 10, name: "Import 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Import(def) });
    m.next_id = 20;
    m.appearance.color = color;
    m
}

/// Assembly of laminate, copper and components, each fixed at the default position.
pub fn assembly(b: &Board, name: &str) -> Result<AssemblyData, String> {
    let base = name.to_uppercase();
    let board = board_part(b, &format!("{}_LP", base))?;
    let copper = import_part(&format!("{}_KUPFER", base), copper_import(b), [0.85, 0.66, 0.30]);
    // one part per material, so the components show their colours
    let mut mats = component_meshes(b);
    mats.sort_by_key(|x| x.0);
    let parts: Vec<ModelData> = mats
        .into_iter()
        .map(|(mat, faces)| import_part(&format!("{}_{}", base, mat.name()), ImportDef { source: format!("Leiterplatte (Bauteile, {})", mat.name().to_lowercase()), format: "Freo-PCB".into(), faces }, mat.rgb()))
        .collect();
    let mut asm = AssemblyData::new(&format!("{}_ASM", base));
    let mut id = 100;
    for m in [board, copper].into_iter().chain(parts) {
        let n = asm.add_part(m, None);
        let mut c = Component::new(id, &n, Placement::default());
        c.constraints.push(AsmConstraint::new(AsmCType::Default));
        asm.components.push(c);
        id += 1;
    }
    asm.next_id = id;
    Ok(asm)
}
