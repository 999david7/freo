//! 3D-Druck: real Freo parts sliced end to end, with the G-code read back and checked.

use freo::app::export;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::Sketch;
use freo::slicer::config::*;
use freo::slicer::mesh::{Mesh, Placement};
use freo::slicer::plan::Role;
use freo::slicer::{self, printers, Progress, Settings};

fn extrude(m: &mut ModelData, id: u64, sk: Sketch, depth: f64, remove: bool, plane: PlaneRef) {
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(plane);
    e.sketch = sk;
    e.depth = Dim::v(depth);
    e.remove = remove;
    if remove {
        e.depth_type = DepthType::ThroughAll;
    }
    m.features.push(Feature { id, name: format!("F{}", id), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
}

fn part(m: &ModelData) -> Mesh {
    let r = regen::run(m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    Mesh::from_world_triangles(&export::triangles(&r))
}

/// 20 × 20 × 10 block on the TOP plane.
fn block() -> Mesh {
    let mut m = ModelData::new_part("BLOCK");
    extrude(&mut m, 10, Sketch::rect(20.0, 20.0, 0.0, 0.0), 10.0, false, PlaneRef::Datum(TOP_ID));
    part(&m)
}

fn settings() -> Settings {
    Settings::default()
}

fn run(mesh: &Mesh, s: &Settings) -> slicer::Sliced {
    let placed = Placement::default().apply(mesh, s.printer.bed_size());
    slicer::slice(placed, s, &Progress::default()).expect("slice")
}

/// Reads G-code back: total extrusion (mm of filament), XY bounds, Z of extrusions, and
/// any malformed numbers.
struct Readback {
    e: f64,
    min: [f64; 2],
    max: [f64; 2],
    zs: Vec<f64>,
    extrude_moves: usize,
}

fn read(g: &str) -> Readback {
    let mut rb = Readback { e: 0.0, min: [f64::INFINITY; 2], max: [f64::NEG_INFINITY; 2], zs: vec![], extrude_moves: 0 };
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let mut relative = false;
    let mut in_start = true;
    for line in g.lines() {
        // the print itself: from the first layer change to the end G-code
        if line.starts_with(";LAYER_CHANGE") {
            in_start = false;
        }
        if line.starts_with("M73 P100") {
            break;
        }
        let code = line.split(';').next().unwrap().trim();
        if code.is_empty() {
            continue;
        }
        if code == "M83" {
            relative = true;
            continue;
        }
        let mut it = code.split_whitespace();
        let cmd = it.next().unwrap();
        if cmd != "G0" && cmd != "G1" {
            continue;
        }
        let mut e = None;
        for w in it {
            let (k, v) = w.split_at(1);
            let v: f64 = v.parse().unwrap_or_else(|_| panic!("bad number in {:?}", line));
            assert!(v.is_finite(), "{}", line);
            match k {
                "X" => x = v,
                "Y" => y = v,
                "Z" => z = v,
                "E" => e = Some(v),
                "F" => assert!(v > 0.0, "{}", line),
                _ => {}
            }
        }
        if in_start || !relative {
            continue;
        }
        if let Some(e) = e {
            rb.e += e;
            if e > 0.0 && cmd == "G1" {
                rb.extrude_moves += 1;
                rb.min = [rb.min[0].min(x), rb.min[1].min(y)];
                rb.max = [rb.max[0].max(x), rb.max[1].max(y)];
                rb.zs.push(z);
            }
        }
    }
    rb
}

#[test]
fn block_slices_into_layers_with_shells() {
    let s = settings();
    let r = run(&block(), &s);
    let layers = &r.plan.layers;
    assert_eq!(layers.len(), 50, "0.2 mm layers on a 10 mm block");
    assert!((layers.last().unwrap().z - 10.0).abs() < 1e-9);
    for (i, l) in layers.iter().enumerate() {
        assert!(l.paths.iter().any(|p| p.role == Role::OuterWall), "layer {} has no outer wall", i);
    }
    let has = |i: usize, r: Role| layers[i].paths.iter().any(|p| p.role == r);
    assert!(has(0, Role::BottomSurface));
    assert!(has(49, Role::TopSurface));
    assert!(has(25, Role::SparseInfill));
    assert!(!has(25, Role::TopSurface) && !has(25, Role::InternalSolid));
    // three bottom and five top layers are solid
    for i in [1, 2] {
        assert!(has(i, Role::InternalSolid) && !has(i, Role::SparseInfill), "layer {}", i);
    }
    for i in 45..49 {
        assert!(has(i, Role::InternalSolid), "layer {}", i);
    }
    assert!(r.out.warnings.is_empty(), "{:?}", r.out.warnings);
}

#[test]
fn extruded_volume_matches_a_solid_part() {
    // 100 % infill: the plastic laid down must equal the part's volume
    let mut s = settings();
    s.process.infill_density = 1.0;
    s.process.elephant_foot = 0.0;
    s.filament.flow_ratio = 1.0;
    let m = block();
    let vol = m.volume();
    assert!((vol - 4000.0).abs() < 1e-6);
    let r = run(&m, &s);
    let rb = read(&r.out.gcode);
    let plastic = rb.e * s.filament.area();
    let ratio = plastic / vol;
    assert!((0.95..1.04).contains(&ratio), "extruded {:.1} mm³ for a {:.1} mm³ part ({:.3})", plastic, vol, ratio);
    // the read-back G-code agrees with the slicer's own statistics
    assert!((rb.e - r.out.stats.filament_mm).abs() < 0.05 + rb.e * 1e-4, "{} vs {}", rb.e, r.out.stats.filament_mm);
}

#[test]
fn sparse_part_uses_less_plastic() {
    let s = settings();
    let m = block();
    let r = run(&m, &s);
    let rb = read(&r.out.gcode);
    let ratio = rb.e * s.filament.area() / m.volume();
    // 2 walls, 3+5 solid layers, 15 % grid inside: well under half the solid volume
    assert!((0.3..0.65).contains(&ratio), "{}", ratio);
    // everything is on the bed, around the centre, inside the part's footprint (+ line width)
    assert!(rb.min[0] >= 120.0 - 0.3 && rb.max[0] <= 140.0 + 0.3, "{:?} {:?}", rb.min, rb.max);
    assert!(rb.min[1] >= 120.0 - 0.3 && rb.max[1] <= 140.0 + 0.3, "{:?} {:?}", rb.min, rb.max);
    // layers go up, never down
    assert!(rb.zs.windows(2).all(|w| w[1] >= w[0] - 1e-9));
    assert!((rb.zs[0] - 0.2).abs() < 1e-9 && (rb.zs.last().unwrap() - 10.0).abs() < 1e-9);
    // time estimate is plausible: slower than everything at the top speed
    let t = r.out.stats.time;
    assert!(t > 60.0 && t < 3600.0, "{}", t);
    assert!(r.out.stats.grams > 0.5 && r.out.stats.grams < 5.0, "{}", r.out.stats.grams);
}

#[test]
fn m1_pro_gcode_is_complete() {
    let s = settings();
    assert_eq!(s.printer.name(), "Artillery M1 Pro");
    let r = run(&block(), &s);
    let g = &r.out.gcode;
    assert!(r.out.warnings.is_empty(), "{:?}", r.out.warnings);
    assert!(g.contains("; generated by Freo"));
    assert!(g.contains("; thumbnail begin 300x300"));
    assert!(g.contains("M190 S55;"), "bed temperature from the PLA profile");
    assert!(g.contains("M109 S220;"));
    assert!(g.contains("SET_PRINT_STATS_INFO TOTAL_LAYER=50"));
    assert!(g.contains("TIMELAPSE_TAKE_FRAME"));
    assert!(g.contains("G1 Z12 F300"), "end lifts 2 mm above the 10 mm print");
    assert!(!g.contains('{') && !g.contains("[bed_temperature"), "unexpanded template");
    assert!(!g.contains("M191"), "chamber wait is for ABS/ASA/PC/PA only");
    // fan: off on the first layer, full from the second (PLA 100 %)
    assert!(g.contains("M106 S0\n") && g.contains("M106 S255"));
    // ;TYPE: comments for viewers
    for t in ["Outer wall", "Inner wall", "Sparse infill", "Top surface", "Bottom surface"] {
        assert!(g.contains(&format!(";TYPE:{}", t)), "{}", t);
    }
    // retraction 0.8 mm at 40 mm/s, z-hop 0.6
    assert!(g.contains("G1 E-0.56 F2400") || g.contains("G1 E-0.8 F2400"));
    // ABS adds the chamber heating block
    let mut abs = s.clone();
    abs.filament = printers::filament_presets().into_iter().find(|f| f.material == Material::Abs).unwrap();
    let r = run(&block(), &abs);
    assert!(r.out.gcode.contains("M191 S47;"));
}

#[test]
fn overhang_gets_support_only_when_enabled() {
    // a T: 10 × 10 post, 40 × 10 × 3 roof at 10..13 mm
    let mut m = ModelData::new_part("T");
    extrude(&mut m, 10, Sketch::rect(10.0, 10.0, 0.0, 0.0), 13.0, false, PlaneRef::Datum(TOP_ID));
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    // roof: an extrusion on the FRONT plane (sketch x → X, y → Y), 10 mm deep each way
    e.plane = Some(PlaneRef::Datum(FRONT_ID));
    e.sketch = Sketch::rect(40.0, 3.0, 0.0, 11.5);
    e.depth = Dim::v(5.0);
    e.side2 = Side2::Blind;
    e.depth2 = Dim::v(5.0);
    m.features.push(Feature { id: 20, name: "Dach".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
    let mesh = part(&m);
    let (lo, hi) = mesh.bbox();
    assert!((hi.z - 13.0).abs() < 1e-6 && lo.z.abs() < 1e-6, "{:?} {:?}", lo, hi);
    let mut s = settings();
    let r = run(&mesh, &s);
    let count = |r: &slicer::Sliced, role: Role| r.plan.layers.iter().flat_map(|l| &l.paths).filter(|p| p.role == role).count();
    assert_eq!(count(&r, Role::Support), 0);
    // the roof's underside is printed as a bridge without support
    assert!(count(&r, Role::Bridge) > 0);
    s.process.support = true;
    let r = run(&mesh, &s);
    assert!(count(&r, Role::Support) > 0);
    assert!(count(&r, Role::SupportInterface) > 0);
    // support stops below the roof (top z distance) and stays clear of the post (xy distance)
    let roof_layer = r.plan.layers.iter().position(|l| l.z > 10.0 + 1e-6).unwrap();
    for (i, l) in r.plan.layers.iter().enumerate() {
        for p in l.paths.iter().filter(|p| matches!(p.role, Role::Support | Role::SupportInterface)) {
            assert!(i + 1 < roof_layer, "support on layer {} under the roof at {}", i, roof_layer);
            for pt in &p.pts {
                let inside_post = (pt[0] - 130.0).abs() < 5.0 + 0.45 && (pt[1] - 130.0).abs() < 5.0 + 0.45;
                assert!(!inside_post, "support at {:?} touches the post", pt);
            }
        }
    }
}

#[test]
fn spiral_vase_climbs_continuously() {
    let mut m = ModelData::new_part("VASE");
    extrude(&mut m, 10, Sketch::circle(15.0, 0.0, 0.0), 20.0, false, PlaneRef::Datum(TOP_ID));
    let mesh = part(&m);
    let mut s = settings();
    s.process.spiral_vase = true;
    let r = run(&mesh, &s);
    let spiral: Vec<_> = r.out.moves.iter().filter(|m| m.kind == slicer::gcode::Kind::Extrude && m.layer >= 4).collect();
    assert!(!spiral.is_empty());
    // z never drops and changes within every layer
    assert!(spiral.windows(2).all(|w| w[1].b[2] >= w[0].b[2] - 1e-5));
    assert!(r.plan.layers[10].paths.len() == 1 && r.plan.layers[10].paths[0].spiral);
    assert!(!r.plan.layers.iter().skip(4).any(|l| l.paths.iter().any(|p| p.role == Role::SparseInfill || p.role == Role::TopSurface)));
}

#[test]
fn every_infill_pattern_and_printer_slices() {
    let m = block();
    for pat in InfillPattern::SPARSE {
        let mut s = settings();
        s.process.infill_pattern = pat;
        let r = run(&m, &s);
        assert!(r.plan.layers[25].paths.iter().any(|p| p.role == Role::SparseInfill), "{:?}", pat);
    }
    for p in printers::all_printers() {
        let mut s = settings();
        s.process = printers::process_presets_for(&p)[3].clone();
        s.filament.diameter = p.filament_diameter;
        s.printer = p.clone();
        let r = run(&m, &s);
        assert!(r.out.warnings.is_empty(), "{}: {:?}", p.name(), r.out.warnings);
        let rb = read(&r.out.gcode);
        assert!(rb.extrude_moves > 100, "{}", p.name());
        if p.origin_center {
            assert!(rb.min[0] > -15.0 && rb.max[0] < 15.0, "{} centre origin {:?}", p.name(), rb.min);
        }
    }
}

#[test]
fn brim_skirt_ironing_fuzzy() {
    let m = block();
    let mut s = settings();
    s.process.brim = Brim::Outer;
    s.process.skirt_loops = 2;
    s.process.ironing = Ironing::Top;
    s.process.fuzzy = Fuzzy::Outer;
    let r = run(&m, &s);
    let l0 = &r.plan.layers[0];
    assert_eq!(l0.paths.iter().filter(|p| p.role == Role::Skirt).count(), 2);
    let brims = l0.paths.iter().filter(|p| p.role == Role::Brim).count();
    assert!(brims >= 9, "{}", brims);
    assert!(r.plan.layers[49].paths.iter().any(|p| p.role == Role::Ironing));
    // fuzzy skin moves the outer wall off the straight edge
    let wall = r.plan.layers[20].paths.iter().find(|p| p.role == Role::OuterWall).unwrap();
    assert!(wall.pts.len() > 40);
}
