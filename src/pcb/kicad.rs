//! KiCad file exchange: .kicad_sch (schematic, with the used library symbols embedded) and
//! .kicad_pcb (board) in KiCad 8's s-expression format, both directions.
//!
//! KiCad library symbols are y-up while Freo's (and KiCad's schematic sheet) are y-down, so
//! symbol geometry is mirrored in y on the way through.

use super::board::{layer_from_name, layer_name, Board, FpInst, Track, Via, Zone};
use super::footprints::{self, Body3, Footprint, Pad, PadShape};
use super::geom::{arc3, p, P};
use super::schematic::{Label, LabelKind, Schematic, Sheet, SheetPin, SymInst, Wire};
use super::sexpr::{a, l, n, s, Sx};
use super::symbols::{Fill, Gfx, Pin, PinType, Symbol};
use std::collections::HashMap;

struct Ids(u64);
impl Ids {
    fn next(&mut self) -> Sx {
        self.0 += 1;
        l("uuid", vec![s(&format!("{:08x}-0000-4000-8000-{:012x}", 0xf4e0_0000u64 + (self.0 >> 32), self.0 & 0xffff_ffff_ffff))])
    }
}

fn xy(q: P) -> Sx {
    l("xy", vec![n(q.x), n(q.y)])
}
fn at(q: P, r: f64) -> Sx {
    l("at", vec![n(q.x), n(q.y), n(r)])
}
fn at2(q: P) -> Sx {
    l("at", vec![n(q.x), n(q.y)])
}
fn font(size: f64) -> Sx {
    l("effects", vec![l("font", vec![l("size", vec![n(size), n(size)])])])
}
fn font_hide(size: f64) -> Sx {
    l("effects", vec![l("font", vec![l("size", vec![n(size), n(size)])]), l("hide", vec![a("yes")])])
}
fn stroke(w: f64) -> Sx {
    l("stroke", vec![l("width", vec![n(w)]), l("type", vec![a("default")])])
}
fn fill(f: Fill) -> Sx {
    l(
        "fill",
        vec![l(
            "type",
            vec![a(match f {
                Fill::None => "none",
                Fill::Outline => "outline",
                Fill::Background => "background",
            })],
        )],
    )
}
fn flip(q: P) -> P {
    p(q.x, -q.y)
}

// ------------------------------------------------------------------ schematic out

fn gfx_sx(gf: &Gfx) -> Sx {
    match gf {
        Gfx::Poly(pts, f) => l("polyline", vec![l("pts", pts.iter().map(|q| xy(flip(*q))).collect()), stroke(0.254), fill(*f)]),
        Gfx::Rect(a0, b0, f) => l("rectangle", vec![l("start", vec![n(a0.x), n(-a0.y)]), l("end", vec![n(b0.x), n(-b0.y)]), stroke(0.254), fill(*f)]),
        Gfx::Circle(c, r, f) => l("circle", vec![l("center", vec![n(c.x), n(-c.y)]), l("radius", vec![n(*r)]), stroke(0.254), fill(*f)]),
        Gfx::Arc(a0, m, b0) => l("arc", vec![l("start", vec![n(a0.x), n(-a0.y)]), l("mid", vec![n(m.x), n(-m.y)]), l("end", vec![n(b0.x), n(-b0.y)]), stroke(0.254), fill(Fill::None)]),
        Gfx::Text(c, t, size) => l("text", vec![s(t), at(flip(*c), 0.0), font(*size)]),
    }
}

fn pin_sx(q: &Pin) -> Sx {
    let mut pv = vec![a(q.kind.kicad()), a("line"), at(flip(q.pos), q.dir), l("length", vec![n(q.len)])];
    if q.hidden {
        pv.push(l("hide", vec![a("yes")]));
    }
    pv.push(l("name", vec![s(&q.name), font(1.27)]));
    pv.push(l("number", vec![s(&q.num), font(1.27)]));
    l("pin", pv)
}

/// A library symbol in KiCad form (also used for .kicad_sym libraries).
pub fn lib_symbol(sym: &Symbol) -> Sx {
    let short = sym.short().to_string();
    let mut v = vec![s(&sym.id)];
    if sym.power {
        v.push(l("power", vec![]));
    }
    if !sym.show_pin_numbers {
        v.push(l("pin_numbers", vec![l("hide", vec![a("yes")])]));
    }
    v.push(l("pin_names", vec![l("offset", vec![n(if sym.show_pin_names { 1.016 } else { 0.0 })]), l("hide", vec![a(if sym.show_pin_names { "no" } else { "yes" })])]));
    v.push(l("exclude_from_sim", vec![a("no")]));
    v.push(l("in_bom", vec![a(if sym.power { "no" } else { "yes" })]));
    v.push(l("on_board", vec![a(if sym.power { "no" } else { "yes" })]));
    let (lo, hi) = sym.bounds();
    v.push(l("property", vec![s("Reference"), s(&sym.prefix), at(p(hi.x + 1.0, -(lo.y + hi.y) / 2.0), 0.0), font(1.27)]));
    v.push(l("property", vec![s("Value"), s(&sym.value), at(p(hi.x + 1.0, -(lo.y + hi.y) / 2.0 - 2.54), 0.0), font(1.27)]));
    v.push(l("property", vec![s("Footprint"), s(&sym.footprint), at(p(0.0, 0.0), 0.0), font_hide(1.27)]));
    v.push(l("property", vec![s("Description"), s(&sym.description), at(p(0.0, 0.0), 0.0), font_hide(1.27)]));
    if !sym.alt_footprints.is_empty() {
        v.push(l("property", vec![s("ki_fp_filters"), s(&sym.alt_footprints.iter().map(|f| f.rsplit(':').next().unwrap_or(f).to_string()).collect::<Vec<_>>().join(" ")), at(P::ZERO, 0.0), font_hide(1.27)]));
    }
    let units = sym.units.max(1);
    for u in 0..=units {
        let mut body = vec![s(&format!("{}_{}_1", short, u))];
        if u == 0 {
            body.extend(sym.gfx.iter().map(gfx_sx));
        } else {
            body.extend(sym.unit_gfx.iter().filter(|(k, _)| *k == u).map(|(_, g)| gfx_sx(g)));
        }
        body.extend(sym.pins.iter().filter(|q| q.unit == u || (u == 1 && units == 1 && q.unit == 0 && false)).map(pin_sx));
        if u == 0 {
            // single-unit symbols keep their pins in unit 1 like KiCad's own libraries
        }
        if body.len() > 1 {
            v.push(Sx::List(std::iter::once(a("symbol")).chain(body).collect()));
        }
    }
    l("symbol", v)
}

fn label_sx(lb: &Label, ids: &mut Ids) -> Sx {
    let eff = |j: &str| l("effects", vec![l("font", vec![l("size", vec![n(1.27), n(1.27)])]), l("justify", vec![a(j)])]);
    match lb.kind {
        LabelKind::Local => l("label", vec![s(&lb.text), at(lb.pos, lb.rot), l("fields_autoplaced", vec![a("yes")]), l("effects", vec![l("font", vec![l("size", vec![n(1.27), n(1.27)])]), l("justify", vec![a("left"), a("bottom")])]), ids.next()]),
        LabelKind::Global => l("global_label", vec![s(&lb.text), l("shape", vec![a("bidirectional")]), at(lb.pos, lb.rot), l("fields_autoplaced", vec![a("yes")]), eff("left"), ids.next()]),
        LabelKind::Hier => l("hierarchical_label", vec![s(&lb.text), l("shape", vec![a("bidirectional")]), at(lb.pos, lb.rot), l("fields_autoplaced", vec![a("yes")]), eff("left"), ids.next()]),
    }
}

