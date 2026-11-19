//! Leiterplatten: schematic → netlist → ERC, board update, ratsnest, routing, zones, DRC,
//! Gerber/Excellon, BOM, KiCad round trips and the 3D body.

use freo::pcb::board::{Track, Via, B_CU, F_CU};
use freo::pcb::geom::{p, P};
use freo::pcb::schematic::{Label, LabelKind, Schematic, Severity};
use freo::pcb::*;
use freo::pcb::fab;

fn two_resistors() -> Schematic {
    // R1 and R2 in series between two labels, the middle joined by a wire
    let mut s = Schematic::default();
    let a = s.add_symbol("Device:R", p(10.16, 10.16)).unwrap();
    let b = s.add_symbol("Device:R", p(20.32, 10.16)).unwrap();
    s.annotate(false);
    assert_eq!(s.symbols[a].reference, "R1");
    assert_eq!(s.symbols[b].reference, "R2");
    // pin 2 of R1 (10.16, 13.97) to pin 1 of R2 (20.32, 6.35) through an L-shaped wire
    s.add_wire(p(10.16, 13.97), p(15.24, 13.97));
    s.add_wire(p(15.24, 13.97), p(15.24, 6.35));
    s.add_wire(p(15.24, 6.35), p(20.32, 6.35));
    s.labels.push(Label { pos: p(10.16, 6.35), text: "IN".into(), kind: LabelKind::Local, rot: 0.0 });
    s.labels.push(Label { pos: p(20.32, 13.97), text: "OUT".into(), kind: LabelKind::Local, rot: 0.0 });
    s
}

#[test]
fn netlist_from_schematic() {
    let s = two_resistors();
    let n = s.netlist();
    assert_eq!(n.net_of("R1", "1"), Some("IN"));
    assert_eq!(n.net_of("R2", "2"), Some("OUT"));
    let mid = n.net_of("R1", "2").unwrap();
    assert_eq!(n.net_of("R2", "1"), Some(mid));
    assert!(mid.starts_with("Net-(R1-Pad2)"), "{}", mid);
    // labels with the same text join nets without wires
    let mut s2 = s.clone();
    s2.labels.push(Label { pos: p(20.32, 6.35), text: "IN".into(), kind: LabelKind::Local, rot: 0.0 });
    let n2 = s2.netlist();
    assert_eq!(n2.net_of("R2", "1"), Some("IN"));
    assert_eq!(n2.net_of("R1", "2"), Some("IN"));
}

#[test]
fn demo_netlist_and_clean_erc() {
    let s = demo::schematic();
    let n = s.netlist();
    assert_eq!(n.net_of("U1", "8"), Some("+5V"));
    assert_eq!(n.net_of("U1", "1"), Some("GND"));
    assert_eq!(n.net_of("U1", "2"), n.net_of("U1", "6"));
    assert_eq!(n.net_of("C1", "1"), Some("TRIG"));
    assert_eq!(n.net_of("J1", "A9"), Some("+5V"));
    assert_eq!(n.net_of("J1", "B9"), Some("+5V"), "gestapelte Pins");
    assert_eq!(n.net_of("D1", "1"), Some("GND"));
    let erc = s.erc();
    let errs: Vec<_> = erc.iter().filter(|e| e.severity == Severity::Error).collect();
    assert!(errs.is_empty(), "ERC: {:#?}", errs);
}

#[test]
fn erc_finds_problems() {
    // supply without PWR_FLAG: the NE555's power pins are not driven
    let mut s = demo::schematic();
    s.symbols.retain(|x| !x.lib.ends_with("PWR_FLAG"));
    let erc = s.erc();
    assert!(erc.iter().any(|e| e.severity == Severity::Error && e.msg.contains("nicht versorgt")), "{:#?}", erc);
    // unconnected pin, duplicate and missing references
    let mut s = Schematic::default();
    s.add_symbol("Device:R", p(0.0, 0.0));
    s.add_symbol("Device:R", p(10.16, 0.0));
    let erc = s.erc();
    assert!(erc.iter().any(|e| e.msg.contains("nicht annotiert")));
    s.annotate(false);
    s.symbols[1].reference = "R1".into();
    let erc = s.erc();
    assert!(erc.iter().any(|e| e.msg.contains("doppelt")));
    assert!(erc.iter().filter(|e| e.msg.contains("nicht angeschlossen")).count() >= 2);
    // a no-connect flag silences the pin
    let mut s = Schematic::default();
    s.add_symbol("Device:R", p(0.0, 0.0));
    s.annotate(false);
    s.no_connects.push(p(0.0, -3.81));
    s.no_connects.push(p(0.0, 3.81));
    assert!(s.erc().iter().all(|e| !e.msg.contains("nicht angeschlossen")), "{:#?}", s.erc());
    // two outputs on one net
    let mut s = Schematic::default();
    s.add_symbol("Timer:NE555P", p(0.0, 0.0));
    s.add_symbol("Timer:NE555P", p(40.64, 0.0));
    s.annotate(false);
    let q1 = s.pin_positions(0).into_iter().find(|(_, q)| q.num == "3").unwrap().0;
    let q2 = s.pin_positions(1).into_iter().find(|(_, q)| q.num == "3").unwrap().0;
    s.add_wire(q1, p(q1.x, q1.y - 7.62));
    s.add_wire(p(q1.x, q1.y - 7.62), p(q2.x, q1.y - 7.62));
    s.add_wire(p(q2.x, q1.y - 7.62), q2);
    assert!(s.erc().iter().any(|e| e.msg.contains("Ausgänge gegeneinander")));
}

#[test]
fn annotation_left_to_right() {
    let mut s = Schematic::default();
    s.add_symbol("Device:C", p(50.8, 0.0));
    s.add_symbol("Device:C", p(10.16, 0.0));
    s.add_symbol("Device:R", p(30.48, 0.0));
    let k = s.annotate(false);
    assert_eq!(k, 3);
    assert_eq!(s.symbols[1].reference, "C1");
    assert_eq!(s.symbols[0].reference, "C2");
    assert_eq!(s.symbols[2].reference, "R1");
}

#[test]
fn board_update_and_ratsnest() {
    let pr = demo::project();
    let b = &pr.board;
    assert_eq!(b.footprints.len(), 11);
    let u1 = &b.footprints[b.find_fp("U1").unwrap()];
    assert_eq!(u1.fp.id, "Package_DIP:DIP-8_W7.62mm");
    let k = u1.fp.pads.iter().position(|q| q.num == "8").unwrap();
    assert_eq!(u1.nets[k], "+5V");
    let j1 = &b.footprints[b.find_fp("J1").unwrap()];
    assert!(j1.fp.port.as_deref() == Some("USB-C / Thunderbolt"));
    // nothing routed yet: every multi-pad net needs pads-1 connections
    let rats = b.ratsnest();
    let gnd = rats.iter().filter(|r| r.2 == "GND").count();
    let gnd_pads = b.footprints.iter().flat_map(|f| f.nets.iter().zip(&f.fp.pads)).filter(|(n, _)| *n == "GND").count();
    // shield pads share the number S1 but are separate copper: one line each
    assert_eq!(gnd, gnd_pads - 1, "GND-Luftlinien");
    // a track between two pads removes their line
    let mut b2 = b.clone();
    let r1 = &b2.footprints[b2.find_fp("R1").unwrap()];
    let (a, e) = (r1.pad_pos(1), b2.footprints[b2.find_fp("R2").unwrap()].pad_pos(0));
    let net = r1.nets[1].clone();
    assert_eq!(net, "DIS");
    let before = b2.ratsnest().iter().filter(|r| r.2 == "DIS").count();
    b2.tracks.push(Track { a, b: e, width: 0.25, layer: F_CU, net: net.clone() });
    let after = b2.ratsnest().iter().filter(|r| r.2 == "DIS").count();
    assert_eq!(after, before - 1);
}

