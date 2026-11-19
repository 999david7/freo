//! Real KiCad projects (KiCad's own demos, not shipped with Freo). Point FREO_KICAD_DEMOS
//! at a folder with KiCad's `demos/` sub-folders (pic_programmer, complex_hierarchy …) to run
//! these checks; without it they pass silently.

use freo::pcb::*;

fn demos() -> Option<std::path::PathBuf> {
    std::env::var_os("FREO_KICAD_DEMOS").map(std::path::PathBuf::from).filter(|d| d.is_dir())
}

fn check(dir: &std::path::Path, name: &str) {
    let path = dir.join(name).join(format!("{}.kicad_pro", name));
    if !path.exists() {
        eprintln!("{} fehlt – übersprungen", path.display());
        return;
    }
    let (pr, got) = kicad::read_project(&path).unwrap_or_else(|e| panic!("{}: {}", name, e));
    let s = &pr.schematic;
    let b = &pr.board;
    eprintln!(
        "{}: {:?}; Blätter {}, Symbole {}, Netze (Schaltplan) {}; Platine: {} Footprints, {} Bahnen, {} Vias, {} Zonen, Umriss {}, {} Lagen, {} Klassen",
        name,
        got,
        s.sheets.len(),
        s.all_symbols().len(),
        s.netlist().nets.len(),
        b.footprints.len(),
        b.tracks.len(),
        b.vias.len(),
        b.zones.len(),
        b.outline.len(),
        b.layers,
        b.rules.classes.len()
    );
    assert!(!b.footprints.is_empty() && !b.outline.is_empty() && !b.tracks.is_empty());
    // the board's pad nets agree with the schematic netlist for most pads
    let nl = s.netlist();
    let mut same = 0;
    let mut total = 0;
    let mut diff = vec![];
    for f in &b.footprints {
        for (k, pad) in f.fp.pads.iter().enumerate() {
            if pad.num.is_empty() || f.nets[k].is_empty() {
                continue;
            }
            total += 1;
            let want = nl.net_of(&f.reference, &pad.num);
            let have = &f.nets[k];
            // KiCad prefixes local nets with the sheet path ("/VCC"); compare by the last part
            let norm = |x: &str| x.trim_start_matches('/').rsplit('/').next().unwrap_or("").to_uppercase();
            if want.is_some_and(|w| norm(w) == norm(have) || (w.starts_with("Net-") && have.starts_with("Net-")) || (w.starts_with("unconnected") && have.starts_with("unconnected"))) {
                same += 1;
            } else if diff.len() < 12 {
                diff.push(format!("{} Pad {}: Platine {} / Schaltplan {:?}", f.reference, pad.num, have, want));
            }
        }
    }
    eprintln!("Pads mit gleichem Netz: {}/{}; Beispiele: {:#?}", same, total, diff);
    assert!(same as f64 >= total as f64 * 0.9, "{}: nur {}/{} Pads stimmen", name, same, total);
    // DRC runs; the design is routed: few unrouted connections
    let v = drc::run(b, None);
    let un = v.iter().filter(|x| x.kind == drc::Kind::Unrouted).count();
    let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
    for x in &v {
        *kinds.entry(x.kind.name()).or_default() += 1;
    }
    eprintln!("DRC: {:?}", kinds);
    assert!(un <= b.footprints.len() / 2, "{} offene Verbindungen", un);
    // Gerber, round trip through our own writer
    let files = gerber::fab_files(b, name);
    assert!(files.iter().all(|(_, t)| !t.is_empty()));
    let back = kicad::read_pcb(&kicad::write_pcb(b, name)).unwrap();
    assert_eq!(back.footprints.len(), b.footprints.len());
    assert_eq!(back.tracks.len(), b.tracks.len());
}

#[test]
fn kicad_demo_pic_programmer() {
    if let Some(d) = demos() {
        check(&d, "pic_programmer");
    }
}

#[test]
fn kicad_demo_complex_hierarchy() {
    if let Some(d) = demos() {
        check(&d, "complex_hierarchy");
    }
}