/// File name of a sub-sheet ("<stem>-<Blattname>.kicad_sch").
pub fn sheet_file(stem: &str, name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{}-{}.kicad_sch", stem, clean)
}

fn sch_body(sch: &Schematic, project: &str, stem: &str, ids: &mut Ids, path_uuids: &str, root_id: &str, files: &mut Vec<(String, String)>) -> Vec<Sx> {
    let mut v = vec![];
    let mut used: Vec<&str> = sch.symbols.iter().map(|x| x.lib.as_str()).collect();
    used.sort();
    used.dedup();
    v.push(l("lib_symbols", used.iter().filter_map(|id| sch.def(id)).map(lib_symbol).collect()));
    for j in sch.junctions() {
        v.push(l("junction", vec![at2(j), l("diameter", vec![n(0.0)]), l("color", vec![n(0.0), n(0.0), n(0.0), n(0.0)]), ids.next()]));
    }
    for q in &sch.no_connects {
        v.push(l("no_connect", vec![at2(*q), ids.next()]));
    }
    for e in &sch.bus_entries {
        v.push(l("bus_entry", vec![at2(e.a), l("size", vec![n(e.b.x - e.a.x), n(e.b.y - e.a.y)]), stroke(0.0), ids.next()]));
    }
    for w in &sch.wires {
        v.push(l("wire", vec![l("pts", vec![xy(w.a), xy(w.b)]), stroke(0.0), ids.next()]));
    }
    for w in &sch.buses {
        v.push(l("bus", vec![l("pts", vec![xy(w.a), xy(w.b)]), stroke(0.0), ids.next()]));
    }
    for lb in &sch.labels {
        v.push(label_sx(lb, ids));
    }
    for (i, x) in sch.symbols.iter().enumerate() {
        let d = sch.def(&x.lib);
        let mut sv = vec![l("lib_id", vec![s(&x.lib)]), at(x.pos, x.rot)];
        if x.mirror {
            sv.push(l("mirror", vec![a("y")]));
        }
        sv.extend([l("unit", vec![n(x.unit as f64)]), l("exclude_from_sim", vec![a("no")]), l("in_bom", vec![a("yes")]), l("on_board", vec![a("yes")]), l("dnp", vec![a("no")]), ids.next()]);
        let power = d.is_some_and(|d| d.power);
        let (lo, hi) = d.map(|d| d.unit_bounds(x.unit)).unwrap_or((P::ZERO, P::ZERO));
        let rpos = Schematic::xform(x, p(hi.x + 1.27, (lo.y + hi.y) / 2.0 - 1.27));
        let vpos = Schematic::xform(x, p(hi.x + 1.27, (lo.y + hi.y) / 2.0 + 1.27));
        sv.push(l("property", vec![s("Reference"), s(&x.reference), at(rpos, 0.0), if power { font_hide(1.27) } else { font(1.27) }]));
        sv.push(l("property", vec![s("Value"), s(&x.value), at(vpos, 0.0), font(1.27)]));
        sv.push(l("property", vec![s("Footprint"), s(&x.footprint), at(x.pos, 0.0), font_hide(1.27)]));
        for (k, val) in &x.fields {
            sv.push(l("property", vec![s(k), s(val), at(x.pos, 0.0), font_hide(1.27)]));
        }
        for (_, q) in sch.pin_positions(i) {
            sv.push(l("pin", vec![s(&q.num), ids.next()]));
        }
        sv.push(l("instances", vec![l("project", vec![s(project), l("path", vec![s(&format!("/{}{}", root_id, path_uuids)), l("reference", vec![s(&x.reference)]), l("unit", vec![n(x.unit as f64)])])])]));
        v.push(l("symbol", sv));
    }
    for sh in &sch.sheets {
        let file = sheet_file(stem, &sh.name);
        let sid = ids.next();
        let sid_text = sid.at(1).unwrap_or("").to_string();
        let mut shv = vec![at2(sh.pos), l("size", vec![n(sh.size.x), n(sh.size.y)]), l("fields_autoplaced", vec![a("yes")]), stroke(0.1524), l("fill", vec![l("color", vec![n(0.0), n(0.0), n(0.0), n(0.0)])]), sid];
        shv.push(l("property", vec![s("Sheetname"), s(&sh.name), at(sh.pos - p(0.0, 0.7), 0.0), l("effects", vec![l("font", vec![l("size", vec![n(1.27), n(1.27)])]), l("justify", vec![a("left"), a("bottom")])])]));
        shv.push(l("property", vec![s("Sheetfile"), s(&file), at(sh.pos + p(0.0, sh.size.y + 0.6), 0.0), l("effects", vec![l("font", vec![l("size", vec![n(1.27), n(1.27)])]), l("justify", vec![a("left"), a("top")])])]));
        for pin in &sh.pins {
            let ang = if (pin.pos.x - sh.pos.x).abs() < 1e-6 { 180.0 } else if (pin.pos.x - sh.pos.x - sh.size.x).abs() < 1e-6 { 0.0 } else if (pin.pos.y - sh.pos.y).abs() < 1e-6 { 90.0 } else { 270.0 };
            shv.push(l("pin", vec![s(&pin.name), a("bidirectional"), at(pin.pos, ang), font(1.27), ids.next()]));
        }
        shv.push(l("instances", vec![l("project", vec![s(project), l("path", vec![s(&format!("/{}{}", root_id, path_uuids)), l("page", vec![s(&format!("{}", files.len() + 2))])])])]));
        v.push(l("sheet", shv));
        // the sub-sheet's own file
        let child_path = format!("{}/{}", path_uuids, sid_text);
        let mut cv = vec![l("version", vec![a("20231120")]), l("generator", vec![s("freo")]), l("generator_version", vec![s("1.2")]), ids.next(), l("paper", vec![s("A4")])];
        cv.push(l("title_block", vec![l("title", vec![s(&sh.name)])]));
        let slot = files.len();
        files.push((file.clone(), String::new()));
        cv.extend(sch_body(&sh.sch, project, stem, ids, &child_path, root_id, files));
        files[slot].1 = super::sexpr::write(&l("kicad_sch", cv));
    }
    v
}

/// The schematic as KiCad files: the root sheet first, then one file per sub-sheet.
pub fn write_sch_files(sch: &Schematic, project: &str) -> Vec<(String, String)> {
    let mut ids = Ids(0);
    let root = ids.next();
    let root_id = root.at(1).unwrap_or("").to_string();
    let mut v = vec![l("version", vec![a("20231120")]), l("generator", vec![s("freo")]), l("generator_version", vec![s("1.2")]), root, l("paper", vec![s("A4")])];
    v.push(l("title_block", vec![l("title", vec![s(if sch.title.is_empty() { project } else { &sch.title })]), l("company", vec![s("Freo")])]));
    let mut files = vec![];
    v.extend(sch_body(sch, project, project, &mut ids, "", &root_id, &mut files));
    v.push(l("sheet_instances", vec![l("path", vec![s("/"), l("page", vec![s("1")])])]));
    let mut out = vec![(format!("{}.kicad_sch", project), super::sexpr::write(&l("kicad_sch", v)))];
    out.extend(files);
    out
}

/// The root sheet only (sub-sheets need [`write_sch_files`]).
pub fn write_sch(sch: &Schematic, project: &str) -> String {
    write_sch_files(sch, project).swap_remove(0).1
}

// ------------------------------------------------------------------ schematic in

fn pt(sx: &Sx, key: &str) -> Option<P> {
    let c = sx.get(key)?;
    Some(p(c.atn(1)?, c.atn(2)?))
}