#[test]
fn drc_catches_clearance_and_shorts() {
    let mut b = demo::project().board;
    // clean placement: no copper problems, only unrouted connections
    let v = drc::run(&b, None);
    assert!(v.iter().all(|x| x.kind == drc::Kind::Unrouted || x.kind == drc::Kind::Courtyard), "{:#?}", v.iter().filter(|x| x.kind != drc::Kind::Unrouted).collect::<Vec<_>>());
    // a track passing 0.1 mm from a pad of another net
    let u1 = &b.footprints[b.find_fp("U1").unwrap()];
    let pad = u1.pad_hull(0);
    let (lo, hi) = pad.bbox();
    let y = lo.y - 0.1 - 0.125;
    b.tracks.push(Track { a: p(lo.x - 3.0, y), b: p(hi.x + 3.0, y), width: 0.25, layer: F_CU, net: "OUT".into() });
    let v = drc::run(&b, None);
    let c: Vec<_> = v.iter().filter(|x| x.kind == drc::Kind::Clearance).collect();
    assert!(!c.is_empty(), "Abstandsfehler erwartet");
    assert!(c[0].msg.contains("0,1"), "{}", c[0].msg);
    // straight through the pad: a short
    b.tracks.push(Track { a: p(lo.x - 3.0, (lo.y + hi.y) / 2.0), b: p(hi.x + 3.0, (lo.y + hi.y) / 2.0), width: 0.25, layer: B_CU, net: "OUT".into() });
    let v = drc::run(&b, None);
    assert!(v.iter().any(|x| x.kind == drc::Kind::Short), "Kurzschluss erwartet");
    // too thin, tiny via, copper at the edge
    b.tracks.push(Track { a: p(140.0, 102.0), b: p(141.0, 102.0), width: 0.1, layer: F_CU, net: "LED".into() });
    b.vias.push(Via { pos: p(100.1, 130.0), dia: 0.45, drill: 0.2, net: "LED".into() });
    let v = drc::run(&b, None);
    for k in [drc::Kind::TrackWidth, drc::Kind::Drill, drc::Kind::Annular, drc::Kind::Edge] {
        assert!(v.iter().any(|x| x.kind == k), "{:?} erwartet", k);
    }
}

#[test]
fn autoroute_and_zone_give_clean_drc() {
    let pr = demo::routed();
    let b = &pr.board;
    assert!(!b.tracks.is_empty());
    let v = drc::run(b, Some(&pr.schematic));
    let bad: Vec<_> = v.iter().filter(|x| x.kind.is_error()).collect();
    assert!(bad.is_empty(), "DRC: {:#?}", bad);
    let z = &b.zones[0];
    assert!(!z.fill.is_empty(), "GND-Fläche gefüllt");
    let area = freo::slicer::poly::area(&z.fill);
    assert!(area > 50.0 * 35.0 * 0.4, "Fläche {}", area);
}

#[test]
fn gerber_and_drill_for_a_known_board() {
    // one 0805 resistor, a track, a via on a 20 × 10 board
    let mut s = Schematic::default();
    s.add_symbol("Device:R", p(0.0, 0.0));
    s.annotate(false);
    s.labels.push(Label { pos: p(0.0, -3.81), text: "A".into(), kind: LabelKind::Local, rot: 0.0 });
    s.labels.push(Label { pos: p(0.0, 3.81), text: "B".into(), kind: LabelKind::Local, rot: 0.0 });
    let mut b = Board::default();
    b.rect_outline(0.0, 0.0, 20.0, 10.0);
    b.update_from(&s);
    b.footprints[0].pos = p(10.0, 5.0);
    b.tracks.push(Track { a: p(10.9125, 5.0), b: p(15.0, 5.0), width: 0.3, layer: F_CU, net: "B".into() });
    b.vias.push(Via { pos: p(15.0, 5.0), dia: 0.8, drill: 0.4, net: "B".into() });
    let top = gerber::layer(&b, gerber::Layer::Copper(F_CU), "TEST");
    assert!(top.contains("%FSLAX46Y46*%") && top.contains("%MOMM*%"));
    assert!(top.contains("%TF.FileFunction,Copper,L1,Top*%"));
    assert!(top.trim_end().ends_with("M02*"));
    let g = gerber::parse(&top);
    // the track: from (10.9125, -5) to (15, -5) with a 0.3 mm round aperture
    let d = g.draws.iter().find(|(_, a, e)| (a.x - 10.9125).abs() < 1e-6 && (e.x - 15.0).abs() < 1e-6).expect("Leiterbahn");
    assert_eq!(g.apertures[&d.0], "C,0.300000");
    assert!((d.1.y + 5.0).abs() < 1e-9, "y gespiegelt");
    // the via flash (0.8) and two round-rect pads as regions
    assert!(g.flashes.iter().any(|(ap, q)| g.apertures[ap] == "C,0.800000" && (q.x - 15.0).abs() < 1e-6 && (q.y + 5.0).abs() < 1e-6));
    assert_eq!(g.regions, 2);
    // nothing on the bottom except the via
    let bot = gerber::parse(&gerber::layer(&b, gerber::Layer::Copper(B_CU), "TEST"));
    assert_eq!(bot.flashes.len(), 1);
    assert!(bot.draws.is_empty() && bot.regions == 0);
    // outline: 4 lines
    let edge = gerber::parse(&gerber::layer(&b, gerber::Layer::Edge, "TEST"));
    assert_eq!(edge.draws.len(), 4);
    // paste only for the SMD pads, mask with margin
    let paste = gerber::parse(&gerber::layer(&b, gerber::Layer::Paste(true), "TEST"));
    assert_eq!(paste.regions, 2);
    let (pth, npth) = gerber::drill(&b);
    assert!(pth.starts_with("M48") && pth.contains("METRIC") && pth.contains("T1C0.400") && pth.contains("X15Y-5") && pth.trim_end().ends_with("M30"), "{}", pth);
    assert!(!npth.contains("T1C"));
    let files = gerber::fab_files(&b, "TEST");
    for want in ["TEST-F_Cu.gtl", "TEST-B_Cu.gbl", "TEST-F_Mask.gts", "TEST-Edge_Cuts.gm1", "TEST-PTH.drl", "TEST-NPTH.drl", "TEST-F_Silkscreen.gto"] {
        assert!(files.iter().any(|(n, _)| n == want), "{} fehlt", want);
    }
    // silkscreen carries the reference as outline text
    let silk = gerber::parse(&gerber::layer(&b, gerber::Layer::Silk(true), "TEST"));
    assert!(silk.draws.len() >= 8, "Referenz R1 als Strichschrift");
}

