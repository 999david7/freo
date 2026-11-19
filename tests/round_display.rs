//! Rounds must look like Creo's: smooth shading across the round, black edges only at real
//! boundaries (no hard line where a round runs tangentially into its faces, no stray chords).

use freo::kernel::math::*;
use freo::mcp::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, tool: &str, args: Value) -> Value {
    let out = s.call(tool, args.as_object().unwrap()).unwrap_or_else(|e| panic!("{}: {}", tool, e));
    out.value
}

/// Upright 40 x 80 x 20 block whose lower end is a half circle (two R20 rounds on the bottom
/// edges) with an R3 round all around its front face.
fn rounded_slab(front_round: bool) -> Session {
    let mut s = Session::new();
    call(&mut s, "new_part", json!({}));
    call(&mut s, "add_extrude", json!({ "plane": "FRONT", "shapes": [{ "type": "rect", "width": 40, "height": 80, "cy": 40 }], "depth": 20 }));
    call(&mut s, "add_round", json!({ "edges": { "direction": "Z", "face_dir": "-Y" }, "radius": 20 }));
    if front_round {
        call(&mut s, "add_round", json!({ "edges": { "face_dir": "+Z" }, "radius": 3 }));
    }
    if let Ok(dir) = std::env::var("FREO_ROUND_SAVE") {
        let name = if front_round { "slab_front.pfprt" } else { "slab.pfprt" };
        call(&mut s, "save_model", json!({ "path": format!("{}/{}", dir, name) }));
    }
    s
}

fn edge_polys(s: &Session) -> std::collections::HashMap<(usize, usize), Vec<usize>> {
    let t = &s.result.topo;
    let mut by_edge: std::collections::HashMap<(usize, usize), Vec<usize>> = Default::default();
    for (pi, p) in t.polys.iter().enumerate() {
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push(pi);
        }
    }
    by_edge
}

fn vnormal(s: &Session, pi: usize, v: usize) -> V3 {
    let t = &s.result.topo;
    safe_norm(t.vnormals[pi][t.polys[pi].iter().position(|&x| x == v).unwrap()], V3::Z)
}

/// Drawn edges along which the shading runs on without any crease: tangent seams drawn as lines.
fn tangent_drawn_edges(s: &Session) -> Vec<(V3, V3)> {
    let t = &s.result.topo;
    let by_edge = edge_polys(s);
    let mut out = vec![];
    for &(a, b) in &t.draw_edges {
        let Some(ps) = by_edge.get(&(a.min(b), a.max(b))) else { continue };
        if ps.len() == 2 && [a, b].iter().all(|&v| vnormal(s, ps[0], v).dot(vnormal(s, ps[1], v)) > 1f64.to_radians().cos()) {
            out.push((t.positions[a], t.positions[b]));
        }
    }
    out
}

/// Largest angle between the shading normals two facets of one round wall use at a vertex they
/// share: a faceted round shows each facet's flat normal.
fn worst_shading_jump(s: &Session) -> f64 {
    let t = &s.result.topo;
    let mut at: std::collections::HashMap<usize, Vec<(usize, u32, V3)>> = Default::default();
    for (pi, p) in t.polys.iter().enumerate() {
        for (k, &v) in p.iter().enumerate() {
            at.entry(v).or_default().push((t.poly_face[pi], t.tags[pi], safe_norm(t.vnormals[pi][k], V3::Z)));
        }
    }
    let mut worst: f64 = 0.0;
    for list in at.values() {
        for (i, (fa, ta, na)) in list.iter().enumerate() {
            for (fb, tb, nb) in &list[i + 1..] {
                if fa == fb && ta == tb && !t.faces[*fa].planar {
                    let a = na.dot(*nb).clamp(-1.0, 1.0).acos().to_degrees();
                    worst = worst.max(a);
                }
            }
        }
    }
    worst
}

#[test]
fn half_round_end_has_no_seams_or_chord() {
    let s = rounded_slab(false);
    assert!(s.result.errors.is_empty(), "{:?}", s.result.errors);
    let t = &s.result.topo;
    // where the two R20 rounds run into the side faces (x = ±20, y = 20) and into each other at
    // the bottom (x = 0, y = 0): smooth transitions, nothing drawn
    let on_seam = |p: V3| ((p.x.abs() - 20.0).abs() < 1e-3 && (p.y - 20.0).abs() < 1e-3) || (p.x.abs() < 1e-3 && p.y.abs() < 1e-3);
    let drawn: Vec<_> = t.draw_edges.iter().map(|&(a, b)| (t.positions[a], t.positions[b])).filter(|(a, b)| on_seam(*a) && on_seam(*b) && (a.z - b.z).abs() > 1.0).collect();
    assert!(drawn.is_empty(), "tangent seams drawn: {:?}", drawn);
    // the outline of the front and back faces is still drawn: 2 x (3 straight sides + half circle)
    let outline = t.draw_edges.iter().filter(|&&(a, b)| (t.positions[a].z - t.positions[b].z).abs() < 1e-6).count();
    assert!(outline >= 2 * (3 + 15), "front and back outlines must stay: {} segments", outline);
    // tangent seams are no edges to round or chamfer either
    assert_eq!(t.chains.iter().filter(|c| c.points.iter().all(|p| on_seam(*p))).count(), 0);
}