fn hidden(c: &Sx) -> bool {
    c.get("hide").is_some_and(|h| h.at(1) != Some("no")) || c.items().iter().any(|x| x.text() == Some("hide"))
}

/// One library symbol; `parent` resolves `(extends "…")` in .kicad_sym libraries.
fn read_lib_symbol(sx: &Sx, parent: &dyn Fn(&str) -> Option<Symbol>) -> Option<Symbol> {
    let id = sx.at(1)?.to_string();
    let base = sx.val("extends").and_then(parent);
    let mut sym = base.clone().unwrap_or(Symbol {
        id: id.clone(),
        category: "Importiert".into(),
        description: String::new(),
        prefix: "U".into(),
        value: String::new(),
        footprint: String::new(),
        pins: vec![],
        gfx: vec![],
        power: false,
        show_pin_numbers: true,
        show_pin_names: true,
        alt_footprints: vec![],
        units: 1,
        unit_gfx: vec![],
    });
    sym.id = id.clone();
    if let Some(v) = sx.property("Description").or_else(|| sx.property("ki_description")) {
        sym.description = v.to_string();
    }
    if let Some(v) = sx.property("Reference") {
        sym.prefix = v.to_string();
    }
    if let Some(v) = sx.property("Value") {
        sym.value = v.to_string();
    }
    if let Some(v) = sx.property("Footprint") {
        sym.footprint = v.to_string();
        if !v.is_empty() && !sym.alt_footprints.iter().any(|f| f == v) {
            sym.alt_footprints.insert(0, v.to_string());
        }
    }
    if sx.get("power").is_some() {
        sym.power = true;
    }
    if let Some(c) = sx.get("pin_numbers") {
        sym.show_pin_numbers = !hidden(c);
    }
    if let Some(c) = sx.get("pin_names") {
        sym.show_pin_names = !hidden(c);
    }
    if base.is_some() {
        return Some(sym);
    }
    let fillk = |g: &Sx| match g.get("fill").and_then(|f| f.val("type")) {
        Some("outline") => Fill::Outline,
        Some("background") => Fill::Background,
        _ => Fill::None,
    };
    for u in std::iter::once(sx).chain(sx.all("symbol")) {
        // sub-symbols "NAME_U_S": U = unit (0 = shared), S = body style (2 = De Morgan, skipped)
        let mut unit = 0u32;
        if let Some(name) = u.at(1) {
            if name != id {
                let parts: Vec<&str> = name.rsplit('_').take(2).collect();
                if parts.len() == 2 {
                    if parts[0] == "2" {
                        continue;
                    }
                    unit = parts[1].parse().unwrap_or(0);
                }
            }
        }
        sym.units = sym.units.max(unit);
        for g in u.items() {
            let gf = match g.head() {
                Some("polyline") => Some(Gfx::Poly(g.get("pts").map(|c| c.all("xy").filter_map(|q| Some(p(q.atn(1)?, -q.atn(2)?))).collect()).unwrap_or_default(), fillk(g))),
                Some("rectangle") => match (pt(g, "start"), pt(g, "end")) {
                    (Some(a0), Some(b0)) => Some(Gfx::Rect(flip(a0), flip(b0), fillk(g))),
                    _ => None,
                },
                Some("circle") => pt(g, "center").map(|c| Gfx::Circle(flip(c), g.valn("radius").unwrap_or(1.0), fillk(g))),
                Some("arc") => match (pt(g, "start"), pt(g, "mid"), pt(g, "end")) {
                    (Some(a0), Some(m), Some(b0)) => Some(Gfx::Arc(flip(a0), flip(m), flip(b0))),
                    _ => None,
                },
                Some("text") => match (g.at(1), pt(g, "at")) {
                    (Some(t), Some(q)) => Some(Gfx::Text(flip(q), t.to_string(), 1.27)),
                    _ => None,
                },
                Some("pin") => {
                    let Some(atx) = g.get("at") else { continue };
                    let (Some(x), Some(y)) = (atx.atn(1), atx.atn(2)) else { continue };
                    sym.pins.push(Pin {
                        num: g.get("number").and_then(|c| c.at(1)).unwrap_or("").to_string(),
                        name: g.get("name").and_then(|c| c.at(1)).unwrap_or("").to_string(),
                        pos: p(x, -y),
                        dir: atx.atn(3).unwrap_or(0.0),
                        len: g.valn("length").unwrap_or(2.54),
                        kind: PinType::from_kicad(g.at(1).unwrap_or("passive")),
                        hidden: hidden(g),
                        unit,
                    });
                    None
                }
                _ => None,
            };
            if let Some(gf) = gf {
                if unit == 0 {
                    sym.gfx.push(gf);
                } else {
                    sym.unit_gfx.push((unit, gf));
                }
            }
        }
    }
    sym.units = sym.units.max(1);
    // power symbols converted from old libraries lack "(power)": reference #PWR and one
    // power pin make them global supply symbols like in KiCad
    if !sym.power && sym.prefix.starts_with("#PWR") && sym.pins.len() == 1 && matches!(sym.pins[0].kind, PinType::PowerIn | PinType::PowerOut) {
        sym.power = true;
    }
    // single-unit symbols: unit-1 contents count for every unit
    if sym.units == 1 {
        for q in &mut sym.pins {
            q.unit = 0;
        }
        let ug = std::mem::take(&mut sym.unit_gfx);
        sym.gfx.extend(ug.into_iter().map(|(_, g)| g));
    }
    Some(sym)
}

/// Symbols of a KiCad symbol library (.kicad_sym); ids become "<lib>:<name>".
pub fn read_symbol_lib(src: &str, lib: &str) -> Result<Vec<Symbol>, String> {
    let root = super::sexpr::parse(src)?;
    if root.head() != Some("kicad_symbol_lib") {
        return Err("Keine KiCad-Symbolbibliothek (.kicad_sym)".into());
    }
    let mut out: Vec<Symbol> = vec![];
    for c in root.all("symbol") {
        let found = |name: &str| out.iter().find(|s| s.short() == name).cloned();
        if let Some(mut sym) = read_lib_symbol(c, &found) {
            if !sym.id.contains(':') {
                sym.id = format!("{}:{}", lib, sym.id);
            }
            sym.category = lib.to_string();
            out.push(sym);
        }
    }
    Ok(out)
}

/// A .kicad_sym library holding the given symbols.
pub fn write_symbol_lib(syms: &[Symbol]) -> String {
    let mut v = vec![l("version", vec![a("20231120")]), l("generator", vec![s("freo")])];
    for sym in syms {
        let mut x = sym.clone();
        x.id = sym.short().to_string();
        v.push(lib_symbol(&x));
    }
    super::sexpr::write(&l("kicad_symbol_lib", v))
}

/// KiCad escapes some characters in names ("{slash}" for '/').
pub fn unescape(s: &str) -> String {
    if !s.contains('{') {
        return s.to_string();
    }
    s.replace("{slash}", "/").replace("{backslash}", "\\").replace("{lt}", "<").replace("{gt}", ">").replace("{colon}", ":").replace("{dblquote}", "\"").replace("{space}", " ").replace("{tab}", "\t").replace("{return}", "\n").replace("{brace}", "{")
}

/// Per symbol of a sheet file: (instance path, reference, unit) for every use of the sheet.
type Instances = Vec<Vec<(String, String, u32)>>;

/// Sheets: (file, sheet uuid).
fn read_sch_text(src: &str) -> Result<(Schematic, Vec<String>), String> {
    read_sch_text_full(src).map(|(s, f, _, _)| (s, f.into_iter().map(|x| x.0).collect()))
}