#[test]
fn gerber_of_demo_board_has_all_layers() {
    let pr = demo::routed();
    let files = gerber::fab_files(&pr.board, "LED");
    assert_eq!(files.len(), 12);
    let bot = files.iter().find(|(n, _)| n.ends_with(".gbl")).unwrap();
    assert!(bot.1.contains("G36*") && bot.1.contains("%LPC*%"), "GND-Fläche mit Aussparungen");
    let (pth, _) = gerber::drill(&pr.board);
    let holes = pth.lines().filter(|l| l.starts_with('X')).count();
    let expect = pr.board.vias.len() + pr.board.footprints.iter().flat_map(|f| &f.fp.pads).filter(|q| q.drill > 0.0 && !q.npth).count();
    assert_eq!(holes, expect);
}

#[test]
fn bom_groups_values() {
    let s = demo::schematic();
    let lines = bom::lines(&s);
    let r = lines.iter().find(|l| l.value == "5k1").unwrap();
    assert_eq!(r.refs, vec!["R4", "R5"]);
    assert!(lines.iter().all(|l| !l.refs.iter().any(|x| x.starts_with('#'))));
    let csv = bom::csv(&s);
    assert!(csv.starts_with("Pos;Referenz;Menge;Wert;Footprint"));
    assert!(csv.contains("\"R4, R5\";2;\"5k1\""), "{}", csv);
}

#[test]
fn kicad_schematic_round_trip() {
    let s = demo::schematic();
    let text = kicad::write_sch(&s, "LED_BLINKER");
    assert!(text.starts_with("(kicad_sch"));
    assert!(text.contains("(lib_id \"Timer:NE555P\")"));
    let back = kicad::read_sch(&text).unwrap();
    assert_eq!(back.symbols.len(), s.symbols.len());
    assert_eq!(back.wires.len(), s.wires.len());
    let (n1, n2) = (s.netlist(), back.netlist());
    for net in &n1.nets {
        for q in net.part_pins() {
            assert_eq!(n2.net_of(&q.reference, &q.pin), Some(net.name.as_str()), "{} {}", q.reference, q.pin);
        }
    }
    assert!(back.erc().iter().all(|e| e.severity != Severity::Error), "{:#?}", back.erc());
}

#[test]
fn kicad_board_round_trip() {
    let pr = demo::routed();
    let text = kicad::write_pcb(&pr.board, "LED_BLINKER");
    assert!(text.starts_with("(kicad_pcb"));
    let back = kicad::read_pcb(&text).unwrap();
    assert_eq!(back.footprints.len(), pr.board.footprints.len());
    assert_eq!(back.tracks.len(), pr.board.tracks.len());
    assert_eq!(back.vias.len(), pr.board.vias.len());
    assert_eq!(back.outline.len(), 1);
    assert_eq!(back.zones.len(), 1);
    for (a, b) in pr.board.footprints.iter().zip(&back.footprints) {
        assert_eq!(a.reference, b.reference);
        for k in 0..a.fp.pads.len() {
            assert!((a.pad_pos(k) - b.pad_pos(k)).length() < 1e-4, "{} Pad {}", a.reference, a.fp.pads[k].num);
            assert_eq!(a.nets[k], b.nets[k]);
        }
    }
    let mut back = back;
    back.fill_zones();
    assert!(back.ratsnest().is_empty(), "{:?}", back.ratsnest());
}

#[test]
fn kicad_import_of_handwritten_file() {
    // a minimal board as KiCad 8 writes it: rectangle outline, one footprint, arc track
    let src = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
      (general (thickness 1.6))
      (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
      (net 0 "") (net 1 "VCC")
      (footprint "Resistor_SMD:R_0603_1608Metric" (layer "F.Cu") (at 20 10 90)
        (property "Reference" "R7" (at 0 -1.43 90) (layer "F.SilkS"))
        (property "Value" "1k" (at 0 1.43 90) (layer "F.Fab"))
        (pad "1" smd roundrect (at -0.825 0 90) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25) (net 1 "VCC"))
        (pad "2" smd roundrect (at 0.825 0 90) (size 0.8 0.95) (layers "F.Cu" "F.Paste" "F.Mask") (roundrect_rratio 0.25)))
      (gr_rect (start 0 0) (end 40 20) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
      (arc (start 5 5) (mid 7 7) (end 9 5) (width 0.3) (layer "B.Cu") (net 1))
      (via (at 9 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1)))"#;
    let b = kicad::read_pcb(src).unwrap();
    assert_eq!(b.outline.len(), 1);
    assert_eq!(b.footprints[0].reference, "R7");
    assert_eq!(b.footprints[0].nets, vec!["VCC".to_string(), String::new()]);
    // rotated 90°: pad 1 at x=-0.825 moves to y=+0.825 (KiCad rotates counter-clockwise on screen)
    let q = b.footprints[0].pad_pos(0);
    assert!((q - p(20.0, 10.825)).length() < 1e-6, "{:?}", q);
    assert!(b.tracks.len() > 4 && b.tracks.iter().all(|t| t.layer == B_CU && t.net == "VCC"));
    assert_eq!(b.vias[0].net, "VCC");
    assert!(b.footprints[0].fp.pads[0].rot.abs() < 1e-9);
}

#[test]
fn board_as_freo_part_and_assembly() {
    let pr = demo::routed();
    let part = body3d::board_part(&pr.board, "LED_LP").unwrap();
    let r = freo::model::regen::run(&part);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let bb = r.topo.bbox;
    assert!((bb.max.x - bb.min.x - 50.0).abs() < 1e-6, "{:?}", bb);
    assert!((bb.max.y - bb.min.y - 1.6).abs() < 1e-6);
    assert!((bb.max.z - bb.min.z - 35.0).abs() < 1e-6);
    // the drills are holes in the laminate
    let mp = r.topo.polys.len();
    assert!(mp > 0);
    let asm = body3d::assembly(&pr.board, "LED").unwrap();
    // laminate, copper and one part per material of the components
    let names: Vec<&str> = asm.parts.iter().map(|p| p.data.name.as_str()).collect();
    assert!(names.contains(&"LED_KUNSTSTOFF") && names.contains(&"LED_METALL") && names.contains(&"LED_LED_ROT"), "{:?}", names);
    // the 10k resistor of the demo is SMD; colour bands appear once an axial part is used
    let parts = body3d::parts_import(&pr.board);
    assert!(parts.faces.len() > 20);
    // only through-hole leads go below the board top: they end 1.5 mm under the board
    let ymin = parts.faces.iter().flat_map(|f| f.v.iter().map(|v| v[1])).fold(f32::MAX, f32::min);
    assert!(ymin >= -1.5 - 1e-3 && ymin < 0.0, "{}", ymin);
    // every triangle faces outwards consistently with its normals
    for f in &parts.faces {
        for t in &f.t {
            let (a, b, c) = (f.v[t[0] as usize], f.v[t[1] as usize], f.v[t[2] as usize]);
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            let vn = [a[3] + b[3] + c[3], a[4] + b[4] + c[4], a[5] + b[5] + c[5]];
            assert!(n[0] * vn[0] + n[1] * vn[1] + n[2] * vn[2] >= -1e-6);
        }
    }
}

