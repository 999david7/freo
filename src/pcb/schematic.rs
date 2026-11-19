//! Schematic: placed symbols, wires, net labels, no-connect flags; netlist extraction,
//! annotation and the electrical rules check (ERC).

use super::geom::{p, pt_seg, rot, P};
use super::symbols::{self, PinType, Symbol};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymInst {
    pub lib: String,
    pub reference: String,
    pub value: String,
    pub footprint: String,
    pub pos: P,
    /// 0, 90, 180 or 270 degrees, counter-clockwise on screen
    pub rot: f64,
    /// mirrored left-right
    #[serde(default)]
    pub mirror: bool,
    /// free fields (Datenblatt, Hersteller, …)
    #[serde(default)]
    pub fields: Vec<(String, String)>,
    /// unit (gate) of a multi-unit part, 1 = A
    #[serde(default = "one")]
    pub unit: u32,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub a: P,
    pub b: P,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelKind {
    Local,
    Global,
    /// hierarchical label: connects to the pin of the same name on the parent's sheet symbol
    Hier,
}

/// A pin on a sheet symbol, joined to the hierarchical label of the same name inside.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetPin {
    pub name: String,
    pub pos: P,
}

/// Hierarchical sheet: a box on the parent with pins, holding a sub-schematic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    pub pos: P,
    pub size: P,
    pub pins: Vec<SheetPin>,
    pub sch: Schematic,
}