fn read_sch_text_full(src: &str) -> Result<(Schematic, Vec<(String, String)>, Instances, String), String> {
    let root = super::sexpr::parse(src)?;
    if root.head() != Some("kicad_sch") {
        return Err("Keine KiCad-Schaltplandatei (.kicad_sch)".into());
    }
    let mut sch = Schematic { title: root.get("title_block").and_then(|t| t.val("title")).unwrap_or("").to_string(), ..Default::default() };
    let root_uuid = root.val("uuid").unwrap_or("").to_string();
    let mut instances: Instances = vec![];
    if let Some(libs) = root.get("lib_symbols") {
        for ls in libs.all("symbol") {
            let known: Vec<Symbol> = sch.local_lib.clone();
            let find = move |name: &str| known.iter().find(|s| s.short() == name || s.id == name).cloned();
            if let Some(sym) = read_lib_symbol(ls, &find) {
                sch.local_lib.push(sym);
            }
        }
    }
    let mut files: Vec<(String, String)> = vec![];
    for c in root.items() {
        match c.head() {
            Some(h @ ("wire" | "bus")) => {
                let pts: Vec<P> = c.get("pts").map(|k| k.all("xy").filter_map(|q| Some(p(q.atn(1)?, q.atn(2)?))).collect()).unwrap_or_default();
                for w in pts.windows(2) {
                    if h == "wire" {
                        sch.wires.push(Wire { a: w[0], b: w[1] });
                    } else {
                        sch.buses.push(Wire { a: w[0], b: w[1] });
                    }
                }
            }
            Some("bus_entry") => {
                if let (Some(q), Some(sz)) = (pt(c, "at"), pt(c, "size")) {
                    sch.bus_entries.push(Wire { a: q, b: q + sz });
                }
            }
            Some("no_connect") => {
                if let Some(q) = pt(c, "at") {
                    sch.no_connects.push(q);
                }
            }
            Some(h @ ("label" | "global_label" | "hierarchical_label")) => {
                if let (Some(t), Some(q)) = (c.at(1), pt(c, "at")) {
                    let kind = match h {
                        "label" => LabelKind::Local,
                        "global_label" => LabelKind::Global,
                        _ => LabelKind::Hier,
                    };
                    sch.labels.push(Label { pos: q, text: unescape(t), kind, rot: c.get("at").and_then(|x| x.atn(3)).unwrap_or(0.0) });
                }
            }
            Some("symbol") => {
                let lib = c.val("lib_id").unwrap_or("").to_string();
                let atx = c.get("at");
                // the reference of this instance (newer files keep it under instances)
                let mut inst = vec![];
                if let Some(i) = c.get("instances") {
                    for pj in i.all("project") {
                        for pa in pj.all("path") {
                            if let (Some(path), Some(r)) = (pa.at(1), pa.val("reference")) {
                                inst.push((path.to_string(), r.to_string(), pa.valn("unit").unwrap_or(1.0) as u32));
                            }
                        }
                    }
                }
                let reference = inst.first().map(|x| x.1.clone()).or_else(|| c.property("Reference").map(|x| x.to_string())).unwrap_or_else(|| "?".into());
                instances.push(inst);
                sch.symbols.push(SymInst {
                    lib,
                    reference,
                    value: c.property("Value").unwrap_or("").to_string(),
                    footprint: c.property("Footprint").unwrap_or("").to_string(),
                    pos: atx.and_then(|x| Some(p(x.atn(1)?, x.atn(2)?))).unwrap_or(P::ZERO),
                    // "mirror x" (upside down) is "mirror y" turned by 180°
                    rot: (atx.and_then(|x| x.atn(3)).unwrap_or(0.0) + if c.val("mirror") == Some("x") { 180.0 } else { 0.0 }).rem_euclid(360.0),
                    mirror: matches!(c.val("mirror"), Some("x") | Some("y")),
                    fields: c.all("property").filter(|pr| !matches!(pr.at(1), Some("Reference" | "Value" | "Footprint" | "Datasheet" | "Description"))).filter_map(|pr| Some((pr.at(1)?.to_string(), pr.at(2)?.to_string()))).collect(),
                    unit: c.valn("unit").unwrap_or(1.0).max(1.0) as u32,
                });
            }
            Some("sheet") => {
                let (Some(pos), Some(size)) = (pt(c, "at"), pt(c, "size")) else { continue };
                let name = c.property("Sheetname").or_else(|| c.property("Sheet name")).unwrap_or("Blatt").to_string();
                let file = c.property("Sheetfile").or_else(|| c.property("Sheet file")).unwrap_or("").to_string();
                let pins = c.all("pin").filter_map(|q| Some(SheetPin { name: unescape(q.at(1)?), pos: pt(q, "at")? })).collect();
                sch.sheets.push(Sheet { name, pos, size, pins, sch: Schematic::default() });
                files.push((file, c.val("uuid").unwrap_or("").to_string()));
            }
            _ => {}
        }
    }
    Ok((sch, files, instances, root_uuid))
}

/// A schematic from text; sub-sheets stay empty (see [`read_sch_file`]).
pub fn read_sch(src: &str) -> Result<Schematic, String> {
    read_sch_text(src).map(|x| x.0)
}