#[test]
fn generated_models_show_real_details() {
    use freo::pcb::model3d::{generate, Mat};
    // axial 4k7: yellow, violet, red, gold bands
    let fp = freo::pcb::footprints::find("R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal").unwrap();
    let m = generate(fp, "4k7", 1.6).unwrap();
    for b in [4u8, 7, 2, 10] {
        assert!(m.parts.iter().any(|x| x.0 == Mat::Band(b)), "Ring {}", b);
    }
    // SOIC-8: eight gull-wing leads (metal) and a plastic body
    let so = generate(freo::pcb::footprints::find("SOIC-8_3.9x4.9mm_P1.27mm").unwrap(), "", 1.6).unwrap();
    let metal = so.parts.iter().find(|x| x.0 == Mat::Metal).unwrap();
    // each lead: three bars of 12 triangles, plus the pin-1 dimple
    assert!(metal.2.len() >= 8 * 36, "{}", metal.2.len());
    // the leads reach the pads (outer end beyond the pad centre at x = ±2.475)
    let xmax = metal.1.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
    assert!(xmax > 2.6, "{}", xmax);
    // LED 5 mm: red dome above 6 mm, leads through the board
    let led = generate(freo::pcb::footprints::find("LED_D5.0mm").unwrap(), "", 1.6).unwrap();
    let red = led.parts.iter().find(|x| x.0 == Mat::LedRed).unwrap();
    assert!(red.1.iter().any(|v| v[2] > 8.0));
    let lead = led.parts.iter().find(|x| x.0 == Mat::Metal).unwrap();
    assert!(lead.1.iter().any(|v| v[2] < -1.6));
}

#[test]
fn ports_connectors_in_library() {
    let ports: Vec<&str> = freo::pcb::footprints::library().iter().filter_map(|f| f.port.as_deref()).collect();
    for want in ["USB-C / Thunderbolt", "Micro-USB", "USB-A", "RJ45 (Ethernet)", "DC-Buchse 5,5×2,1", "Klinke 3,5 mm"] {
        assert!(ports.contains(&want), "{} fehlt", want);
    }
}

// ---------------------------------------------------------------- units, buses, hierarchy

use freo::pcb::schematic::{Sheet, SheetPin, Wire};

#[test]
fn multi_unit_part_shares_reference_and_footprint() {
    let mut s = Schematic::default();
    let a = s.add_symbol("Amplifier_Operational:LM358", p(25.4, 25.4)).unwrap();
    s.annotate(false);
    let b = s.add_next_unit(a, p(50.8, 25.4)).unwrap();
    let c = s.add_next_unit(a, p(76.2, 25.4)).unwrap();
    assert!(s.add_next_unit(a, p(0.0, 0.0)).is_none(), "nur 3 Einheiten");
    assert_eq!(s.shown_ref(a), "U1A");
    assert_eq!(s.shown_ref(b), "U1B");
    assert_eq!(s.shown_ref(c), "U1C");
    // unit B has pins 5, 6, 7; unit C the supply
    let pins_b: Vec<String> = s.pin_positions(b).iter().map(|x| x.1.num.clone()).collect();
    assert_eq!(pins_b, vec!["5", "6", "7"]);
    let pins_c: Vec<String> = s.pin_positions(c).iter().map(|x| x.1.num.clone()).collect();
    assert_eq!(pins_c, vec!["8", "4"]);
    // no "duplicate reference" for units, one footprint on the board
    assert!(s.erc().iter().all(|e| !e.msg.contains("doppelt")), "{:#?}", s.erc());
    let mut brd = Board::default();
    brd.rect_outline(0.0, 0.0, 30.0, 20.0);
    brd.update_from(&s);
    assert_eq!(brd.footprints.len(), 1);
    assert_eq!(bom::lines(&s).len(), 1);
    // a missing supply unit is reported
    let mut s2 = s.clone();
    s2.symbols.remove(c);
    assert!(s2.erc().iter().any(|e| e.msg.contains("U1C") && e.msg.contains("nicht platziert")), "{:#?}", s2.erc());
    // annotation keeps the units of a new part together
    let mut s3 = Schematic::default();
    let x = s3.add_symbol("74xx:74HC00", p(0.0, 0.0)).unwrap();
    s3.symbols[x].unit = 1;
    let y = s3.add_symbol("74xx:74HC00", p(30.0, 0.0)).unwrap();
    s3.symbols[y].unit = 2;
    s3.annotate(false);
    assert_eq!(s3.symbols[x].reference, s3.symbols[y].reference);
}

#[test]
fn bus_members_connect_through_labels() {
    let mut s = Schematic::default();
    let a = s.add_symbol("Device:R", p(10.16, 10.16)).unwrap();
    let b = s.add_symbol("Device:R", p(40.64, 10.16)).unwrap();
    s.annotate(false);
    let _ = (a, b);
    // bus line between them, a bus label, entries and member labels D0 at both pins
    s.buses.push(Wire { a: p(25.4, 0.0), b: p(25.4, 30.48) });
    s.labels.push(Label { pos: p(25.4, 2.54), text: "D[0..7]".into(), kind: LabelKind::Local, rot: 0.0 });
    s.bus_entries.push(Wire { a: p(25.4, 6.35), b: p(22.86, 3.81) });
    s.add_wire(p(10.16, 6.35), p(22.86, 6.35));
    s.add_wire(p(22.86, 6.35), p(22.86, 3.81));
    s.labels.push(Label { pos: p(10.16, 6.35), text: "D0".into(), kind: LabelKind::Local, rot: 0.0 });
    s.labels.push(Label { pos: p(40.64, 6.35), text: "D0".into(), kind: LabelKind::Local, rot: 0.0 });
    let n = s.netlist();
    assert_eq!(n.net_of("R1", "1"), Some("D0"));
    assert_eq!(n.net_of("R2", "1"), Some("D0"));
    // the bus itself does not short anything
    assert_ne!(n.net_of("R1", "2"), n.net_of("R2", "2"));
    assert!(n.nets.iter().all(|x| !x.name.contains('[')));
    assert_eq!(freo::pcb::schematic::bus_members("A[1..3]").unwrap(), vec!["A1", "A2", "A3"]);
}

fn hier_project() -> Schematic {
    // root: R1 to a sheet pin "OUT"; sheet "Ausgang" holds R2 with a hierarchical label OUT
    let mut root = Schematic::default();
    root.add_symbol("Device:R", p(10.16, 10.16)).unwrap();
    let mut child = Schematic::default();
    child.add_symbol("Device:R", p(20.32, 20.32)).unwrap();
    child.labels.push(Label { pos: p(20.32, 16.51), text: "OUT".into(), kind: LabelKind::Hier, rot: 0.0 });
    child.labels.push(Label { pos: p(20.32, 24.13), text: "INNEN".into(), kind: LabelKind::Local, rot: 0.0 });
    root.sheets.push(Sheet { name: "Ausgang".into(), pos: p(50.8, 0.0), size: p(30.48, 20.32), pins: vec![SheetPin { name: "OUT".into(), pos: p(50.8, 6.35) }], sch: child });
    root.add_wire(p(10.16, 6.35), p(50.8, 6.35));
    root.annotate(false);
    root
}

