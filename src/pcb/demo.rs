//! Sample project: an NE555 LED blinker powered from USB-C (with the CC pull-downs), used by
//! the demo scenes and the tests.

use super::board::{Board, Zone, B_CU};
use super::geom::{p, P};
use super::schematic::{Label, LabelKind, Schematic, Wire};
use super::Project;

const U: f64 = 2.54;

fn at(x: f64, y: f64) -> P {
    p(x * U, y * U)
}

/// Pin `pin` of the symbol with reference `r` gets a stub wire and a label or power symbol.
fn tie(s: &mut Schematic, r: &str, pin: &str, net: &str) {
    let i = s.symbols.iter().position(|x| x.reference == r).expect("Bauteil");
    let inst = s.symbols[i].clone();
    let d = s.def(&inst.lib).expect("Symbol").clone();
    let pn = d.pin(pin).expect("Pin");
    let pos = Schematic::xform(&inst, pn.pos);
    // outwards = opposite of the pin direction (which points into the body)
    let dir = Schematic::xform_dir(&inst, pn.dir) + 180.0;
    let a = dir.to_radians();
    let out = p(a.cos().round(), -a.sin().round());
    let end = pos + out * U;
    s.add_wire(pos, end);
    match net {
        "GND" | "+5V" => {
            let k = s.add_symbol(&format!("power:{}", net), end).unwrap();
            // ground hangs below, supply sits above the stub end
            if net == "GND" && out.y < -0.5 {
                let e2 = end + p(0.0, 0.0);
                s.symbols[k].pos = e2;
                s.symbols[k].rot = 180.0;
            }
            if net == "+5V" && out.y > 0.5 {
                s.symbols[k].rot = 180.0;
            }
        }
        _ => s.labels.push(Label { pos: end, text: net.into(), kind: LabelKind::Local, rot: 0.0 }),
    }
}

pub fn schematic() -> Schematic {
    let mut s = Schematic { title: "LED-Blinker mit NE555 und USB-C".into(), ..Default::default() };
    let place = |s: &mut Schematic, lib: &str, r: &str, v: &str, pos: P, rot: f64| {
        let i = s.add_symbol(lib, pos).unwrap();
        s.symbols[i].reference = r.into();
        s.symbols[i].value = v.into();
        s.symbols[i].rot = rot;
    };
    place(&mut s, "Connector:USB_C_Receptacle_PowerOnly_6P", "J1", "USB_C_Power", at(14.0, 30.0), 0.0);
    place(&mut s, "Device:R", "R4", "5k1", at(28.0, 40.0), 0.0);
    place(&mut s, "Device:R", "R5", "5k1", at(32.0, 40.0), 0.0);
    place(&mut s, "Timer:NE555P", "U1", "NE555", at(62.0, 30.0), 0.0);
    place(&mut s, "Device:R", "R1", "10k", at(48.0, 14.0), 0.0);
    place(&mut s, "Device:R", "R2", "47k", at(48.0, 24.0), 0.0);
    place(&mut s, "Device:C_Polarized", "C1", "10u", at(46.0, 42.0), 0.0);
    place(&mut s, "Device:C", "C2", "10n", at(52.0, 42.0), 0.0);
    place(&mut s, "Device:C", "C3", "100n", at(76.0, 16.0), 0.0);
    place(&mut s, "Device:R", "R3", "330", at(84.0, 28.0), 90.0);
    place(&mut s, "Device:LED", "D1", "rot", at(92.0, 34.0), 90.0);
    for (r, pin, net) in [
        ("J1", "A9", "+5V"),
        ("J1", "A12", "GND"),
        ("J1", "A5", "CC1"),
        ("J1", "B5", "CC2"),
        ("J1", "S1", "GND"),
        ("R4", "1", "CC1"),
        ("R4", "2", "GND"),
        ("R5", "1", "CC2"),
        ("R5", "2", "GND"),
        ("U1", "8", "+5V"),
        ("U1", "1", "GND"),
        ("U1", "4", "+5V"),
        ("U1", "2", "TRIG"),
        ("U1", "6", "TRIG"),
        ("U1", "7", "DIS"),
        ("U1", "5", "CV"),
        ("U1", "3", "OUT"),
        ("R1", "1", "+5V"),
        ("R1", "2", "DIS"),
        ("R2", "1", "DIS"),
        ("R2", "2", "TRIG"),
        ("C1", "1", "TRIG"),
        ("C1", "2", "GND"),
        ("C2", "1", "CV"),
        ("C2", "2", "GND"),
        ("C3", "1", "+5V"),
        ("C3", "2", "GND"),
        ("R3", "2", "OUT"),
        ("R3", "1", "LED"),
        ("D1", "2", "LED"),
        ("D1", "1", "GND"),
    ] {
        tie(&mut s, r, pin, net);
    }
    // the USB connector's pins are passive: flag the supply nets as driven (ERC)
    for (net, x, y) in [("+5V", 8.0, 20.0), ("GND", 8.0, 44.0)] {
        s.add_symbol(&format!("power:{}", net), at(x, y));
        s.add_symbol("power:PWR_FLAG", at(x + 3.0, y));
        s.add_wire(at(x, y), at(x + 3.0, y));
    }
    s.annotate(false);
    s
}