/// A schematic file with all its sub-sheet files (looked up next to it).
pub fn read_sch_file(path: &std::path::Path) -> Result<Schematic, String> {
    // `inst`: the instance path of this sheet ("/root" or "/root/sheet/…"); a sheet file
    // used several times gets the references of each use from the symbols' instance lists
    fn go(path: &std::path::Path, depth: usize, inst: Option<String>) -> Result<Schematic, String> {
        if depth > 16 {
            return Err("Blatthierarchie zu tief (Zyklus?)".into());
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        let (mut sch, files, instances, root_uuid) = read_sch_text_full(&text)?;
        let here = inst.unwrap_or_else(|| format!("/{}", root_uuid));
        for (s, list) in sch.symbols.iter_mut().zip(&instances) {
            if let Some((_, r, u)) = list.iter().find(|(p, _, _)| *p == here) {
                s.reference = r.clone();
                s.unit = (*u).max(1);
            }
        }
        let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        for (sh, (f, uuid)) in sch.sheets.iter_mut().zip(files) {
            if f.is_empty() {
                continue;
            }
            sh.sch = go(&dir.join(&f), depth + 1, Some(format!("{}/{}", here, uuid)))?;
        }
        Ok(sch)
    }
    go(path, 0, None)
}

// ------------------------------------------------------------------ board out

fn layer_list(layers: u8) -> Sx {
    let mut v = vec![l("0", vec![s("F.Cu"), a("signal")])];
    if layers >= 4 {
        v.push(l("1", vec![s("In1.Cu"), a("signal")]));
        v.push(l("2", vec![s("In2.Cu"), a("signal")]));
    }
    v.push(l("31", vec![s("B.Cu"), a("signal")]));
    for (i, name, user) in [
        (34, "B.Paste", ""),
        (35, "F.Paste", ""),
        (36, "B.SilkS", "B.Silkscreen"),
        (37, "F.SilkS", "F.Silkscreen"),
        (38, "B.Mask", ""),
        (39, "F.Mask", ""),
        (44, "Edge.Cuts", ""),
        (46, "B.CrtYd", "B.Courtyard"),
        (47, "F.CrtYd", "F.Courtyard"),
        (48, "B.Fab", ""),
        (49, "F.Fab", ""),
    ] {
        let mut e = vec![s(name), a("user")];
        if !user.is_empty() {
            e.push(s(user));
        }
        v.push(l(&i.to_string(), e));
    }
    l("layers", v)
}

fn pad_sx(f: &FpInst, pad: &Pad, net: &str, nets: &HashMap<String, usize>, layers: u8, ids: &mut Ids) -> Sx {
    let kind = if pad.npth {
        "np_thru_hole"
    } else if pad.smd() {
        "smd"
    } else {
        "thru_hole"
    };
    let shape = match pad.shape {
        PadShape::Circle => "circle",
        PadShape::Rect => "rect",
        PadShape::Oval => "oval",
        PadShape::RoundRect(_) => "roundrect",
    };
    let local = if f.bottom { p(-pad.pos.x, pad.pos.y) } else { pad.pos };
    let side = if f.bottom { "B" } else { "F" };
    let lay: Vec<Sx> = if pad.smd() {
        vec![s(&format!("{}.Cu", side)), s(&format!("{}.Paste", side)), s(&format!("{}.Mask", side))]
    } else {
        let _ = layers;
        vec![s("*.Cu"), s("*.Mask")]
    };
    let mut v = vec![s(&pad.num), a(kind), a(shape), at(local, f.rot + pad.rot), l("size", vec![n(pad.size.x), n(pad.size.y)])];
    if pad.drill > 0.0 && pad.slot > pad.drill {
        let (sx, sy) = if pad.size.x >= pad.size.y { (pad.slot, pad.drill) } else { (pad.drill, pad.slot) };
        let mut d = vec![a("oval"), n(sx), n(sy)];
        if pad.drill_off != P::ZERO {
            d.push(l("offset", vec![n(pad.drill_off.x), n(pad.drill_off.y)]));
        }
        v.push(l("drill", d));
    } else if pad.drill > 0.0 {
        let mut d = vec![n(pad.drill)];
        if pad.drill_off != P::ZERO {
            d.push(l("offset", vec![n(pad.drill_off.x), n(pad.drill_off.y)]));
        }
        v.push(l("drill", d));
    }
    v.push(l("layers", lay));
    if let PadShape::RoundRect(k) = pad.shape {
        v.push(l("roundrect_rratio", vec![n(k)]));
    }
    if !net.is_empty() {
        v.push(l("net", vec![n(nets[net] as f64), s(net)]));
    }
    v.push(ids.next());
    l("pad", v)
}

/// One footprint; `lib` writes the library form (.kicad_mod: no position, no nets).
fn fp_sx(f: &FpInst, nets: &HashMap<String, usize>, layers: u8, ids: &mut Ids, lib: bool) -> Sx {
    let side = if f.bottom { "B" } else { "F" };
    let mut fv = vec![s(if lib { f.fp.short() } else { &f.fp.id }), l("layer", vec![s(&format!("{}.Cu", side))])];
    if !lib {
        fv.push(ids.next());
        fv.push(at(f.pos, f.rot));
    }
    let (lo, hi) = f.fp.court;
    let m = |q: P| if f.bottom { p(-q.x, q.y) } else { q };
    fv.push(l("property", vec![s("Reference"), s(&f.reference), at(m(p((lo.x + hi.x) / 2.0, lo.y - 0.8)), f.rot), l("layer", vec![s(&format!("{}.SilkS", side))]), ids.next(), font(1.0)]));
    fv.push(l("property", vec![s("Value"), s(&f.value), at(m(p((lo.x + hi.x) / 2.0, hi.y + 0.8)), f.rot), l("layer", vec![s(&format!("{}.Fab", side))]), ids.next(), font(1.0)]));
    fv.push(l("property", vec![s("Footprint"), s(&f.fp.id), at(P::ZERO, f.rot), l("layer", vec![s(&format!("{}.Fab", side))]), ids.next(), font_hide(1.0)]));
    fv.push(l("descr", vec![s(&f.fp.description)]));
    if f.fp.smd() {
        fv.push(l("attr", vec![a("smd")]));
    } else {
        fv.push(l("attr", vec![a("through_hole")]));
    }
    for (a0, b0) in &f.fp.silk {
        fv.push(l("fp_line", vec![l("start", vec![n(m(*a0).x), n(a0.y)]), l("end", vec![n(m(*b0).x), n(b0.y)]), l("stroke", vec![l("width", vec![n(0.12)]), l("type", vec![a("solid")])]), l("layer", vec![s(&format!("{}.SilkS", side))]), ids.next()]));
    }
    let (cl, ch) = (m(lo), m(hi));
    fv.push(l(
        "fp_rect",
        vec![l("start", vec![n(cl.x.min(ch.x)), n(lo.y)]), l("end", vec![n(cl.x.max(ch.x)), n(hi.y)]), l("stroke", vec![l("width", vec![n(0.05)]), l("type", vec![a("solid")])]), l("fill", vec![a("none")]), l("layer", vec![s(&format!("{}.CrtYd", side))]), ids.next()],
    ));
    for (k, pad) in f.fp.pads.iter().enumerate() {
        fv.push(pad_sx(f, pad, f.nets.get(k).map(|x| x.as_str()).unwrap_or(""), nets, layers, ids));
    }
    if let Some(m) = &f.fp.model {
        fv.push(l("model", vec![s(m), l("offset", vec![l("xyz", vec![n(0.0), n(0.0), n(0.0)])]), l("scale", vec![l("xyz", vec![n(1.0), n(1.0), n(1.0)])]), l("rotate", vec![l("xyz", vec![n(0.0), n(0.0), n(0.0)])])]));
    }
    l("footprint", fv)
}

pub fn write_pcb(b: &Board, project: &str) -> String {
    let mut ids = Ids(1000);
    let names = b.nets();
    let mut nets: HashMap<String, usize> = HashMap::new();
    let mut v = vec![
        l("version", vec![a("20240108")]),
        l("generator", vec![s("freo")]),
        l("generator_version", vec![s("1.2")]),
        l("general", vec![l("thickness", vec![n(b.thickness)]), l("legacy_teardrops", vec![a("no")])]),
        l("paper", vec![s("A4")]),
        l("title_block", vec![l("title", vec![s(project)])]),
        layer_list(b.layers),
        l("setup", vec![l("pad_to_mask_clearance", vec![n(super::gerber::MASK_MARGIN)]), l("allow_soldermask_bridges_in_footprints", vec![a("no")])]),
        l("net", vec![n(0.0), s("")]),
    ];
    for (i, name) in names.iter().enumerate() {
        nets.insert(name.clone(), i + 1);
        v.push(l("net", vec![n((i + 1) as f64), s(name)]));
    }
    for f in &b.footprints {
        v.push(fp_sx(f, &nets, b.layers, &mut ids, false));
    }
    for lp in &b.outline {
        for i in 0..lp.len() {
            v.push(l("gr_line", vec![l("start", vec![n(lp[i].x), n(lp[i].y)]), l("end", vec![n(lp[(i + 1) % lp.len()].x), n(lp[(i + 1) % lp.len()].y)]), stroke(0.1), l("layer", vec![s("Edge.Cuts")]), ids.next()]));
        }
    }
    for (q, t, h) in &b.texts {
        v.push(l("gr_text", vec![s(t), at(*q, 0.0), l("layer", vec![s("F.SilkS")]), ids.next(), l("effects", vec![l("font", vec![l("size", vec![n(*h), n(*h)]), l("thickness", vec![n(h * 0.15)])])])]));
    }
    for t in &b.tracks {
        v.push(l("segment", vec![l("start", vec![n(t.a.x), n(t.a.y)]), l("end", vec![n(t.b.x), n(t.b.y)]), l("width", vec![n(t.width)]), l("layer", vec![s(layer_name(t.layer))]), l("net", vec![n(*nets.get(&t.net).unwrap_or(&0) as f64)]), ids.next()]));
    }
    for via in &b.vias {
        v.push(l("via", vec![at2(via.pos), l("size", vec![n(via.dia)]), l("drill", vec![n(via.drill)]), l("layers", vec![s("F.Cu"), s("B.Cu")]), l("net", vec![n(*nets.get(&via.net).unwrap_or(&0) as f64)]), ids.next()]));
    }
    for z in &b.zones {
        let mut zv = vec![
            l("net", vec![n(*nets.get(&z.net).unwrap_or(&0) as f64)]),
            l("net_name", vec![s(&z.net)]),
            l("layer", vec![s(layer_name(z.layer))]),
            ids.next(),
            l("hatch", vec![a("edge"), n(0.5)]),
            l("connect_pads", if z.thermal { vec![l("clearance", vec![n(z.clearance)])] } else { vec![a("yes"), l("clearance", vec![n(z.clearance)])] }),
            l("min_thickness", vec![n(z.min_width.max(0.1))]),
            l("filled_areas_thickness", vec![a("no")]),
            l("fill", vec![a("yes"), l("thermal_gap", vec![n(0.5)]), l("thermal_bridge_width", vec![n(0.5)])]),
            l("polygon", vec![l("pts", z.outline.iter().map(|q| xy(*q)).collect())]),
        ];
        for shape in &z.fill {
            for c in shape.iter().take(1) {
                zv.push(l("filled_polygon", vec![l("layer", vec![s(layer_name(z.layer))]), l("pts", c.iter().map(|q| xy(p(q[0], q[1]))).collect())]));
            }
        }
        v.push(l("zone", zv));
    }
    super::sexpr::write(&l("kicad_pcb", v))
}

// ------------------------------------------------------------------ board in

fn xyz(c: Option<&Sx>) -> [f64; 3] {
    let Some(x) = c.and_then(|c| c.get("xyz")) else { return [0.0; 3] };
    [x.atn(1).unwrap_or(0.0), x.atn(2).unwrap_or(0.0), x.atn(3).unwrap_or(0.0)]
}

fn read_footprint(c: &Sx, netname: &dyn Fn(&Sx) -> String) -> Option<FpInst> {
    let id = c.at(1)?.to_string();
    let atx = c.get("at");
    let pos = atx.and_then(|x| Some(p(x.atn(1)?, x.atn(2)?))).unwrap_or(P::ZERO);
    let rot = atx.and_then(|x| x.atn(3)).unwrap_or(0.0);
    let bottom = c.val("layer").is_some_and(|x| x.starts_with("B."));
    let unm = |q: P| if bottom { p(-q.x, q.y) } else { q };
    let lib = footprints::find(&id);
    let mut fp = Footprint { id: id.clone(), description: c.val("descr").unwrap_or("").to_string(), pads: vec![], silk: vec![], court: (P::ZERO, P::ZERO), body: vec![], port: lib.and_then(|f| f.port.clone()), model: c.get("model").and_then(|m| m.at(1)).map(|m| m.to_string()).or_else(|| lib.and_then(|f| f.model.clone())), model_offset: xyz(c.get("model").and_then(|m| m.get("offset"))), model_rot: xyz(c.get("model").and_then(|m| m.get("rotate"))) };
    let mut nets = vec![];
    let mut court = vec![];
    for g in c.items() {
        match g.head() {
            Some("pad") => {
                let pa = g.get("at")?;
                let local = unm(p(pa.atn(1)?, pa.atn(2)?));
                let prot = pa.atn(3).unwrap_or(0.0) - rot;
                let size = g.get("size").and_then(|sz| Some(p(sz.atn(1)?, sz.atn(2)?))).unwrap_or(p(1.0, 1.0));
                let kind = g.at(2).unwrap_or("smd");
                let drill = g.get("drill").map(|d| d.items().iter().skip(1).filter_map(|x| x.num()).fold(f64::MAX, f64::min)).filter(|d| *d < f64::MAX).unwrap_or(0.0);
                let drill_off = g.get("drill").and_then(|d| d.get("offset")).map(|o| p(o.atn(1).unwrap_or(0.0), o.atn(2).unwrap_or(0.0))).unwrap_or(P::ZERO);
                let slot = g.get("drill").filter(|d| d.at(1) == Some("oval")).map(|d| d.items().iter().skip(2).filter_map(|x| x.num()).fold(0.0, f64::max)).filter(|s| *s > drill + 1e-6).unwrap_or(0.0);
                let shape = match g.at(3) {
                    Some("circle") => PadShape::Circle,
                    Some("oval") => PadShape::Oval,
                    Some("roundrect") => PadShape::RoundRect(g.valn("roundrect_rratio").unwrap_or(0.25)),
                    _ => PadShape::Rect,
                };
                fp.pads.push(Pad { num: g.at(1).unwrap_or("").to_string(), pos: local, size, shape, drill: if kind == "smd" || kind == "connect" { 0.0 } else { drill.max(0.01) }, rot: (prot.rem_euclid(360.0) * 1e6).round() / 1e6, npth: kind == "np_thru_hole", slot, drill_off });
                nets.push(g.get("net").map(netname).unwrap_or_default());
            }
            Some(h @ ("fp_line" | "fp_rect" | "fp_circle" | "fp_poly" | "fp_arc")) => {
                let layer = g.val("layer").unwrap_or("");
                let pts: Vec<P> = match h {
                    "fp_line" | "fp_rect" => [pt(g, "start"), pt(g, "end")].into_iter().flatten().map(unm).collect(),
                    "fp_circle" => {
                        let (Some(cc), Some(e)) = (pt(g, "center"), pt(g, "end")) else { continue };
                        let r = (e - cc).length();
                        (0..=24).map(|i| unm(cc + p((i as f64 / 24.0 * std::f64::consts::TAU).cos(), (i as f64 / 24.0 * std::f64::consts::TAU).sin()) * r)).collect()
                    }
                    "fp_arc" => match (pt(g, "start"), pt(g, "mid"), pt(g, "end")) {
                        (Some(a0), Some(m), Some(b0)) => arc3(a0, m, b0, 36).into_iter().map(unm).collect(),
                        _ => continue,
                    },
                    _ => g.get("pts").map(|k| k.all("xy").filter_map(|q| Some(unm(p(q.atn(1)?, q.atn(2)?)))).collect()).unwrap_or_default(),
                };
                if layer.ends_with("SilkS") {
                    if h == "fp_rect" && pts.len() == 2 {
                        let (lo, hi) = (pts[0].min(pts[1]), pts[0].max(pts[1]));
                        let c4 = [lo, p(hi.x, lo.y), hi, p(lo.x, hi.y)];
                        for i in 0..4 {
                            fp.silk.push((c4[i], c4[(i + 1) % 4]));
                        }
                    } else {
                        for w in pts.windows(2) {
                            fp.silk.push((w[0], w[1]));
                        }
                    }
                } else if layer.ends_with("CrtYd") {
                    court.extend(pts);
                }
            }
            _ => {}
        }
    }
    let pad_box = super::geom::bbox(fp.pads.iter().flat_map(|q| {
        let (lo, hi) = q.hull(P::ZERO, 0.0, false).bbox();
        [lo, hi]
    }));
    fp.court = super::geom::bbox(court).or(pad_box.map(|(lo, hi)| (lo - P::splat(0.25), hi + P::splat(0.25)))).unwrap_or((p(-1.0, -1.0), p(1.0, 1.0)));
    fp.body = match lib {
        Some(lf) if lf.pads.len() == fp.pads.len() => lf.body.clone(),
        _ => {
            let (lo, hi) = fp.court;
            vec![Body3::Box { lo: lo + P::splat(0.25), hi: hi - P::splat(0.25), z0: 0.0, z1: if fp.pads.iter().all(|q| q.smd()) { 1.0 } else { 3.0 }, color: [40, 40, 44] }]
        }
    };
    Some(FpInst { reference: c.property("Reference").or_else(|| c.all("fp_text").find(|t| t.at(1) == Some("reference")).and_then(|t| t.at(2))).unwrap_or("?").to_string(), value: c.property("Value").unwrap_or("").to_string(), fp, pos, rot, bottom, nets, locked: c.get("locked").is_some() })
}

/// Chains Edge.Cuts segments into closed loops.
fn chain(mut segs: Vec<Vec<P>>) -> Vec<Vec<P>> {
    let mut loops = vec![];
    while let Some(mut cur) = segs.pop() {
        loop {
            let end = *cur.last().unwrap();
            if cur.len() > 2 && (end - cur[0]).length() < 1e-3 {
                cur.pop();
                break;
            }
            let Some(k) = segs.iter().position(|s| (s[0] - end).length() < 1e-3 || (s[s.len() - 1] - end).length() < 1e-3) else { break };
            let mut s = segs.swap_remove(k);
            if (s[0] - end).length() >= 1e-3 {
                s.reverse();
            }
            cur.extend(s.into_iter().skip(1));
        }
        if cur.len() >= 3 {
            loops.push(cur);
        }
    }
    loops
}

pub fn read_pcb(src: &str) -> Result<Board, String> {
    let root = super::sexpr::parse(src)?;
    if root.head() != Some("kicad_pcb") {
        return Err("Keine KiCad-Platinendatei (.kicad_pcb)".into());
    }
    let mut table: HashMap<String, String> = HashMap::new();
    for nsx in root.all("net") {
        if let (Some(i), Some(name)) = (nsx.at(1), nsx.at(2)) {
            table.insert(i.to_string(), name.to_string());
        }
    }
    let netname = |c: &Sx| -> String {
        match (c.at(1), c.at(2)) {
            (_, Some(name)) => unescape(name),
            (Some(id), None) => table.get(id).map(|x| unescape(x)).unwrap_or_else(|| if id.parse::<i64>().is_ok() { String::new() } else { unescape(id) }),
            _ => String::new(),
        }
    };
    let mut b = Board { thickness: root.get("general").and_then(|g| g.valn("thickness")).unwrap_or(1.6), ..Default::default() };
    let copper_layers = root.get("layers").map(|ls| ls.items().iter().skip(1).filter(|x| x.at(1).is_some_and(|n| n.ends_with(".Cu"))).count()).unwrap_or(2);
    b.layers = if copper_layers >= 4 { 4 } else { 2 };
    let mut edges: Vec<Vec<P>> = vec![];
    for c in root.items() {
        match c.head() {
            Some("footprint") | Some("module") => {
                if let Some(f) = read_footprint(c, &netname) {
                    b.footprints.push(f);
                }
            }
            Some(h @ ("gr_line" | "gr_rect" | "gr_arc" | "gr_circle" | "gr_poly")) if c.val("layer") == Some("Edge.Cuts") => match h {
                "gr_line" => {
                    if let (Some(a0), Some(b0)) = (pt(c, "start"), pt(c, "end")) {
                        edges.push(vec![a0, b0]);
                    }
                }
                "gr_rect" => {
                    if let (Some(a0), Some(b0)) = (pt(c, "start"), pt(c, "end")) {
                        b.outline.push(vec![a0, p(b0.x, a0.y), b0, p(a0.x, b0.y)]);
                    }
                }
                "gr_arc" => {
                    if let (Some(a0), Some(m), Some(b0)) = (pt(c, "start"), pt(c, "mid"), pt(c, "end")) {
                        edges.push(arc3(a0, m, b0, 72));
                    }
                }
                "gr_circle" => {
                    if let (Some(cc), Some(e)) = (pt(c, "center"), pt(c, "end")) {
                        let r = (e - cc).length();
                        b.outline.push((0..72).map(|i| cc + p((i as f64 / 72.0 * std::f64::consts::TAU).cos(), (i as f64 / 72.0 * std::f64::consts::TAU).sin()) * r).collect());
                    }
                }
                _ => {
                    let pts: Vec<P> = c.get("pts").map(|k| k.all("xy").filter_map(|q| Some(p(q.atn(1)?, q.atn(2)?))).collect()).unwrap_or_default();
                    if pts.len() >= 3 {
                        b.outline.push(pts);
                    }
                }
            },
            Some("gr_text") if c.val("layer") == Some("F.SilkS") => {
                if let (Some(t), Some(q)) = (c.at(1), pt(c, "at")) {
                    let h = c.get("effects").and_then(|e| e.get("font")).and_then(|f| f.get("size")).and_then(|s| s.atn(1)).unwrap_or(1.0);
                    b.texts.push((q, t.to_string(), h));
                }
            }
            Some("segment") => {
                if let (Some(a0), Some(b0)) = (pt(c, "start"), pt(c, "end")) {
                    let Some(layer) = c.val("layer").and_then(layer_from_name) else { continue };
                    b.tracks.push(Track { a: a0, b: b0, width: c.valn("width").unwrap_or(0.25), layer, net: c.get("net").map(netname).unwrap_or_default() });
                }
            }
            Some("arc") => {
                if let (Some(a0), Some(m), Some(b0)) = (pt(c, "start"), pt(c, "mid"), pt(c, "end")) {
                    let Some(layer) = c.val("layer").and_then(layer_from_name) else { continue };
                    let net = c.get("net").map(netname).unwrap_or_default();
                    let w = c.valn("width").unwrap_or(0.25);
                    for s2 in arc3(a0, m, b0, 72).windows(2) {
                        b.tracks.push(Track { a: s2[0], b: s2[1], width: w, layer, net: net.clone() });
                    }
                }
            }
            Some("via") => {
                if let Some(q) = pt(c, "at") {
                    b.vias.push(Via { pos: q, dia: c.valn("size").unwrap_or(0.8), drill: c.valn("drill").unwrap_or(0.4), net: c.get("net").map(netname).unwrap_or_default() });
                }
            }
            Some("zone") => {
                let layer = c.val("layer").or_else(|| c.get("layers").and_then(|x| x.at(1))).and_then(layer_from_name);
                let outline: Vec<P> = c.get("polygon").and_then(|pg| pg.get("pts")).map(|k| k.all("xy").filter_map(|q| Some(p(q.atn(1)?, q.atn(2)?))).collect()).unwrap_or_default();
                if let (Some(layer), true) = (layer, outline.len() >= 3) {
                    let net = c.val("net_name").map(|x| x.to_string()).unwrap_or_else(|| c.get("net").map(netname).unwrap_or_default());
                    let cp = c.get("connect_pads");
                    let teardrop = c.get("attr").is_some_and(|a| a.get("teardrop").is_some());
                    let fill: Vec<Vec<Vec<[f64; 2]>>> = if teardrop {
                        c.all("filled_polygon").map(|fp| vec![fp.get("pts").map(|k| k.all("xy").filter_map(|q| Some([q.atn(1)?, q.atn(2)?])).collect::<Vec<_>>()).unwrap_or_default()]).filter(|s: &Vec<Vec<[f64; 2]>>| s[0].len() >= 3).collect()
                    } else {
                        vec![]
                    };
                    b.zones.push(Zone {
                        fixed: teardrop,
                        net,
                        layer,
                        outline,
                        clearance: cp.and_then(|x| x.valn("clearance")).unwrap_or(0.2),
                        min_width: c.valn("min_thickness").unwrap_or(0.25),
                        thermal: !cp.is_some_and(|x| x.at(1) == Some("yes")),
                        fill,
                    });
                }
            }
            _ => {}
        }
    }
    b.outline.extend(chain(edges));
    Ok(b)
}

/// A footprint from a KiCad library file (.kicad_mod); `lib` is the library name (the
/// .pretty folder without extension) for the id "<lib>:<name>".
pub fn read_footprint_file(src: &str, lib: &str) -> Result<Footprint, String> {
    let root = super::sexpr::parse(src)?;
    if !matches!(root.head(), Some("footprint") | Some("module")) {
        return Err("Keine KiCad-Footprintdatei (.kicad_mod)".into());
    }
    let mut f = read_footprint(&root, &|_| String::new()).ok_or("Footprint unvollständig")?;
    if !f.fp.id.contains(':') && !lib.is_empty() {
        f.fp.id = format!("{}:{}", lib, f.fp.id);
    }
    Ok(f.fp)
}

/// The library form of a footprint (.kicad_mod).
pub fn write_footprint_file(fp: &Footprint) -> String {
    let f = FpInst { reference: "REF**".into(), value: fp.short().into(), fp: fp.clone(), pos: P::ZERO, rot: 0.0, bottom: false, nets: vec![String::new(); fp.pads.len()], locked: false };
    let mut ids = Ids(5000);
    let mut sx = fp_sx(&f, &HashMap::new(), 2, &mut ids, true);
    if let Sx::List(v) = &mut sx {
        v.insert(2, l("version", vec![a("20240108")]));
        v.insert(3, l("generator", vec![s("freo")]));
    }
    super::sexpr::write(&sx)
}

/// All footprints of a .pretty folder.
pub fn read_pretty_dir(dir: &std::path::Path) -> Result<Vec<Footprint>, String> {
    let lib = dir.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
    let mut out = vec![];
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "kicad_mod")).collect();
    entries.sort();
    for p in entries {
        let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
        out.push(read_footprint_file(&t, &lib).map_err(|e| format!("{}: {}", p.display(), e))?);
    }
    Ok(out)
}

