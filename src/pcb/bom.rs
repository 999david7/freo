//! Bill of materials grouped by value and footprint (KiCad's default grouping).

use super::schematic::{natural, Schematic};

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub refs: Vec<String>,
    pub value: String,
    pub footprint: String,
    pub description: String,
}

impl Line {
    pub fn qty(&self) -> usize {
        self.refs.len()
    }
}

pub fn lines(s: &Schematic) -> Vec<Line> {
    let flat = s.flatten();
    let s = &flat;
    let mut out: Vec<Line> = vec![];
    let mut seen: Vec<&str> = vec![];
    for (i, sym) in s.symbols.iter().enumerate() {
        if s.is_power(i) || seen.contains(&sym.reference.as_str()) {
            continue;
        }
        seen.push(&sym.reference);
        let desc = s.def(&sym.lib).map(|d| d.description.clone()).unwrap_or_default();
        match out.iter_mut().find(|l| l.value == sym.value && l.footprint == sym.footprint) {
            Some(l) => l.refs.push(sym.reference.clone()),
            None => out.push(Line { refs: vec![sym.reference.clone()], value: sym.value.clone(), footprint: sym.footprint.clone(), description: desc }),
        }
    }
    for l in &mut out {
        l.refs.sort_by_key(|a| natural(a));
    }
    out.sort_by(|a, b| natural(&a.refs[0]).cmp(&natural(&b.refs[0])));
    out
}

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// CSV with a header row (semicolon separated for German spreadsheet programs).
pub fn csv(s: &Schematic) -> String {
    let mut out = String::from("Pos;Referenz;Menge;Wert;Footprint;Beschreibung\n");
    for (i, l) in lines(s).iter().enumerate() {
        out.push_str(&format!("{};{};{};{};{};{}\n", i + 1, q(&l.refs.join(", ")), l.qty(), q(&l.value), q(l.footprint.rsplit(':').next().unwrap_or("")), q(&l.description)));
    }
    out
}
