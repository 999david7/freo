//! NC code for the controllers. Written only for a program the simulation passed. Every
//! file starts by raising Z before any sideways move and ends with Z up and the spindle off.
//! GRBL cannot change tools with length offsets: there is one file per tool.

use super::machines::Controller;
use super::*;
use std::fmt::Write as _;

fn n(v: f64) -> String {
    let s = format!("{:.3}", if v.abs() < 5e-4 { 0.0 } else { v });
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s
    }
}

/// Plain ASCII for comments (some controllers stop on other characters).
fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ä' => "ae".into(),
            'ö' => "oe".into(),
            'ü' => "ue".into(),
            'Ä' => "Ae".into(),
            'Ö' => "Oe".into(),
            'Ü' => "Ue".into(),
            'ß' => "ss".into(),
            '–' | '—' => "-".into(),
            'Ø' => "D".into(),
            '×' => "x".into(),
            '°' => "deg".into(),
            '(' | ')' => " ".into(),
            c if c.is_ascii() => c.to_string(),
            _ => " ".into(),
        })
        .collect()
}

fn dwell(c: Controller, secs: f64) -> String {
    match c {
        Controller::Fanuc => format!("G04 X{}", n(secs)),
        Controller::Haas => format!("G04 P{}", n(secs)),
        _ => format!("G4 P{}", n(secs)),
    }
}

/// The NC files: (file-name suffix, text). Refuses a program that did not pass the check.
pub fn write(out: &Output, s: &Setup, part_name: &str, now: &str) -> Result<Vec<(String, String)>, String> {
    if !out.sim.safe() {
        return Err(format!("Die Prüfung hat {} Fehler gefunden – kein NC-Code, bis alles behoben ist.", out.sim.errors()));
    }
    let c = s.machine.controller;
    let groups: Vec<Vec<&OpPath>> = if c.tool_change() {
        vec![out.program.ops.iter().collect()]
    } else {
        // one file per tool, in order
        let mut g: Vec<Vec<&OpPath>> = vec![];
        for op in &out.program.ops {
            match g.last_mut() {
                Some(last) if last[0].tool.number == op.tool.number => last.push(op),
                _ => g.push(vec![op]),
            }
        }
        g
    };
    let many = groups.len() > 1;
    let safe = out.program.safe_z;
    let mut files = vec![];
    for (gi, ops) in groups.iter().enumerate() {
        let mut g = String::new();
        let fanucish = matches!(c, Controller::Fanuc | Controller::Haas);
        if fanucish {
            let _ = writeln!(g, "%");
            let _ = writeln!(g, "O{:04} ({})", 1000 + gi, ascii(part_name).to_uppercase());
        }
        let _ = writeln!(g, "(Freo CAM {} - {})", env!("CARGO_PKG_VERSION"), ascii(now));
        let _ = writeln!(g, "(Teil {} - Material {})", ascii(part_name), ascii(s.material().name));
        let _ = writeln!(g, "(Maschine {} - {})", ascii(&s.machine.name()), c.name());
        let size = out.job.stock_size();
        let _ = writeln!(g, "(Rohteil {} x {} x {} mm - Nullpunkt {})", n(size.x), n(size.y), n(size.z), ascii(s.origin.name()));
        let _ = writeln!(g, "(Sicherheitshoehe Z{} - geprueft: keine Kollisionen)", n(safe));
        if many {
            let _ = writeln!(g, "(Datei {} von {}: nur Werkzeug {} - nach dem Wechsel Z neu antasten!)", gi + 1, groups.len(), ascii(&ops[0].tool.label()));
        }
        for t in ops.iter().map(|o| &o.tool) {
            let _ = writeln!(g, "({} - Schneidenlaenge {} - Auskragung {})", ascii(&t.label()), n(t.flute_length), n(t.stickout));
        }
        let _ = writeln!(g, "G90 G94 G17 G21");
        let _ = writeln!(g, "G54");
        // first motion: straight up to the safe height
        let _ = writeln!(g, "G0 Z{}", n(safe));
        let mut tool_now: Option<u32> = None;
        let mut feed_now = -1.0;
        let mut cur = DVec3::ZERO;
        let mut coolant = false;
        for op in ops {
            let _ = writeln!(g, "({})", ascii(&op.name));
            if tool_now != Some(op.tool.number) {
                if tool_now.is_some() {
                    let _ = writeln!(g, "M5");
                    if coolant {
                        let _ = writeln!(g, "M9");
                        coolant = false;
                    }
                    let _ = writeln!(g, "G0 Z{}", n(safe));
                }
                if c.tool_change() {
                    let _ = writeln!(g, "T{} M6", op.tool.number);
                    let _ = writeln!(g, "G43 H{}", op.tool.number);
                    let _ = writeln!(g, "G0 Z{}", n(safe));
                }
                tool_now = Some(op.tool.number);
            }
            let _ = writeln!(g, "S{} M3", op.rpm.round() as i64);
            let _ = writeln!(g, "{}", dwell(c, 3.0));
            if op.coolant && !coolant {
                let _ = writeln!(g, "M8");
                coolant = true;
            }
            for mv in &op.moves {
                let from = cur;
                cur = mv.to;
                if mv.rapid {
                    let _ = writeln!(g, "G0 X{} Y{} Z{}", n(mv.to.x), n(mv.to.y), n(mv.to.z));
                } else {
                    let mut line = match mv.arc {
                        // arcs relative to their start (I, J), in the XY plane (G17)
                        Some(c) => format!("{} X{} Y{} Z{} I{} J{}", if c[2] < 0.0 { "G2" } else { "G3" }, n(mv.to.x), n(mv.to.y), n(mv.to.z), n(c[0] - from.x), n(c[1] - from.y)),
                        None => format!("G1 X{} Y{} Z{}", n(mv.to.x), n(mv.to.y), n(mv.to.z)),
                    };
                    if (mv.feed - feed_now).abs() > 1e-6 {
                        let _ = write!(line, " F{}", mv.feed.round() as i64);
                        feed_now = mv.feed;
                    }
                    let _ = writeln!(g, "{}", line);
                }
            }
            let _ = writeln!(g, "G0 Z{}", n(safe));
        }
        let _ = writeln!(g, "M5");
        if coolant {
            let _ = writeln!(g, "M9");
        }
        let _ = writeln!(g, "G0 Z{}", n(safe));
        if fanucish {
            let _ = writeln!(g, "G91 G28 Z0");
            let _ = writeln!(g, "G90");
        }
        let _ = writeln!(g, "M30");
        if fanucish {
            let _ = writeln!(g, "%");
        }
        let suffix = if many { format!("_{}_T{}", gi + 1, ops[0].tool.number) } else { String::new() };
        files.push((suffix, g));
    }
    Ok(files)
}