/// KiCad project file (.kicad_pro, JSON) with the design rules and net classes.
pub fn write_pro(b: &Board, stem: &str) -> String {
    let r = &b.rules;
    let class = |name: &str, clr: f64, tw: f64, vd: f64, vdr: f64, dw: f64, dg: f64| {
        serde_json::json!({ "name": name, "clearance": clr, "track_width": tw, "via_diameter": vd, "via_drill": vdr, "diff_pair_width": dw, "diff_pair_gap": dg, "microvia_diameter": 0.3, "microvia_drill": 0.1, "wire_width": 6, "bus_width": 12, "line_style": 0, "pcb_color": "rgba(0, 0, 0, 0.000)", "schematic_color": "rgba(0, 0, 0, 0.000)", "priority": 2147483647 })
    };
    let mut classes = vec![class("Default", r.clearance, r.track, r.via_dia, r.via_drill, r.track, r.clearance)];
    let mut patterns = vec![];
    for c in &r.classes {
        classes.push(class(&c.name, c.clearance, c.track, c.via_dia, c.via_drill, c.diff_width, c.diff_gap));
        for pat in &c.patterns {
            patterns.push(serde_json::json!({ "netclass": c.name, "pattern": pat }));
        }
    }
    let v = serde_json::json!({
        "board": { "design_settings": { "rules": {
            "min_clearance": r.clearance, "min_track_width": r.min_track, "min_through_hole_diameter": r.min_drill,
            "min_via_annular_width": r.min_annular, "min_copper_edge_clearance": r.edge_clearance, "min_hole_clearance": r.hole_clearance
        } } },
        "meta": { "filename": format!("{}.kicad_pro", stem), "version": 1 },
        "net_settings": { "classes": classes, "meta": { "version": 3 }, "netclass_patterns": patterns },
        "schematic": {}, "sheets": [], "text_variables": {}
    });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}