/// Board 50 × 35 mm with the parts placed (not routed).
pub fn board(s: &Schematic) -> Board {
    let mut b = Board::default();
    b.rect_outline(100.0, 100.0, 50.0, 35.0);
    b.update_from(s);
    let put = |b: &mut Board, r: &str, x: f64, y: f64, rot: f64| {
        let i = b.find_fp(r).expect("Footprint");
        b.footprints[i].pos = p(x, y);
        b.footprints[i].rot = rot;
    };
    put(&mut b, "J1", 103.65, 117.5, 90.0);
    put(&mut b, "R4", 112.0, 110.0, 90.0);
    put(&mut b, "R5", 112.0, 125.0, 90.0);
    put(&mut b, "U1", 128.0, 117.5, 0.0);
    put(&mut b, "R1", 122.0, 107.0, 0.0);
    put(&mut b, "R2", 128.0, 107.0, 0.0);
    put(&mut b, "C1", 120.0, 128.0, 0.0);
    put(&mut b, "C2", 130.0, 128.0, 0.0);
    put(&mut b, "C3", 135.0, 110.0, 90.0);
    put(&mut b, "R3", 138.0, 121.0, 90.0);
    put(&mut b, "D1", 143.5, 114.0, 270.0);
    b.texts.push((p(128.0, 132.5), "Freo LED-Blinker".into(), 1.2));
    b
}

pub fn gnd_zone(b: &mut Board) {
    let (lo, hi) = b.size().unwrap();
    b.zones.push(Zone { net: "GND".into(), layer: B_CU, outline: vec![lo, p(hi.x, lo.y), hi, p(lo.x, hi.y)], clearance: 0.3, min_width: 0.25, thermal: true, fill: vec![], fixed: false });
}

pub fn project() -> Project {
    let s = schematic();
    let b = board(&s);
    Project { name: "LED_BLINKER".into(), schematic: s, board: b, footprints: vec![] }
}

/// The sample, routed with the autorouter and a GND pour on the bottom.
pub fn routed() -> Project {
    let mut pr = project();
    super::autoroute::route(&mut pr.board, None);
    gnd_zone(&mut pr.board);
    pr.board.fill_zones();
    pr
}