impl Sheet {
    pub fn contains(&self, q: P) -> bool {
        q.x >= self.pos.x && q.y >= self.pos.y && q.x <= self.pos.x + self.size.x && q.y <= self.pos.y + self.size.y
    }
    /// Nearest point on the sheet border (sheet pins sit there), on the 1,27 mm grid.
    pub fn border_point(&self, q: P) -> P {
        let (lo, hi) = (self.pos, self.pos + self.size);
        let g = |v: f64| (v / 1.27).round() * 1.27;
        let c = p(g(q.x), g(q.y)).clamp(lo, hi);
        let d = [(c.x - lo.x).abs(), (hi.x - c.x).abs(), (c.y - lo.y).abs(), (hi.y - c.y).abs()];
        let k = (0..4).min_by(|a, b| d[*a].partial_cmp(&d[*b]).unwrap()).unwrap();
        match k {
            0 => p(lo.x, c.y),
            1 => p(hi.x, c.y),
            2 => p(c.x, lo.y),
            _ => p(c.x, hi.y),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub pos: P,
    pub text: String,
    pub kind: LabelKind,
    #[serde(default)]
    pub rot: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Schematic {
    pub title: String,
    pub symbols: Vec<SymInst>,
    pub wires: Vec<Wire>,
    pub labels: Vec<Label>,
    pub no_connects: Vec<P>,
    /// symbols that are not in the built-in library (imported from KiCad files)
    #[serde(default)]
    pub local_lib: Vec<Symbol>,
    /// bus lines (graphic only: members connect through labels of the same name)
    #[serde(default)]
    pub buses: Vec<Wire>,
    /// bus entries: short wires from a bus to a member wire
    #[serde(default)]
    pub bus_entries: Vec<Wire>,
    /// hierarchical sub-sheets
    #[serde(default)]
    pub sheets: Vec<Sheet>,
}

/// Expands a bus label like "D[0..7]" to its members (D0 … D7); other labels give None.
pub fn bus_members(label: &str) -> Option<Vec<String>> {
    let (stem, rest) = label.split_once('[')?;
    let inner = rest.strip_suffix(']')?;
    let (a, b) = inner.split_once("..")?;
    let (a, b): (i64, i64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    let (lo, hi) = (a.min(b), a.max(b));
    if hi - lo > 255 {
        return None;
    }
    Some((lo..=hi).map(|k| format!("{}{}", stem, k)).collect())
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetPin {
    pub sym: usize,
    pub reference: String,
    pub pin: String,
    pub pin_name: String,
    pub kind: PinType,
    /// pin of a power symbol or PWR_FLAG (not a part)
    pub power_sym: bool,
    pub pos: P,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub name: String,
    pub pins: Vec<NetPin>,
    pub labels: Vec<usize>,
}

impl Net {
    pub fn part_pins(&self) -> impl Iterator<Item = &NetPin> {
        self.pins.iter().filter(|q| !q.power_sym)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Netlist {
    pub nets: Vec<Net>,
}

impl Netlist {
    /// Net of a part pin.
    pub fn net_of(&self, reference: &str, pin: &str) -> Option<&str> {
        self.nets.iter().find(|n| n.pins.iter().any(|q| !q.power_sym && q.reference == reference && q.pin == pin)).map(|n| n.name.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ErcItem {
    pub severity: Severity,
    pub msg: String,
    pub pos: P,
}

struct Uf(Vec<usize>);
impl Uf {
    fn new(n: usize) -> Uf {
        Uf((0..n).collect())
    }
    fn find(&mut self, i: usize) -> usize {
        let mut r = i;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut k = i;
        while self.0[k] != r {
            let n = self.0[k];
            self.0[k] = r;
            k = n;
        }
        r
    }
    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

const EPS: f64 = 1e-4;

#[derive(Clone, Copy, PartialEq)]
enum Node {
    WireEnd(usize),
    Pin(usize, usize),
    Label(usize),
    Nc(usize),
}

impl Schematic {
    pub fn def(&self, lib: &str) -> Option<&Symbol> {
        self.local_lib.iter().find(|s| s.id == lib).or_else(|| symbols::find(lib))
    }

    /// World position of a point in symbol coordinates.
    pub fn xform(inst: &SymInst, q: P) -> P {
        let q = if inst.mirror { p(-q.x, q.y) } else { q };
        inst.pos + rot(q, inst.rot)
    }

    /// Direction (degrees) of a symbol-space direction after placement.
    pub fn xform_dir(inst: &SymInst, dir: f64) -> f64 {
        let d = if inst.mirror { 180.0 - dir } else { dir };
        (d + inst.rot).rem_euclid(360.0)
    }

    /// Pins of the placed unit with their sheet positions.
    pub fn pin_positions(&self, i: usize) -> Vec<(P, &super::symbols::Pin)> {
        let s = &self.symbols[i];
        match self.def(&s.lib) {
            Some(d) => d.pins_of(s.unit).map(|q| (Self::xform(s, q.pos), q)).collect(),
            None => vec![],
        }
    }

    /// Reference as shown: with the unit letter for multi-unit parts (U1A).
    pub fn shown_ref(&self, i: usize) -> String {
        let s = &self.symbols[i];
        match self.def(&s.lib) {
            Some(d) if d.units > 1 => format!("{}{}", s.reference, super::symbols::unit_letter(s.unit)),
            _ => s.reference.clone(),
        }
    }

    /// Places the next unit of part `i` that is not on this sheet yet (same reference).
    pub fn add_next_unit(&mut self, i: usize, pos: P) -> Option<usize> {
        let s = self.symbols[i].clone();
        let units = self.def(&s.lib)?.units;
        let used: Vec<u32> = self.symbols.iter().filter(|x| x.reference == s.reference && x.lib == s.lib).map(|x| x.unit).collect();
        let u = (1..=units).find(|u| !used.contains(u))?;
        self.symbols.push(SymInst { unit: u, pos, ..s });
        Some(self.symbols.len() - 1)
    }

    /// Every symbol of the hierarchy (this sheet first).
    pub fn all_symbols(&self) -> Vec<&SymInst> {
        let mut v: Vec<&SymInst> = self.symbols.iter().collect();
        for sh in &self.sheets {
            v.extend(sh.sch.all_symbols());
        }
        v
    }

    /// Sub-sheet by path (indices from this sheet down).
    pub fn sheet_at(&self, path: &[usize]) -> Option<&Schematic> {
        match path.split_first() {
            None => Some(self),
            Some((k, rest)) => self.sheets.get(*k)?.sch.sheet_at(rest),
        }
    }

    pub fn sheet_at_mut(&mut self, path: &[usize]) -> Option<&mut Schematic> {
        match path.split_first() {
            None => Some(self),
            Some((k, rest)) => self.sheets.get_mut(*k)?.sch.sheet_at_mut(rest),
        }
    }

    /// The whole hierarchy as one flat sheet: sub-sheets moved apart, their local and
    /// hierarchical labels prefixed with the sheet path ("/Netzteil/VIN"), sheet pins turned
    /// into labels of the same name; bus entries become wires. Netlist, ERC, BOM and the
    /// board update work on this.
    pub fn flatten(&self) -> Schematic {
        let mut out = Schematic { title: self.title.clone(), ..Default::default() };
        let mut counter = 0usize;
        self.flatten_into(&mut out, "", P::ZERO, &mut counter);
        out
    }

    fn flatten_into(&self, out: &mut Schematic, path: &str, off: P, counter: &mut usize) {
        for l in &self.local_lib {
            if !out.local_lib.iter().any(|x| x.id == l.id) {
                out.local_lib.push(l.clone());
            }
        }
        for s in &self.symbols {
            out.symbols.push(SymInst { pos: s.pos + off, ..s.clone() });
        }
        for w in self.wires.iter().chain(&self.bus_entries) {
            out.wires.push(Wire { a: w.a + off, b: w.b + off });
        }
        for b in &self.buses {
            out.buses.push(Wire { a: b.a + off, b: b.b + off });
        }
        for l in &self.labels {
            let text = match l.kind {
                LabelKind::Global => l.text.clone(),
                LabelKind::Local if path.is_empty() => l.text.clone(),
                _ => format!("{}/{}", path, l.text),
            };
            out.labels.push(Label { pos: l.pos + off, text, kind: if l.kind == LabelKind::Global { LabelKind::Global } else { LabelKind::Local }, rot: l.rot });
        }
        for q in &self.no_connects {
            out.no_connects.push(*q + off);
        }
        for sh in &self.sheets {
            let child = format!("{}/{}", path, sh.name);
            for pin in &sh.pins {
                out.labels.push(Label { pos: pin.pos + off, text: format!("{}/{}", child, pin.name), kind: LabelKind::Local, rot: 0.0 });
            }
            *counter += 1;
            let child_off = p(100_000.0 * *counter as f64, 0.0);
            sh.sch.flatten_into(out, &child, child_off, counter);
        }
    }

    pub fn is_power(&self, i: usize) -> bool {
        self.def(&self.symbols[i].lib).is_some_and(|d| d.power)
    }

    /// Adds a symbol from the library at `pos`; the reference gets a '?' until annotated.
    pub fn add_symbol(&mut self, lib: &str, pos: P) -> Option<usize> {
        let d = self.def(lib)?.clone();
        let reference = if d.power { format!("#PWR0{:02}", self.symbols.len() + 1) } else { format!("{}?", d.prefix) };
        self.symbols.push(SymInst { lib: d.id.clone(), reference, value: d.value.clone(), footprint: d.footprint.clone(), pos, rot: 0.0, mirror: false, fields: vec![], unit: 1 });
        Some(self.symbols.len() - 1)
    }

    /// Adds a wire, splitting it so every segment is straight (horizontal then vertical).
    pub fn add_wire(&mut self, a: P, b: P) {
        if (a - b).length() < EPS {
            return;
        }
        self.wires.push(Wire { a, b });
    }

    fn nodes(&self) -> (Vec<(Node, P)>, Vec<usize>) {
        let mut nodes = vec![];
        for (i, w) in self.wires.iter().enumerate() {
            nodes.push((Node::WireEnd(i), w.a));
            nodes.push((Node::WireEnd(i), w.b));
        }
        let mut pin_start = vec![];
        for i in 0..self.symbols.len() {
            pin_start.push(nodes.len());
            for (k, (pos, _)) in self.pin_positions(i).into_iter().enumerate() {
                nodes.push((Node::Pin(i, k), pos));
            }
        }
        for (i, l) in self.labels.iter().enumerate() {
            nodes.push((Node::Label(i), l.pos));
        }
        for (i, q) in self.no_connects.iter().enumerate() {
            nodes.push((Node::Nc(i), *q));
        }
        (nodes, pin_start)
    }

    /// Hidden power-input pins of parts (74xx VCC/GND …) connect to the global net named
    /// after the pin, not by position (KiCad's rule).
    fn implicit_power(&self, n: &Node) -> Option<String> {
        let Node::Pin(s, k) = n else { return None };
        let inst = &self.symbols[*s];
        let d = self.def(&inst.lib)?;
        if d.power {
            return None;
        }
        let pin = d.pins_of(inst.unit).nth(*k)?;
        (pin.hidden && pin.kind == PinType::PowerIn).then(|| pin.name.clone())
    }

    fn connect(&self, nodes: &[(Node, P)]) -> Uf {
        let mut uf = Uf::new(nodes.len());
        let implicit: Vec<bool> = nodes.iter().map(|(n, _)| self.implicit_power(n).is_some()).collect();
        // both ends of a wire
        for i in 0..self.wires.len() {
            uf.join(2 * i, 2 * i + 1);
        }
        // coincident points (sorted sweep)
        let mut order: Vec<usize> = (0..nodes.len()).collect();
        order.sort_by(|&a, &b| nodes[a].1.x.partial_cmp(&nodes[b].1.x).unwrap());
        for (k, &i) in order.iter().enumerate() {
            for &j in &order[k + 1..] {
                if nodes[j].1.x - nodes[i].1.x > EPS {
                    break;
                }
                if implicit[i] || implicit[j] {
                    continue;
                }
                if (nodes[i].1 - nodes[j].1).length() < EPS {
                    uf.join(i, j);
                }
            }
        }
        // points on the middle of a wire (T joints, labels and pins on wires)
        for (wi, w) in self.wires.iter().enumerate() {
            for (ni, (_, q)) in nodes.iter().enumerate() {
                if !implicit[ni] && pt_seg(*q, w.a, w.b) < EPS {
                    uf.join(ni, 2 * wi);
                }
            }
        }
        // labels with the same text, power symbols with the same value
        let mut by_name: HashMap<String, usize> = HashMap::new();
        for (ni, (n, _)) in nodes.iter().enumerate() {
            let key = match n {
                Node::Label(l) if bus_members(&self.labels[*l].text).is_none() => Some(self.labels[*l].text.clone()),
                Node::Pin(s, _) if self.is_power(*s) && !self.symbols[*s].lib.ends_with("PWR_FLAG") && self.symbols[*s].value != "PWR_FLAG" => Some(self.symbols[*s].value.clone()),
                n if implicit[ni] => self.implicit_power(n),
                _ => None,
            };
            if let Some(k) = key {
                match by_name.get(&k) {
                    Some(&f) => uf.join(f, ni),
                    None => {
                        by_name.insert(k, ni);
                    }
                }
            }
        }
        uf
    }

    /// Junction dots: where three or more connection points meet, or a wire end meets the
    /// middle of another wire.
    pub fn junctions(&self) -> Vec<P> {
        if !self.bus_entries.is_empty() {
            let mut e = self.clone();
            e.wires.extend(self.bus_entries.iter().copied());
            e.bus_entries.clear();
            return e.junctions();
        }
        let (nodes, _) = self.nodes();
        let mut pts: Vec<(P, usize)> = vec![];
        for (n, q) in &nodes {
            if matches!(n, Node::Label(_) | Node::Nc(_)) {
                continue;
            }
            let mut count = 0;
            for w in &self.wires {
                if (w.a - *q).length() < EPS || (w.b - *q).length() < EPS {
                    count += 1;
                } else if pt_seg(*q, w.a, w.b) < EPS {
                    count += 2;
                }
            }
            if matches!(n, Node::Pin(..)) {
                count += 1;
            }
            if count >= 3 && !pts.iter().any(|(r, _)| (*r - *q).length() < EPS) {
                pts.push((*q, count));
            }
        }
        pts.into_iter().map(|(q, _)| q).collect()
    }

    /// Netlist of the whole hierarchy.
    pub fn netlist(&self) -> Netlist {
        if self.sheets.is_empty() && self.bus_entries.is_empty() {
            self.netlist_flat()
        } else {
            self.flatten().netlist_flat()
        }
    }

    fn netlist_flat(&self) -> Netlist {
        let (nodes, _) = self.nodes();
        let mut uf = self.connect(&nodes);
        let mut groups: BTreeMap<usize, (Vec<NetPin>, Vec<usize>)> = BTreeMap::new();
        for (ni, (n, q)) in nodes.iter().enumerate() {
            let root = uf.find(ni);
            match n {
                Node::Pin(s, k) => {
                    let inst = &self.symbols[*s];
                    let Some(d) = self.def(&inst.lib) else { continue };
                    let Some(pin) = d.pins_of(inst.unit).nth(*k) else { continue };
                    groups.entry(root).or_default().0.push(NetPin {
                        sym: *s,
                        reference: inst.reference.clone(),
                        pin: pin.num.clone(),
                        pin_name: pin.name.clone(),
                        kind: pin.kind,
                        power_sym: d.power,
                        pos: *q,
                    });
                }
                Node::Label(l) => groups.entry(root).or_default().1.push(*l),
                _ => {}
            }
        }
        let mut nets = vec![];
        let mut used: HashMap<String, usize> = HashMap::new();
        for (_, (mut pins, labels)) in groups {
            if pins.is_empty() && labels.iter().all(|&l| bus_members(&self.labels[l].text).is_some()) {
                continue;
            }
            pins.sort_by(|a, b| natural(&a.reference).cmp(&natural(&b.reference)).then(natural(&a.pin).cmp(&natural(&b.pin))));
            let power = pins
                .iter()
                .find(|q| q.power_sym && !self.symbols[q.sym].lib.ends_with("PWR_FLAG"))
                .map(|q| self.symbols[q.sym].value.clone())
                .or_else(|| {
                    pins.iter().find_map(|q| {
                        let inst = &self.symbols[q.sym];
                        let d = self.def(&inst.lib)?;
                        let pin = d.pins_of(inst.unit).find(|x| x.num == q.pin)?;
                        (!d.power && pin.hidden && pin.kind == PinType::PowerIn).then(|| pin.name.clone())
                    })
                });
            let global = labels.iter().find(|&&l| self.labels[l].kind == LabelKind::Global).map(|&l| self.labels[l].text.clone());
            let local = labels.first().map(|&l| self.labels[l].text.clone());
            let auto = pins.iter().find(|q| !q.power_sym).map(|q| if pins.iter().filter(|r| !r.power_sym).count() == 1 { format!("unconnected-({}-Pad{})", q.reference, q.pin) } else { format!("Net-({}-Pad{})", q.reference, q.pin) });
            let mut name = power.or(global).or(local).or(auto).unwrap_or_else(|| "Net-(?)".into());
            if let Some(k) = used.get_mut(&name) {
                *k += 1;
                name = format!("{}_{}", name, k);
            } else {
                used.insert(name.clone(), 1);
            }
            nets.push(Net { name, pins, labels });
        }
        nets.sort_by(|a, b| natural(&a.name).cmp(&natural(&b.name)));
        Netlist { nets }
    }

    /// Numbers references ending in '?' (or all when `reset`) across the whole hierarchy,
    /// left to right, then top to bottom, like KiCad's default annotation order. Units of
    /// one multi-unit part keep a common reference. Returns how many parts got a number.
    pub fn annotate(&mut self, reset: bool) -> usize {
        let split = |r: &str| -> (String, Option<u32>) {
            let i = r.find(|c: char| c.is_ascii_digit() || c == '?').unwrap_or(r.len());
            (r[..i].to_string(), r[i..].parse().ok())
        };
        // (path, index, power, prefix, units, sheet order, position, reference, lib, unit)
        struct Info {
            path: Vec<usize>,
            i: usize,
            power: bool,
            flag: bool,
            prefix: String,
            units: u32,
            order: usize,
            pos: P,
            reference: String,
            lib: String,
            unit: u32,
        }
        fn gather(s: &Schematic, path: &mut Vec<usize>, order: &mut usize, out: &mut Vec<Info>) {
            let me = *order;
            for (i, x) in s.symbols.iter().enumerate() {
                let d = s.def(&x.lib);
                out.push(Info {
                    path: path.clone(),
                    i,
                    power: d.is_some_and(|d| d.power),
                    flag: x.lib.ends_with("PWR_FLAG"),
                    prefix: d.map(|d| d.prefix.clone()).unwrap_or_default(),
                    units: d.map(|d| d.units).unwrap_or(1),
                    order: me,
                    pos: x.pos,
                    reference: x.reference.clone(),
                    lib: x.lib.clone(),
                    unit: x.unit,
                });
            }
            for (k, sh) in s.sheets.iter().enumerate() {
                *order += 1;
                path.push(k);
                gather(&sh.sch, path, order, out);
                path.pop();
            }
        }
        let mut infos = vec![];
        gather(self, &mut vec![], &mut 0, &mut infos);
        let mut taken: HashMap<String, Vec<u32>> = HashMap::new();
        let mut todo = vec![];
        for (k, x) in infos.iter().enumerate() {
            if x.power {
                continue;
            }
            let (pre, num) = split(&x.reference);
            match num {
                Some(n) if !reset => taken.entry(pre).or_default().push(n),
                _ => todo.push(k),
            }
        }
        todo.sort_by(|&a, &b| {
            let (x, y) = (&infos[a], &infos[b]);
            x.order.cmp(&y.order).then(x.pos.x.partial_cmp(&y.pos.x).unwrap()).then(x.pos.y.partial_cmp(&y.pos.y).unwrap())
        });
        let mut new_refs: Vec<(usize, String)> = vec![];
        // open slots of multi-unit parts: (lib, reference) → units still free
        let mut open: Vec<(String, String, Vec<u32>)> = vec![];
        for x in infos.iter().filter(|x| x.units > 1 && !x.power && split(&x.reference).1.is_some() && !reset) {
            match open.iter_mut().find(|o| o.0 == x.lib && o.1 == x.reference) {
                Some(o) => o.2.retain(|u| *u != x.unit),
                None => open.push((x.lib.clone(), x.reference.clone(), (1..=x.units).filter(|u| *u != x.unit).collect())),
            }
        }
        let mut count = 0;
        for &k in &todo {
            let x = &infos[k];
            let pre = if x.prefix.is_empty() { split(&x.reference).0 } else { x.prefix.clone() };
            if x.units > 1 {
                if let Some(o) = open.iter_mut().find(|o| o.0 == x.lib && o.2.contains(&x.unit)) {
                    o.2.retain(|u| *u != x.unit);
                    new_refs.push((k, o.1.clone()));
                    continue;
                }
            }
            let t = taken.entry(pre.clone()).or_default();
            let mut n = 1;
            while t.contains(&n) {
                n += 1;
            }
            t.push(n);
            let r = format!("{}{}", pre, n);
            if x.units > 1 {
                open.push((x.lib.clone(), r.clone(), (1..=x.units).filter(|u| *u != x.unit).collect()));
            }
            new_refs.push((k, r));
            count += 1;
        }
        // power symbols get unique hidden references too
        let mut pk = 1;
        for (k, x) in infos.iter().enumerate() {
            if x.power {
                new_refs.push((k, format!("{}{:02}", if x.flag { "#FLG" } else { "#PWR" }, pk)));
                pk += 1;
            }
        }
        for (k, r) in new_refs {
            let x = &infos[k];
            if let Some(sh) = self.sheet_at_mut(&x.path) {
                sh.symbols[x.i].reference = r;
            }
        }
        count
    }

    /// Electrical rules check of the whole hierarchy (positions of findings inside
    /// sub-sheets refer to the flattened sheet; the message names the sheet).
    pub fn erc(&self) -> Vec<ErcItem> {
        let mut out = vec![];
        self.erc_hier("", &mut out);
        if self.sheets.is_empty() && self.bus_entries.is_empty() {
            out.extend(self.erc_flat());
        } else {
            let flat = self.flatten();
            for mut e in flat.erc_flat() {
                // findings inside a sub-sheet sit at 100 m offsets: move them back home
                if e.pos.x >= 50_000.0 {
                    e.pos.x %= 100_000.0;
                    e.msg = format!("{} (Unterblatt)", e.msg);
                }
                out.push(e);
            }
        }
        out
    }

    /// Hierarchy checks: sheet pins without hierarchical label and the other way round,
    /// parts with units missing, bus labels without members.
    fn erc_hier(&self, path: &str, out: &mut Vec<ErcItem>) {
        for sh in &self.sheets {
            for pin in &sh.pins {
                if !sh.sch.labels.iter().any(|l| l.kind == LabelKind::Hier && l.text == pin.name) {
                    out.push(ErcItem { severity: Severity::Error, msg: format!("Blattpin {} von Blatt {} hat kein hierarchisches Label im Blatt", pin.name, sh.name), pos: pin.pos });
                }
            }
            for l in sh.sch.labels.iter().filter(|l| l.kind == LabelKind::Hier) {
                if !sh.pins.iter().any(|q| q.name == l.text) {
                    out.push(ErcItem { severity: Severity::Warning, msg: format!("Hierarchisches Label {} in {}{} hat keinen Blattpin", l.text, path, sh.name), pos: sh.pos });
                }
            }
            sh.sch.erc_hier(&format!("{}/{}", path, sh.name), out);
        }
        if path.is_empty() {
            // units: every unit of a multi-unit part should be placed somewhere
            let all = self.all_symbols();
            let mut seen: Vec<(&str, &str)> = vec![];
            for x in &all {
                let Some(d) = self.def(&x.lib).or_else(|| self.flatten_def(&x.lib)) else { continue };
                if d.units <= 1 || x.reference.ends_with('?') || seen.contains(&(x.reference.as_str(), x.lib.as_str())) {
                    continue;
                }
                seen.push((&x.reference, &x.lib));
                let have: Vec<u32> = all.iter().filter(|y| y.reference == x.reference && y.lib == x.lib).map(|y| y.unit).collect();
                for u in 1..=d.units {
                    if !have.contains(&u) {
                        let shared = d.pins.iter().any(|q| q.unit == u && q.kind == PinType::PowerIn);
                        out.push(ErcItem {
                            severity: if shared { Severity::Error } else { Severity::Warning },
                            msg: format!("Einheit {}{} ({}) ist nicht platziert{}", x.reference, super::symbols::unit_letter(u), d.short(), if shared { " – ihre Versorgungspins fehlen im Netz" } else { "" }),
                            pos: x.pos,
                        });
                    }
                }
            }
        }
        let names: Vec<&str> = self.labels.iter().map(|l| l.text.as_str()).collect();
        for l in &self.labels {
            if let Some(m) = bus_members(&l.text) {
                if !m.iter().any(|x| names.contains(&x.as_str())) {
                    out.push(ErcItem { severity: Severity::Warning, msg: format!("Bus {}: keines der Netze {} … ist beschriftet", l.text, m[0]), pos: l.pos });
                }
            }
        }
    }

    fn flatten_def(&self, lib: &str) -> Option<&Symbol> {
        self.sheets.iter().find_map(|s| s.sch.def(lib).or_else(|| s.sch.flatten_def(lib)))
    }

    fn erc_flat(&self) -> Vec<ErcItem> {
        use PinType::*;
        let mut out = vec![];
        let err = |out: &mut Vec<ErcItem>, sev, msg: String, pos| out.push(ErcItem { severity: sev, msg, pos });
        // references
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for (i, s) in self.symbols.iter().enumerate() {
            if self.is_power(i) {
                continue;
            }
            if s.reference.ends_with('?') {
                err(&mut out, Severity::Error, format!("Bauteil {} ist nicht annotiert", s.reference), s.pos);
            } else if let Some(&j) = seen.get(s.reference.as_str()) {
                let o = &self.symbols[j];
                let multi = self.def(&s.lib).is_some_and(|d| d.units > 1);
                if !(multi && o.lib == s.lib && o.unit != s.unit) || self.symbols[..i].iter().any(|x| x.reference == s.reference && x.unit == s.unit && x.lib == s.lib) {
                    err(&mut out, Severity::Error, format!("Referenz {} ist doppelt vergeben", self.shown_ref(i)), o.pos);
                }
            } else {
                seen.insert(&s.reference, i);
            }
            if self.def(&s.lib).is_none() {
                err(&mut out, Severity::Error, format!("{}: Symbol {} nicht in der Bibliothek", s.reference, s.lib), s.pos);
            }
            if s.footprint.is_empty() {
                err(&mut out, Severity::Warning, format!("{}: kein Footprint zugeordnet", s.reference), s.pos);
            }
        }
        let net = self.netlist();
        let nc_at = |q: P| self.no_connects.iter().any(|n| (*n - q).length() < EPS);
        for n in &net.nets {
            let parts: Vec<&NetPin> = n.pins.iter().filter(|q| !q.power_sym).collect();
            let all = &n.pins;
            if parts.len() == 1 && all.len() == 1 && n.labels.is_empty() {
                let q = parts[0];
                if q.kind != NoConnect && !nc_at(q.pos) {
                    err(&mut out, Severity::Error, format!("Pin {} ({}) von {} ist nicht angeschlossen", q.pin, q.pin_name, q.reference), q.pos);
                }
                continue;
            }
            if parts.len() >= 2 || (parts.len() == 1 && all.len() > 1) {
                for q in &parts {
                    if nc_at(q.pos) {
                        err(&mut out, Severity::Warning, format!("Kein-Anschluss-Markierung an verbundenem Pin {} von {}", q.pin, q.reference), q.pos);
                    }
                }
            }
            if parts.len() == 1 && all.len() == 1 && !n.labels.is_empty() {
                let l = &self.labels[n.labels[0]];
                err(&mut out, Severity::Warning, format!("Netzlabel „{}“ verbindet nur einen Pin", l.text), l.pos);
            }
            // drivers
            let count = |k: PinType| all.iter().filter(|q| q.kind == k).count();
            let power_in = all.iter().filter(|q| q.kind == PowerIn && !q.power_sym).collect::<Vec<_>>();
            let power_out = count(PowerOut);
            if !power_in.is_empty() && power_out == 0 {
                let q = power_in[0];
                err(&mut out, Severity::Error, format!("Versorgungseingang {} ({}) von {} wird nicht versorgt (Netz {}) – PWR_FLAG oder Regler-Ausgang fehlt", q.pin, q.pin_name, q.reference, n.name), q.pos);
            }
            let outputs: Vec<&&NetPin> = parts.iter().filter(|q| q.kind == Output).collect();
            if outputs.len() >= 2 {
                err(&mut out, Severity::Error, format!("Ausgänge gegeneinander: {} Pin {} und {} Pin {}", outputs[0].reference, outputs[0].pin, outputs[1].reference, outputs[1].pin), outputs[1].pos);
            }
            if !outputs.is_empty() && power_out > 0 {
                err(&mut out, Severity::Error, format!("Ausgang {} Pin {} hängt an einem Versorgungsnetz ({})", outputs[0].reference, outputs[0].pin, n.name), outputs[0].pos);
            }
            let inputs = parts.iter().filter(|q| q.kind == Input).count();
            if inputs > 0 && inputs == parts.len() && power_out == 0 && all.iter().all(|q| !q.power_sym) {
                let q = parts[0];
                err(&mut out, Severity::Error, format!("Eingang {} von {} wird von nichts getrieben (Netz {})", q.pin, q.reference, n.name), q.pos);
            }
        }
        // dangling wire ends
        let (nodes, _) = self.nodes();
        for (wi, w) in self.wires.iter().enumerate() {
            for end in [w.a, w.b] {
                let touches = nodes.iter().any(|(n, q)| !matches!(n, Node::WireEnd(k) if *k == wi) && (*q - end).length() < EPS) || self.wires.iter().enumerate().any(|(k, o)| k != wi && pt_seg(end, o.a, o.b) < EPS);
                if !touches {
                    err(&mut out, Severity::Warning, "Drahtende ist nicht angeschlossen".into(), end);
                }
            }
        }
        out
    }

    /// The wire, symbol or label nearest to `q` within `tol`.
    pub fn bounds(&self) -> Option<(P, P)> {
        let mut pts = vec![];
        for (i, s) in self.symbols.iter().enumerate() {
            if let Some(d) = self.def(&s.lib) {
                let (lo, hi) = d.bounds();
                for c in [lo, hi, p(lo.x, hi.y), p(hi.x, lo.y)] {
                    pts.push(Self::xform(s, c));
                }
            }
            let _ = i;
        }
        for w in &self.wires {
            pts.extend([w.a, w.b]);
        }
        for l in &self.labels {
            pts.push(l.pos);
        }
        for b in &self.buses {
            pts.extend([b.a, b.b]);
        }
        for sh in &self.sheets {
            pts.extend([sh.pos, sh.pos + sh.size]);
        }
        super::geom::bbox(pts)
    }
}

/// Sort key that orders "R2" before "R10".
pub fn natural(s: &str) -> (String, u64, String) {
    let i = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    let j = s[i..].find(|c: char| !c.is_ascii_digit()).map(|k| i + k).unwrap_or(s.len());
    (s[..i].to_string(), s[i..j].parse().unwrap_or(0), s[j..].to_string())
}