/// Design rules and net classes from a .kicad_pro file (missing values keep `base`).
pub fn read_pro(src: &str, base: &super::board::Rules) -> super::board::Rules {
    let mut r = base.clone();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(src) else { return r };
    let f = |x: &serde_json::Value, k: &str, d: f64| x.get(k).and_then(|y| y.as_f64()).unwrap_or(d);
    if let Some(rules) = v.pointer("/board/design_settings/rules") {
        r.clearance = f(rules, "min_clearance", r.clearance);
        r.min_track = f(rules, "min_track_width", r.min_track);
        r.min_drill = f(rules, "min_through_hole_diameter", r.min_drill);
        r.min_annular = f(rules, "min_via_annular_width", r.min_annular);
        r.edge_clearance = f(rules, "min_copper_edge_clearance", r.edge_clearance);
        r.hole_clearance = f(rules, "min_hole_clearance", r.hole_clearance);
    }
    if let Some(cls) = v.pointer("/net_settings/classes").and_then(|x| x.as_array()) {
        let pats: Vec<(String, String)> = v
            .pointer("/net_settings/netclass_patterns")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|p| Some((p.get("netclass")?.as_str()?.to_string(), p.get("pattern")?.as_str()?.to_string()))).collect())
            .unwrap_or_default();
        let mut out = vec![];
        for c in cls {
            let name = c.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if name == "Default" {
                r.clearance = f(c, "clearance", r.clearance);
                r.track = f(c, "track_width", r.track);
                r.via_dia = f(c, "via_diameter", r.via_dia);
                r.via_drill = f(c, "via_drill", r.via_drill);
                continue;
            }
            // older files list the nets of a class directly
            let mut patterns: Vec<String> = pats.iter().filter(|(n, _)| *n == name).map(|(_, p)| p.clone()).collect();
            if let Some(nets) = c.get("nets").and_then(|x| x.as_array()) {
                patterns.extend(nets.iter().filter_map(|n| n.as_str().map(|s| s.to_string())));
            }
            out.push(super::board::NetClass {
                name,
                patterns,
                clearance: f(c, "clearance", r.clearance),
                track: f(c, "track_width", r.track),
                via_dia: f(c, "via_diameter", r.via_dia),
                via_drill: f(c, "via_drill", r.via_drill),
                diff_width: f(c, "diff_pair_width", r.track),
                diff_gap: f(c, "diff_pair_gap", r.clearance),
            });
        }
        r.classes = out;
    }
    r
}