/// Adds a sub-sheet "Verstaerker" with a dual op-amp (units A, B and supply C) fed from
/// the blinker's output, and a small data bus, to show hierarchy, units and buses.
pub fn add_extension(s: &mut Schematic) {
    use super::schematic::{Sheet, SheetPin};
    let mut c = Schematic::default();
    let a = c.add_symbol("Amplifier_Operational:LM358", at(30.0, 30.0)).unwrap();
    c.symbols[a].reference = "U2".into();
    let b = c.add_next_unit(a, at(60.0, 30.0)).unwrap();
    let p3 = c.add_next_unit(a, at(45.0, 50.0)).unwrap();
    let _ = (b, p3);
    for (r, pin, net) in [("U2", "3", "IN"), ("U2", "2", "FB"), ("U2", "1", "FB")] {
        tie(&mut c, r, pin, net);
    }
    // unit B as a buffer of unit A
    let pins: Vec<(P, String)> = c.pin_positions(b).into_iter().map(|(q, x)| (q, x.num.clone())).collect();
    for (q, num) in pins {
        let net = match num.as_str() {
            "5" => "FB",
            "6" | "7" => "AUS",
            _ => continue,
        };
        let out = if q.x < c.symbols[b].pos.x { p(-U, 0.0) } else { p(U, 0.0) };
        c.add_wire(q, q + out);
        c.labels.push(Label { pos: q + out, text: net.into(), kind: if net == "AUS" { LabelKind::Hier } else { LabelKind::Local }, rot: 0.0 });
    }
    let pc: Vec<(P, String)> = c.pin_positions(p3).into_iter().map(|(q, x)| (q, x.num.clone())).collect();
    for (q, num) in pc {
        let net = if num == "8" { "+5V" } else { "GND" };
        let k = c.add_symbol(&format!("power:{}", net), q).unwrap();
        if net == "+5V" {
            c.symbols[k].rot = 0.0;
        }
    }
    c.labels.push(Label { pos: c.pin_positions(a).into_iter().find(|x| x.1.num == "3").map(|x| x.0 - p(U, 0.0)).unwrap(), text: "IN".into(), kind: LabelKind::Hier, rot: 0.0 });
    // a bus D[0..3] with entries and member labels (graphic example)
    c.buses.push(Wire { a: at(80.0, 20.0), b: at(80.0, 40.0) });
    c.labels.push(Label { pos: at(80.0, 19.0), text: "D[0..3]".into(), kind: LabelKind::Local, rot: 0.0 });
    for k in 0..4 {
        let y = 24.0 + k as f64 * 4.0;
        c.bus_entries.push(Wire { a: at(80.0, y), b: at(81.0, y - 1.0) });
        c.add_wire(at(81.0, y - 1.0), at(86.0, y - 1.0));
        c.labels.push(Label { pos: at(86.0, y - 1.0), text: format!("D{}", k), kind: LabelKind::Local, rot: 0.0 });
        let r = c.add_symbol("Device:R", at(88.0, y - 1.0 + 1.5)).unwrap();
        c.symbols[r].rot = 90.0;
        let _ = r;
    }
    s.sheets.push(Sheet { name: "Verstaerker".into(), pos: at(100.0, 50.0), size: p(30.48, 20.32), pins: vec![SheetPin { name: "IN".into(), pos: at(100.0, 53.0) }, SheetPin { name: "AUS".into(), pos: at(100.0, 57.0) }], sch: c });
    s.add_wire(at(97.0, 53.0), at(100.0, 53.0));
    s.labels.push(Label { pos: at(97.0, 53.0), text: "OUT".into(), kind: LabelKind::Local, rot: 0.0 });
    s.annotate(false);
}

/// Every library footprint in rows on one board (3D showcase of the generated models).
pub fn showcase() -> Board {
    use super::board::FpInst;
    let mut b = Board::default();
    let lib = super::footprints::library();
    let (mut x, mut y, mut row_h) = (5.0, 5.0, 0.0_f64);
    let width = 160.0;
    let mut fps = vec![];
    for f in lib {
        let (lo, hi) = f.court;
        let w = hi.x - lo.x;
        if x + w > width - 5.0 {
            x = 5.0;
            y += row_h + 4.0;
            row_h = 0.0;
        }
        let value = if f.id.contains("Resistor") { "4k7" } else { "" };
        fps.push(FpInst { reference: format!("X{}", fps.len() + 1), value: value.into(), fp: f.clone(), pos: p(x - lo.x, y - lo.y), rot: 0.0, bottom: false, nets: vec![String::new(); f.pads.len()], locked: false });
        x += w + 4.0;
        row_h = row_h.max(hi.y - lo.y);
    }
    b.rect_outline(0.0, 0.0, width, y + row_h + 5.0);
    b.footprints = fps;
    b
}