#[test]
fn hierarchy_netlist_and_annotation() {
    let s = hier_project();
    let refs: Vec<String> = s.all_symbols().iter().map(|x| x.reference.clone()).collect();
    assert_eq!(refs, vec!["R1", "R2"], "fortlaufend über die Blätter");
    let n = s.netlist();
    assert_eq!(n.net_of("R1", "1"), n.net_of("R2", "1"), "über den Blattpin verbunden");
    assert_eq!(n.net_of("R2", "1"), Some("/Ausgang/OUT"));
    assert_eq!(n.net_of("R2", "2"), Some("/Ausgang/INNEN"));
    let mut b = Board::default();
    b.rect_outline(0.0, 0.0, 30.0, 20.0);
    b.update_from(&s);
    assert_eq!(b.footprints.len(), 2);
    // a sheet pin without label inside is an ERC error
    let mut bad = s.clone();
    bad.sheets[0].pins.push(SheetPin { name: "FEHLT".into(), pos: p(50.8, 12.7) });
    assert!(bad.erc().iter().any(|e| e.msg.contains("FEHLT")));
}

#[test]
fn kicad_hierarchy_round_trip() {
    let s = hier_project();
    let files = kicad::write_sch_files(&s, "HIER");
    assert_eq!(files.len(), 2);
    assert_eq!(files[1].0, "HIER-Ausgang.kicad_sch");
    assert!(files[0].1.contains("(sheet") && files[0].1.contains("\"Sheetfile\" \"HIER-Ausgang.kicad_sch\""));
    assert!(files[1].1.contains("(hierarchical_label \"OUT\""));
    let dir = std::env::temp_dir().join(format!("freo_hier_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (n, t) in &files {
        std::fs::write(dir.join(n), t).unwrap();
    }
    let back = kicad::read_sch_file(&dir.join("HIER.kicad_sch")).unwrap();
    assert_eq!(back.sheets.len(), 1);
    assert_eq!(back.sheets[0].sch.symbols.len(), 1);
    let (a, b) = (s.netlist(), back.netlist());
    assert_eq!(b.net_of("R1", "1"), a.net_of("R1", "1"));
    assert_eq!(b.net_of("R2", "2"), Some("/Ausgang/INNEN"));
    // multi-unit parts survive the round trip with their units
    let mut m = Schematic::default();
    let u = m.add_symbol("74xx:74HC00", p(25.4, 25.4)).unwrap();
    m.annotate(false);
    m.add_next_unit(u, p(50.8, 25.4)).unwrap();
    let t = kicad::write_sch(&m, "M");
    let back = kicad::read_sch(&t).unwrap();
    assert_eq!(back.symbols[1].unit, 2);
    let d = back.def(&back.symbols[1].lib).unwrap();
    assert_eq!(d.units, 5);
    assert_eq!(back.pin_positions(1).len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- KiCad libraries

#[test]
fn symbol_library_round_trip() {
    let lib: Vec<_> = freo::pcb::symbols::library().to_vec();
    let text = kicad::write_symbol_lib(&lib);
    assert!(text.starts_with("(kicad_symbol_lib"));
    let back = kicad::read_symbol_lib(&text, "Freo").unwrap();
    assert_eq!(back.len(), lib.len());
    for (a, b) in lib.iter().zip(&back) {
        assert_eq!(b.id, format!("Freo:{}", a.short()));
        assert_eq!(a.units, b.units, "{}", a.id);
        assert_eq!(a.pins.len(), b.pins.len(), "{}", a.id);
        for (x, y) in a.pins.iter().zip(&b.pins) {
            assert_eq!(x.num, y.num);
            assert!((x.pos - y.pos).length() < 1e-9, "{} Pin {}", a.id, x.num);
            assert_eq!(x.kind, y.kind);
            if a.units > 1 {
                assert_eq!(x.unit, y.unit, "{} Pin {}", a.id, x.num);
            }
        }
    }
}

#[test]
fn kicad_sym_with_extends_and_units() {
    // the way KiCad's own libraries are written (y up, derived symbols)
    let src = r#"(kicad_symbol_lib (version 20231120) (generator "kicad_symbol_editor")
      (symbol "OPA2" (pin_names (offset 0.127)) (in_bom yes) (on_board yes)
        (property "Reference" "U" (at 0 5.08 0)) (property "Value" "OPA2" (at 0 -5.08 0))
        (property "Footprint" "" (at 0 0 0))
        (symbol "OPA2_1_1" (polyline (pts (xy -5.08 5.08) (xy 5.08 0) (xy -5.08 -5.08) (xy -5.08 5.08)) (stroke (width 0.254)) (fill (type background)))
          (pin output line (at 7.62 0 180) (length 2.54) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
          (pin input line (at -7.62 2.54 0) (length 2.54) (name "+" (effects (font (size 1.27 1.27)))) (number "3" (effects (font (size 1.27 1.27))))))
        (symbol "OPA2_2_1" (pin output line (at 7.62 0 180) (length 2.54) (name "~" (effects (font (size 1.27 1.27)))) (number "7" (effects (font (size 1.27 1.27))))))
        (symbol "OPA2_2_2" (pin output line (at 7.62 0 180) (length 2.54) (name "~" (effects (font (size 1.27 1.27)))) (number "99" (effects (font (size 1.27 1.27)))))))
      (symbol "OPA2_ALT" (extends "OPA2") (property "Reference" "U" (at 0 0 0)) (property "Value" "OPA2_ALT" (at 0 0 0)) (property "Footprint" "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm" (at 0 0 0))))"#;
    let syms = kicad::read_symbol_lib(src, "Test").unwrap();
    assert_eq!(syms.len(), 2);
    let a = &syms[0];
    assert_eq!(a.id, "Test:OPA2");
    assert_eq!(a.units, 2);
    assert!(a.pins.iter().all(|q| q.num != "99"), "De-Morgan-Variante übersprungen");
    let p3 = a.pin("3").unwrap();
    assert!((p3.pos - p(-7.62, -2.54)).length() < 1e-9, "y gespiegelt");
    assert_eq!(a.pin("7").unwrap().unit, 2);
    let b = &syms[1];
    assert_eq!(b.id, "Test:OPA2_ALT");
    assert_eq!(b.pins.len(), a.pins.len());
    assert_eq!(b.footprint, "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm");
    freo::pcb::symbols::register(syms.clone());
    assert!(freo::pcb::symbols::find("Test:OPA2_ALT").is_some());
}

#[test]
fn footprint_library_round_trip() {
    for f in freo::pcb::footprints::library() {
        let text = kicad::write_footprint_file(f);
        let back = kicad::read_footprint_file(&text, f.id.split(':').next().unwrap()).unwrap();
        assert_eq!(back.id, f.id);
        assert_eq!(back.pads.len(), f.pads.len(), "{}", f.id);
        for (a, b) in f.pads.iter().zip(&back.pads) {
            assert_eq!(a.num, b.num);
            assert!((a.pos - b.pos).length() < 1e-9 && (a.size - b.size).length() < 1e-9, "{} Pad {}", f.id, a.num);
            assert!((a.drill - b.drill).abs() < 1e-9 && a.npth == b.npth);
        }
    }
    // a .pretty folder
    let dir = std::env::temp_dir().join(format!("freo_lib_{}/Mine.pretty", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut fp = freo::pcb::footprints::find("R_0805_2012Metric").unwrap().clone();
    fp.id = "R_Custom".into();
    std::fs::write(dir.join("R_Custom.kicad_mod"), kicad::write_footprint_file(&fp)).unwrap();
    let got = kicad::read_pretty_dir(&dir).unwrap();
    assert_eq!(got[0].id, "Mine:R_Custom");
    freo::pcb::footprints::register(got);
    assert_eq!(freo::pcb::footprints::find("Mine:R_Custom").unwrap().pads.len(), 2);
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

// ---------------------------------------------------------------- editors

#[test]
fn footprint_generators() {
    use freo::pcb::footprints::{generate, Gen};
    let soic = generate("X:SO8", "", &Gen::Dual { n: 8, pitch: 1.27, span: 4.95, pw: 1.95, ph: 0.6 }, 1.5);
    let lib = freo::pcb::footprints::find("SOIC-8_3.9x4.9mm_P1.27mm").unwrap();
    for (a, b) in soic.pads.iter().zip(&lib.pads) {
        assert_eq!(a.num, b.num);
        assert!((a.pos - b.pos).length() < 1e-9, "Pad {}", a.num);
    }
    let qfp = generate("X:QFP32", "", &Gen::Quad { per_side: 8, pitch: 0.8, span: 8.4, pw: 1.5, ph: 0.55, ep: 3.0 }, 1.2);
    assert_eq!(qfp.pads.len(), 33);
    // counter-clockwise: pin 9 starts the bottom row, pin 17 the right side
    assert!(qfp.pads[8].pos.y > 4.0 && qfp.pads[16].pos.x > 4.0);
    // no two pads of different numbers too close for a 0.2 mm clearance
    for i in 0..qfp.pads.len() {
        for j in i + 1..qfp.pads.len() {
            let g = qfp.pads[i].hull(freo::pcb::geom::P::ZERO, 0.0, false).gap(&qfp.pads[j].hull(freo::pcb::geom::P::ZERO, 0.0, false));
            assert!(g > 0.2, "{} {} {}", qfp.pads[i].num, qfp.pads[j].num, g);
        }
    }
    let hdr = generate("X:H", "", &Gen::Tht { rows: 2, cols: 5, pitch: 2.54, span: 2.54, drill: 1.0, pad: 1.7 }, 8.5);
    assert_eq!(hdr.pads.len(), 10);
    assert!((hdr.pads[9].pos - p(2.54, 0.0)).length() < 1e-9, "Pin 10 gegenüber Pin 1");
    assert!(!hdr.smd());
}

#[test]
fn box_symbol_from_pin_table() {
    use freo::pcb::symbols::{box_symbol, PinRow, PinType, Side};
    let rows: Vec<PinRow> = (1..=10).map(|i| PinRow { num: i.to_string(), name: format!("IO{}", i), kind: PinType::Bidi, side: if i <= 5 { Side::Left } else { Side::Right } }).collect();
    let s = box_symbol("Projekt:X", "U", "X", "Package_SO:SOIC-16_3.9x9.9mm_P1.27mm", "", &rows);
    assert_eq!(s.pins.len(), 10);
    for q in &s.pins {
        for c in [q.pos.x, q.pos.y] {
            assert!(((c / 2.54) - (c / 2.54).round()).abs() < 1e-6, "Pin {} nicht im 2,54-Raster", q.num);
        }
    }
    assert!(s.pin("1").unwrap().pos.x < 0.0 && s.pin("6").unwrap().pos.x > 0.0);
    // usable in a schematic and on a board
    let mut sch = Schematic::default();
    sch.local_lib.push(s);
    let i = sch.add_symbol("Projekt:X", p(50.8, 50.8)).unwrap();
    sch.annotate(false);
    assert_eq!(sch.pin_positions(i).len(), 10);
}

// ---------------------------------------------------------------- net classes and routing

use freo::pcb::board::{diff_partner, pattern_match, NetClass};

#[test]
fn net_classes_drive_widths_and_clearances() {
    let mut b = demo::project().board;
    assert!(pattern_match("+*", "+5V") && pattern_match("GND*", "GND") && !pattern_match("GND*", "AGND"));
    assert_eq!(b.rules.class_of("+5V").name, "Versorgung");
    assert_eq!(b.rules.class_of("TRIG").name, "Default");
    assert!((b.rules.class_of("GND").track - 0.5).abs() < 1e-9);
    // a strict class for TRIG: 0.6 mm clearance makes a 0.4 mm gap an error
    b.rules.classes.push(NetClass { name: "HV".into(), patterns: vec!["TRIG".into()], clearance: 0.6, track: 0.3, via_dia: 0.8, via_drill: 0.4, diff_width: 0.3, diff_gap: 0.6 });
    let u1 = &b.footprints[b.find_fp("U1").unwrap()];
    let k = u1.fp.pads.iter().position(|q| q.num == "1").unwrap();
    let (lo, hi) = u1.pad_hull(k).bbox();
    let y = lo.y - 0.4 - 0.15;
    b.tracks.push(Track { a: p(lo.x - 2.0, y), b: p(hi.x + 2.0, y), width: 0.3, layer: F_CU, net: "TRIG".into() });
    let v = drc::run(&b, None);
    assert!(v.iter().any(|x| x.kind == drc::Kind::Clearance && x.msg.contains("0,6")), "{:#?}", v.iter().filter(|x| x.kind != drc::Kind::Unrouted).collect::<Vec<_>>());
    assert_eq!(diff_partner("USB_D+").as_deref(), Some("USB_D-"));
    assert_eq!(diff_partner("CLK_P").as_deref(), Some("CLK_N"));
    assert_eq!(diff_partner("GND"), None);
}

#[test]
fn four_layer_autoroute_is_drc_clean() {
    let mut pr = demo::project();
    pr.board.layers = 4;
    // inner layers are used when the outer ones are blocked
    let rep = autoroute::route(&mut pr.board, None);
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    let v = drc::run(&pr.board, Some(&pr.schematic));
    let bad: Vec<_> = v.iter().filter(|x| x.kind.is_error()).collect();
    assert!(bad.is_empty(), "{:#?}", bad);
    // inner-layer copper ends up in the Gerber set
    let files = gerber::fab_files(&pr.board, "L4");
    assert!(files.iter().any(|(n, _)| n.ends_with(".g2")) && files.iter().any(|(n, _)| n.ends_with(".g3")));
    // block both outer layers between two pads with a wall of other-net copper → inner layer
    let mut b = Board::default();
    b.rect_outline(0.0, 0.0, 30.0, 20.0);
    b.layers = 4;
    let mut s = Schematic::default();
    s.add_symbol("Connector_Generic:Conn_01x02", p(0.0, 0.0));
    s.add_symbol("Connector_Generic:Conn_01x02", p(20.0, 0.0));
    s.annotate(false);
    s.labels.push(Label { pos: p(-5.08, 0.0), text: "SIG".into(), kind: LabelKind::Local, rot: 0.0 });
    s.labels.push(Label { pos: p(14.92, 0.0), text: "SIG".into(), kind: LabelKind::Local, rot: 0.0 });
    b.update_from(&s);
    b.footprints[0].pos = p(5.0, 8.0);
    b.footprints[1].pos = p(25.0, 8.0);
    for l in [F_CU, B_CU] {
        b.tracks.push(Track { a: p(15.0, 0.4), b: p(15.0, 19.6), width: 1.0, layer: l, net: "WAND".into() });
    }
    let rep = autoroute::route(&mut b, Some("SIG"));
    assert!(rep.failed.is_empty(), "{:?}", rep);
    assert!(b.tracks.iter().any(|t| t.net == "SIG" && (t.layer == 2 || t.layer == 3)));
}

#[test]
fn walkaround_finds_a_way_and_shove_moves_tracks() {
    let mut b = Board::default();
    b.rect_outline(0.0, 0.0, 40.0, 30.0);
    // an obstacle track of net X across the direct path
    b.tracks.push(Track { a: p(20.0, 5.0), b: p(20.0, 25.0), width: 0.25, layer: F_CU, net: "X".into() });
    let path = autoroute::find_path(&b, "Y", F_CU, p(5.0, 15.0), p(35.0, 15.0), 0.25).expect("Umweg");
    assert!(path.len() > 2);
    for w in path.windows(2) {
        let h = freo::pcb::geom::Hull::capsule(w[0], w[1], 0.125);
        assert!(h.gap(&freo::pcb::geom::Hull::capsule(p(20.0, 5.0), p(20.0, 25.0), 0.125)) >= 0.2 - 1e-6);
    }
    // shove: a new track 0.1 mm next to a free-floating X track pushes it aside
    let mut b2 = Board::default();
    b2.rect_outline(0.0, 0.0, 40.0, 30.0);
    b2.tracks.push(Track { a: p(10.0, 10.0), b: p(30.0, 10.0), width: 0.25, layer: F_CU, net: "X".into() });
    b2.tracks.push(Track { a: p(30.0, 10.0), b: p(30.0, 20.0), width: 0.25, layer: F_CU, net: "X".into() });
    let new = vec![Track { a: p(12.0, 10.35), b: p(28.0, 10.35), width: 0.25, layer: F_CU, net: "Y".into() }];
    let n = freo::pcb::route::shove(&mut b2, &new).unwrap();
    assert_eq!(n, 1);
    let t = &b2.tracks[0];
    assert!(t.a.y < 10.0, "nach unten geschoben: {:?}", t);
    assert_eq!(b2.tracks[1].a, t.b, "Nachbar bleibt verbunden");
    let h = freo::pcb::geom::Hull::capsule(new[0].a, new[0].b, 0.125);
    assert!(h.gap(&freo::pcb::geom::Hull::capsule(t.a, t.b, 0.125)) >= 0.2 - 1e-6);
}

#[test]
fn length_tuning_and_differential_pairs() {
    // a straight 30 mm track tuned to 40 mm
    let mut b = Board::default();
    b.rect_outline(0.0, 0.0, 50.0, 30.0);
    b.tracks.push(Track { a: p(10.0, 15.0), b: p(40.0, 15.0), width: 0.2, layer: F_CU, net: "CLK".into() });
    let len = freo::pcb::route::tune_length(&mut b, "CLK", 40.0).unwrap();
    assert!((len - 40.0).abs() < 0.2, "{}", len);
    assert!(drc::run(&b, None).iter().all(|x| !matches!(x.kind, drc::Kind::Clearance | drc::Kind::Short)), "Mäander kollidiert nicht mit sich");
    // differential pair along an L-shaped centre line keeps its gap
    let center = vec![p(5.0, 5.0), p(20.0, 5.0), p(30.0, 15.0)];
    let (a, c) = freo::pcb::route::diff_pair(&center, 0.3, 0.2);
    for (sa, sc) in a.windows(2).zip(c.windows(2)) {
        let g = freo::pcb::geom::Hull::capsule(sa[0], sa[1], 0.15).gap(&freo::pcb::geom::Hull::capsule(sc[0], sc[1], 0.15));
        assert!((g - 0.2).abs() < 0.03, "Spalt {}", g);
    }
    // skew check in the DRC
    let mut b = Board::default();
    b.rect_outline(0.0, 0.0, 50.0, 30.0);
    b.tracks.push(Track { a: p(5.0, 5.0), b: p(25.0, 5.0), width: 0.3, layer: F_CU, net: "USB_D+".into() });
    b.tracks.push(Track { a: p(5.0, 10.0), b: p(30.0, 10.0), width: 0.3, layer: F_CU, net: "USB_D-".into() });
    let mut s = Schematic::default();
    s.add_symbol("Connector:USB_B_Micro", p(25.4, 25.4));
    s.annotate(false);
    for (pin, net) in [("2", "USB_D-"), ("3", "USB_D+")] {
        let q = s.pin_positions(0).into_iter().find(|x| x.1.num == pin).unwrap().0;
        s.labels.push(Label { pos: q, text: net.into(), kind: LabelKind::Local, rot: 0.0 });
    }
    b.update_from(&s);
    let v = drc::run(&b, None);
    assert!(v.iter().any(|x| x.kind == drc::Kind::Skew), "{:#?}", v);
}

// ---------------------------------------------------------------- fabrication data

/// Tiny XML check: every opened tag is closed in order.
fn well_formed(xml: &str) -> Result<usize, String> {
    let mut stack: Vec<String> = vec![];
    let mut n = 0;
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        let j = rest[i..].find('>').ok_or("offenes <")? + i;
        let tag = &rest[i + 1..j];
        rest = &rest[j + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        n += 1;
        if let Some(name) = tag.strip_prefix('/') {
            let top = stack.pop().ok_or(format!("zu viele </{}>", name))?;
            if top != name {
                return Err(format!("</{}> schließt <{}>", name, top));
            }
        } else if !tag.ends_with('/') {
            stack.push(tag.split_whitespace().next().unwrap_or("").to_string());
        }
    }
    if stack.is_empty() {
        Ok(n)
    } else {
        Err(format!("nicht geschlossen: {:?}", stack))
    }
}

#[test]
fn pick_and_place_job_file_and_ipc2581() {
    let pr = demo::routed();
    let b = &pr.board;
    let pl = fab::placements(b);
    assert_eq!(pl.len(), b.footprints.len());
    // relative to the lower-left board corner, y up
    let r1 = pl.iter().find(|x| x.reference == "R1").unwrap();
    let f = &b.footprints[b.find_fp("R1").unwrap()];
    assert!((r1.pos.x - (f.pos.x - 100.0)).abs() < 1e-6 && (r1.pos.y - (135.0 - f.pos.y)).abs() < 1e-6, "{:?}", r1.pos);
    let top = fab::pos_file(b, Some(true));
    assert!(top.contains("R1 ") && top.contains("## End"));
    let csv = fab::pos_csv(b);
    assert!(csv.starts_with("Designator,Val,Package,Mid X,Mid Y,Rotation,Layer"));
    assert_eq!(csv.lines().count(), b.footprints.len() + 1);
    let job: serde_json::Value = serde_json::from_str(&fab::job_file(b, "LED")).unwrap();
    assert_eq!(job["GeneralSpecs"]["LayerNumber"], 2);
    assert!(job["FilesAttributes"].as_array().unwrap().iter().any(|x| x["Path"] == "LED-F_Cu.gtl"));
    let xml = fab::ipc2581(b, "LED");
    let tags = well_formed(&xml).unwrap();
    assert!(tags > 200);
    assert!(xml.contains("<IPC-2581 revision=\"B\"") && xml.contains("<Component refDes=\"U1\"") && xml.contains("<LogicalNet name=\"GND\">"));
    assert_eq!(xml.matches("<Hole ").count(), pr.board.vias.len() + b.footprints.iter().flat_map(|f| &f.fp.pads).filter(|q| q.drill > 0.0 && q.slot <= q.drill).count());
}

#[test]
fn slotted_holes_in_drill_file_and_kicad() {
    let mut s = Schematic::default();
    s.add_symbol("Connector:USB_B_Micro", p(25.4, 25.4));
    s.annotate(false);
    let mut b = Board::default();
    b.rect_outline(0.0, 0.0, 20.0, 15.0);
    b.update_from(&s);
    b.footprints[0].pos = p(10.0, 7.5);
    let pad = b.footprints[0].fp.pads.iter().find(|q| q.num == "6").unwrap().clone();
    assert!(pad.slot > pad.drill);
    let (pth, _) = gerber::drill(&b);
    assert_eq!(pth.matches("G85").count(), 2, "{}", pth);
    // slot along the pad's long (y) side: 1.3 − 0.6 = 0.7 mm between the ends
    let line = pth.lines().find(|l| l.contains("G85")).unwrap();
    assert!(line.contains("Y-7.15G85") && line.contains("Y-7.85"), "{}", line);
    let back = kicad::read_pcb(&kicad::write_pcb(&b, "S")).unwrap();
    let bp = back.footprints[0].fp.pads.iter().find(|q| q.num == "6").unwrap();
    assert!((bp.slot - 1.3).abs() < 1e-9 && (bp.drill - 0.6).abs() < 1e-9);
    // the laminate gets slot-shaped holes and still regenerates
    let part = body3d::board_part(&b, "S").unwrap();
    let r = freo::model::regen::run(&part);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}

#[test]
fn stroke_font_covers_german_text() {
    for t in ["Freo LED-Blinker", "ÄÖÜß µΩ 10k±", "R1 C2 U3A"] {
        let s = freo::pcb::stroke::centered(t, p(0.0, 0.0), 1.0, 0.0, false);
        assert!(!s.is_empty());
    }
}

#[test]
fn step_models_replace_the_simple_bodies() {
    // a 4 × 2 × 1 mm block as STEP (Freo's own writer), z up like KiCad models
    use freo::model::features::*;
    let mut m = ModelData::new_part("BLOCK");
    let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
    e.plane = Some(PlaneRef::Datum(FRONT_ID));
    e.sketch = freo::sketch::sketch::Sketch::rect(4.0, 2.0, 0.0, 0.0);
    e.depth = Dim::v(1.0);
    m.features.push(Feature { id: 10, name: "P".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(e) });
    m.next_id = 20;
    let r = freo::model::regen::run(&m);
    let dir = std::env::temp_dir().join(format!("freo_step_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("block.step");
    std::fs::write(&path, freo::app::export::step_text(&r, "BLOCK", "block.step")).unwrap();
    // a .wrl name resolves to the .step next to it
    assert_eq!(freo::pcb::models::resolve(&dir.join("block.wrl").to_string_lossy()).as_deref(), Some(path.as_path()));
    let mut b = Board::default();
    b.rect_outline(0.0, 0.0, 20.0, 10.0);
    let mut s = Schematic::default();
    s.add_symbol("Device:R", p(0.0, 0.0));
    s.annotate(false);
    b.update_from(&s);
    b.footprints[0].pos = p(10.0, 5.0);
    b.footprints[0].fp.model = Some(dir.join("block.wrl").to_string_lossy().to_string());
    let parts = body3d::parts_import(&b);
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for f in &parts.faces {
        for v in &f.v {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
    }
    // centred on the part, 4 × 2 footprint, 1 mm above the board top (1,6 mm)
    assert!(((lo[0] + hi[0]) / 2.0 - 10.0).abs() < 0.01 && ((lo[2] + hi[2]) / 2.0 - 5.0).abs() < 0.01, "{:?} {:?}", lo, hi);
    assert!((hi[0] - lo[0] - 4.0).abs() < 0.01 && (hi[2] - lo[2] - 2.0).abs() < 0.01);
    assert!((lo[1] - 1.6).abs() < 0.01 && (hi[1] - 2.6).abs() < 0.01, "{:?} {:?}", lo, hi);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn shove_jogs_around_tracks_fixed_on_pads() {
    // a track from a via to a via; a new track crosses close to its middle
    let mut b = Board::default();
    b.rect_outline(0.0, 0.0, 40.0, 30.0);
    b.vias.push(Via { pos: p(5.0, 10.0), dia: 0.8, drill: 0.4, net: "X".into() });
    b.vias.push(Via { pos: p(35.0, 10.0), dia: 0.8, drill: 0.4, net: "X".into() });
    b.tracks.push(Track { a: p(5.0, 10.0), b: p(35.0, 10.0), width: 0.25, layer: F_CU, net: "X".into() });
    let new = vec![Track { a: p(12.0, 10.4), b: p(28.0, 10.4), width: 0.25, layer: F_CU, net: "Y".into() }];
    freo::pcb::route::shove(&mut b, &new).unwrap();
    let x: Vec<_> = b.tracks.iter().filter(|t| t.net == "X").collect();
    assert_eq!(x.len(), 3, "zwei Knicke: {:#?}", x);
    // still connected via to via
    let mut ends: Vec<P> = x.iter().flat_map(|t| [t.a, t.b]).collect();
    ends.sort_by(|a, c| a.x.partial_cmp(&c.x).unwrap());
    assert!((ends[0] - p(5.0, 10.0)).length() < 1e-9 && (ends[ends.len() - 1] - p(35.0, 10.0)).length() < 1e-9);
    let mut b2 = b.clone();
    b2.tracks.extend(new);
    let v = drc::run(&b2, None);
    assert!(v.iter().all(|x| !matches!(x.kind, drc::Kind::Clearance | drc::Kind::Short)), "{:#?}", v);
}

#[test]
fn laminate_with_many_holes_has_no_stray_edges() {
    // ~200 drills in rows: every model edge on the top face must lie on a hole or the outline
    let b = demo::showcase();
    let part = body3d::board_part(&b, "X").unwrap();
    let r = freo::model::regen::run(&part);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let t = &r.topo;
    let drills: Vec<(freo::pcb::geom::P, f64)> = b.footprints.iter().flat_map(|f| f.fp.pads.iter().filter(|q| q.drill > 0.0).map(move |q| (q.hole_ends(f.pos, f.rot, f.bottom).0, q.slot.max(q.drill)))).collect();
    let mut stray = 0;
    for &(u, v) in &t.draw_edges {
        let (a, c) = (t.positions[u], t.positions[v]);
        if (a.y - b.thickness).abs() > 1e-6 || (c.y - b.thickness).abs() > 1e-6 {
            continue;
        }
        let near_hole = |q: glam::DVec3| drills.iter().any(|(h, d)| (p(q.x, q.z) - *h).length() <= d / 2.0 + 0.8);
        let on_outline = |q: glam::DVec3| q.x.abs() < 1e-6 || q.z.abs() < 1e-6 || (q.x - 160.0).abs() < 1e-6 || q.z > 0.0 && b.size().is_some_and(|(_, hi)| (q.z - hi.y).abs() < 1e-6);
        if !(near_hole(a) && near_hole(c) && (a - c).length() < 2.0) && !(on_outline(a) && on_outline(c)) {
            stray += 1;
        }
    }
    assert_eq!(stray, 0, "Kanten zwischen den Bohrungen");
}