/// All files of a KiCad project: schematic sheets, board and project file.
pub fn write_project(pr: &super::Project, stem: &str) -> Vec<(String, String)> {
    let mut v = write_sch_files(&pr.schematic, stem);
    v.push((format!("{}.kicad_pcb", stem), write_pcb(&pr.board, stem)));
    v.push((format!("{}.kicad_pro", stem), write_pro(&pr.board, stem)));
    v
}

/// Reads a KiCad project (any of its files names it): schematic tree, board and rules.
pub fn read_project(path: &std::path::Path) -> Result<(super::Project, Vec<&'static str>), String> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let dir = path.parent().map(|x| x.to_path_buf()).unwrap_or_default();
    let mut pr = super::Project::new(&stem);
    let mut got = vec![];
    let sch = dir.join(format!("{}.kicad_sch", stem));
    if sch.exists() {
        pr.schematic = read_sch_file(&sch)?;
        got.push("Schaltplan");
    }
    let brd = dir.join(format!("{}.kicad_pcb", stem));
    if brd.exists() {
        let t = std::fs::read_to_string(&brd).map_err(|e| e.to_string())?;
        pr.board = read_pcb(&t).map_err(|e| format!("{}: {}", brd.display(), e))?;
        got.push("Platine");
    }
    let pro = dir.join(format!("{}.kicad_pro", stem));
    if let Ok(t) = std::fs::read_to_string(&pro) {
        pr.board.rules = read_pro(&t, &pr.board.rules);
    }
    if got.is_empty() {
        return Err(format!("Keine KiCad-Dateien zu {} gefunden", stem));
    }
    pr.board.fill_zones();
    Ok((pr, got))
}