#[test]
fn rounds_meet_their_faces_without_drawn_seams() {
    for front in [false, true] {
        let s = rounded_slab(front);
        assert!(s.result.errors.is_empty(), "{:?}", s.result.errors);
        // no visible gaps: a slit where a round runs into a face is drawn as a stray line
        // (slivers of 1e-4 mm from the BSP's rounding are far below a pixel)
        let by_edge = edge_polys(&s);
        let t = &s.result.topo;
        let open: Vec<_> =
            by_edge.iter().filter(|(_, ps)| ps.len() != 2).map(|(k, _)| (t.positions[k.0], t.positions[k.1])).filter(|(a, b)| (*a - *b).length() > 0.05).collect();
        assert!(open.is_empty(), "front round {}: open edges {:?}", front, &open[..open.len().min(4)]);
        let seams = tangent_drawn_edges(&s);
        assert!(seams.is_empty(), "front round {}: {} tangent seams drawn, e.g. {:?}", front, seams.len(), &seams[..seams.len().min(4)]);
    }
}

#[test]
fn round_surfaces_shade_smoothly() {
    for front in [false, true] {
        let s = rounded_slab(front);
        let jump = worst_shading_jump(&s);
        assert!(jump < 1.0, "front round {}: shading normals jump {:.1}° inside a round", front, jump);
    }
}


/// 100 x 60 x 40 block with a 40 x 30 x 20 pocket cut into its top; an R8 round on one inner
/// (concave) vertical corner of the pocket.
fn pocket(through: bool) -> Session {
    let mut s = Session::new();
    call(&mut s, "new_part", json!({}));
    call(&mut s, "add_extrude", json!({ "plane": "TOP", "shapes": [{ "type": "rect", "width": 100, "height": 60 }], "depth": 40 }));
    let cut = if through { json!({ "plane": "+Y", "shapes": [{ "type": "rect", "width": 40, "height": 30 }], "depth_type": "through_all", "remove": true }) } else { json!({ "plane": "+Y", "shapes": [{ "type": "rect", "width": 40, "height": 30 }], "depth": 20, "remove": true }) };
    call(&mut s, "add_extrude", cut);
    call(&mut s, "add_round", json!({ "edges": { "direction": "Y", "convex": false }, "radius": 8 }));
    if let Ok(dir) = std::env::var("FREO_ROUND_SAVE") {
        call(&mut s, "save_model", json!({ "path": format!("{}/pocket{}.pfprt", dir, if through { "_through" } else { "" }) }));
    }
    s
}

#[test]
fn round_in_a_pocket_corner_stays_inside_the_part() {
    for through in [false, true] {
        let s = pocket(through);
        assert!(s.result.errors.is_empty(), "{:?}", s.result.errors);
        let t = &s.result.topo;
        // the filler of a concave round used to overshoot the edge's ends: a 0.16 mm lip with a
        // triangular cap above the top face (and below the bottom one, for a through cut)
        assert!(t.bbox.min.y > -1e-6 && t.bbox.max.y < 40.0 + 1e-6, "through {}: part grew to {:?} .. {:?}", through, t.bbox.min, t.bbox.max);
        // block - pocket + four corner fillers (polygonal arc of 15 steps)
        let depth = if through { 40.0 } else { 20.0 };
        let sector = 0.5 * 64.0 * 15.0 * 6f64.to_radians().sin();
        let expect = 100.0 * 60.0 * 40.0 - 40.0 * 30.0 * depth + 4.0 * (64.0 - sector) * depth;
        let v = freo::kernel::csg::signed_volume(&s.result.polys);
        assert!((v - expect).abs() < 0.5, "through {}: volume {} expected {}", through, v, expect);
        let by_edge = edge_polys(&s);
        let open = by_edge.iter().filter(|(k, ps)| ps.len() != 2 && (t.positions[k.0] - t.positions[k.1]).length() > 0.05).count();
        assert_eq!(open, 0, "through {}: open edges", through);
        assert!(tangent_drawn_edges(&s).is_empty(), "through {}: tangent seams drawn", through);
    }
}
