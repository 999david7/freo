//! Assembly and fabrication data besides Gerber/Excellon: pick-and-place (KiCad .pos and
//! CSV), the Gerber job file (.gbrjob) and IPC-2581 (rev. B) as one XML file.
//!
//! Coordinates are millimetres relative to the lower-left corner of the board, y up.

use super::board::{layer_name, Board, B_CU, F_CU};
use super::footprints::PadShape;
use super::geom::{p, P};
use super::schematic::natural;
use std::fmt::Write;

/// Placement origin: lower-left corner of the board outline (KiCad's "Bohr-/Platzierungsursprung").
pub fn origin(b: &Board) -> P {
    b.size().map(|(lo, hi)| p(lo.x, hi.y)).unwrap_or(P::ZERO)
}

fn local(b: &Board, q: P) -> P {
    let o = origin(b);
    p(q.x - o.x, o.y - q.y)
}

fn f4(v: f64) -> String {
    let s = format!("{:.4}", v);
    if s == "-0.0000" {
        "0.0000".into()
    } else {
        s
    }
}

/// One placement row.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub reference: String,
    pub value: String,
    pub package: String,
    pub pos: P,
    pub rot: f64,
    pub top: bool,
    pub smd: bool,
}

/// Placements sorted by reference; positions are the footprint origins (pad centroid for
/// parts whose origin is pin 1, like KiCad's "Mitte" option).
pub fn placements(b: &Board) -> Vec<Place> {
    let mut v: Vec<Place> = b
        .footprints
        .iter()
        .map(|f| {
            // centroid of the pads = the part's centre, also for pin-1-origin THT parts
            let n = f.fp.pads.len().max(1) as f64;
            let c = f.fp.pads.iter().map(|q| q.pos).sum::<P>() / n;
            let (lo, hi) = f.fp.court;
            let mid = if f.fp.smd() { P::ZERO } else { ((lo + hi) / 2.0 + c) / 2.0 };
            Place {
                reference: f.reference.clone(),
                value: f.value.clone(),
                package: f.fp.short().to_string(),
                pos: local(b, f.xform(mid)),
                rot: if f.bottom { (180.0 - f.rot).rem_euclid(360.0) } else { f.rot.rem_euclid(360.0) },
                top: !f.bottom,
                smd: f.fp.smd(),
            }
        })
        .collect();
    v.sort_by(|a, c| natural(&a.reference).cmp(&natural(&c.reference)));
    v
}

/// KiCad-style .pos file for one side (or both).
pub fn pos_file(b: &Board, side: Option<bool>) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "### Footprint positions - created by Freo {} ###", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(s, "## Unit = mm, Angle = deg.");
    let _ = writeln!(s, "## Side : {}", match side {
        Some(true) => "top",
        Some(false) => "bottom",
        None => "all",
    });
    let _ = writeln!(s, "# Ref     Val       Package                PosX       PosY       Rot  Side");
    for pl in placements(b).into_iter().filter(|x| side.is_none_or(|t| t == x.top)) {
        let _ = writeln!(s, "{:<9} {:<9} {:<22} {:>9} {:>9} {:>8}  {}", pl.reference, pl.value, pl.package, f4(pl.pos.x), f4(pl.pos.y), f4(pl.rot), if pl.top { "top" } else { "bottom" });
    }
    let _ = writeln!(s, "## End");
    s
}

/// Pick-and-place CSV (the column names most assembly services read).
pub fn pos_csv(b: &Board) -> String {
    let mut s = String::from("Designator,Val,Package,Mid X,Mid Y,Rotation,Layer\n");
    for pl in placements(b) {
        let _ = writeln!(s, "{},\"{}\",\"{}\",{},{},{},{}", pl.reference, pl.value.replace('"', "'"), pl.package, f4(pl.pos.x), f4(pl.pos.y), f4(pl.rot), if pl.top { "top" } else { "bottom" });
    }
    s
}

/// Gerber job file (JSON) describing the fabrication file set.
pub fn job_file(b: &Board, name: &str) -> String {
    let (w, h) = b.size().map(|(lo, hi)| (hi.x - lo.x, hi.y - lo.y)).unwrap_or((0.0, 0.0));
    let mut files = vec![];
    for l in super::gerber::Layer::all(b.layers) {
        let (func, pol) = match l {
            super::gerber::Layer::Copper(0) => ("Copper,L1,Top".to_string(), "Positive"),
            super::gerber::Layer::Copper(1) => (format!("Copper,L{},Bot", b.layers), "Positive"),
            super::gerber::Layer::Copper(k) => (format!("Copper,L{},Inr", k), "Positive"),
            super::gerber::Layer::Mask(t) => (format!("SolderMask,{}", if t { "Top" } else { "Bot" }), "Negative"),
            super::gerber::Layer::Paste(t) => (format!("SolderPaste,{}", if t { "Top" } else { "Bot" }), "Positive"),
            super::gerber::Layer::Silk(t) => (format!("Legend,{}", if t { "Top" } else { "Bot" }), "Positive"),
            super::gerber::Layer::Edge => ("Profile".to_string(), "Positive"),
        };
        files.push(serde_json::json!({ "Path": format!("{}{}", name, l.file_suffix(b.layers)), "FileFunction": func, "FilePolarity": pol }));
    }
    let r = &b.rules;
    let min_w = b.tracks.iter().map(|t| t.width).fold(f64::MAX, f64::min);
    let v = serde_json::json!({
        "Header": { "GenerationSoftware": { "Vendor": "Freo", "Application": "Freo Leiterplatte", "Version": env!("CARGO_PKG_VERSION") }, "CreationDate": "" },
        "GeneralSpecs": {
            "ProjectId": { "Name": name, "GUID": "00000000-0000-0000-0000-000000000000", "Revision": "1" },
            "Size": { "X": (w * 1000.0).round() / 1000.0, "Y": (h * 1000.0).round() / 1000.0 },
            "LayerNumber": b.layers,
            "BoardThickness": b.thickness,
            "Finish": "None"
        },
        "DesignRules": [{ "Layers": "Outer", "PadToPad": r.clearance, "PadToTrack": r.clearance, "TrackToTrack": r.clearance, "MinLineWidth": if min_w == f64::MAX { r.track } else { min_w } }],
        "FilesAttributes": files,
        "MaterialStackup": (0..b.layers).map(|i| serde_json::json!({ "Type": "Copper", "Thickness": 0.035, "Name": layer_name(super::autoroute::stack(b).get(i as usize).copied().unwrap_or(F_CU)) })).collect::<Vec<_>>()
    });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// IPC-2581 rev. B: content, stack-up, board profile, package and component data, the
/// copper features per layer (pads, tracks, vias, zones) and the drills, with nets.
pub fn ipc2581(b: &Board, name: &str) -> String {
    let mut s = String::new();
    let o = |q: P| local(b, q);
    let lays = super::autoroute::stack(b);
    let _ = writeln!(s, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(s, r#"<IPC-2581 revision="B" xmlns="http://webstds.ipc.org/2581" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">"#);
    let _ = writeln!(s, r#"  <Content roleRef="Owner">"#);
    let _ = writeln!(s, r#"    <FunctionMode mode="FABRICATION"/>"#);
    let _ = writeln!(s, r#"    <StepRef name="{}"/>"#, esc(name));
    for l in &lays {
        let _ = writeln!(s, r#"    <LayerRef name="{}"/>"#, layer_name(*l));
    }
    let _ = writeln!(s, r#"    <LayerRef name="DRILL_PTH"/>"#);
    let _ = writeln!(s, r#"    <DictionaryStandard units="MILLIMETER">"#);
    // pad shapes as standard primitives
    let mut prims: Vec<(String, String)> = vec![];
    let mut prim = |shape: &str| -> String {
        if let Some(x) = prims.iter().find(|x| x.1 == shape) {
            return x.0.clone();
        }
        let id = format!("PRIM_{}", prims.len() + 1);
        prims.push((id.clone(), shape.to_string()));
        id
    };
    let mut pad_refs: Vec<Vec<String>> = vec![];
    for f in &b.footprints {
        let mut v = vec![];
        for pad in &f.fp.pads {
            let shape = match pad.shape {
                PadShape::Circle => format!(r#"<Circle diameter="{}"/>"#, f4(pad.size.x.max(pad.size.y))),
                PadShape::Rect => format!(r#"<RectCenter width="{}" height="{}"/>"#, f4(pad.size.x), f4(pad.size.y)),
                PadShape::Oval => format!(r#"<Oval width="{}" height="{}"/>"#, f4(pad.size.x), f4(pad.size.y)),
                PadShape::RoundRect(k) => format!(r#"<RectRound width="{}" height="{}" radius="{}"/>"#, f4(pad.size.x), f4(pad.size.y), f4(k * pad.size.x.min(pad.size.y))),
            };
            v.push(prim(&shape));
        }
        pad_refs.push(v);
    }
    let via_prims: Vec<String> = b.vias.iter().map(|v| prim(&format!(r#"<Circle diameter="{}"/>"#, f4(v.dia)))).collect();
    for (id, shape) in &prims {
        let _ = writeln!(s, r#"      <EntryStandard id="{}">{}</EntryStandard>"#, id, shape);
    }
    let _ = writeln!(s, r#"    </DictionaryStandard>"#);
    let _ = writeln!(s, r#"  </Content>"#);
    let _ = writeln!(s, r#"  <LogisticHeader><Role id="Owner" roleFunction="SENDER"/><Enterprise id="Freo" code="NONE"/><Person name="Freo" enterpriseRef="Freo" roleRef="Owner"/></LogisticHeader>"#);
    let _ = writeln!(s, r#"  <HistoryRecord number="1" origination="" software="Freo" lastChange=""><FileRevision fileRevisionId="1" comment=""><SoftwarePackage name="Freo" revision="{}" vendor="Freo"/></FileRevision></HistoryRecord>"#, env!("CARGO_PKG_VERSION"));
    // BOM
    let _ = writeln!(s, r#"  <Bom name="{}_BOM"><BomHeader assembly="{}" revision="1"/>"#, esc(name), esc(name));
    let mut groups: Vec<(String, String, Vec<String>)> = vec![];
    for f in &b.footprints {
        match groups.iter_mut().find(|g| g.0 == f.value && g.1 == f.fp.id) {
            Some(g) => g.2.push(f.reference.clone()),
            None => groups.push((f.value.clone(), f.fp.id.clone(), vec![f.reference.clone()])),
        }
    }
    for (i, (val, pkg, refs)) in groups.iter().enumerate() {
        let _ = writeln!(s, r#"    <BomItem OEMDesignNumberRef="{}_{}" quantity="{}" pinCount="0" category="ELECTRICAL">"#, esc(val), i + 1, refs.len());
        for r in refs {
            let _ = writeln!(s, r#"      <RefDes name="{}" packageRef="{}" populate="true" layerRef="{}"/>"#, esc(r), esc(pkg), layer_name(if b.footprints.iter().find(|f| &f.reference == r).is_some_and(|f| f.bottom) { B_CU } else { F_CU }));
        }
        let _ = writeln!(s, r#"      <Characteristics category="ELECTRICAL"><Textual definitionSource="Freo" textualCharacteristicName="Value" textualCharacteristicValue="{}"/></Characteristics>"#, esc(val));
        let _ = writeln!(s, r#"    </BomItem>"#);
    }
    let _ = writeln!(s, r#"  </Bom>"#);
    // ECAD
    let _ = writeln!(s, r#"  <Ecad name="{}">"#, esc(name));
    let _ = writeln!(s, r#"    <CadHeader units="MILLIMETER">"#);
    let _ = writeln!(s, r#"      <Spec name="BOARD"><General type="MATERIAL"><Property text="FR4"/></General></Spec>"#);
    let _ = writeln!(s, r#"    </CadHeader>"#);
    let _ = writeln!(s, r#"    <CadData>"#);
    for l in &lays {
        let side = if *l == F_CU {
            "TOP"
        } else if *l == B_CU {
            "BOTTOM"
        } else {
            "INTERNAL"
        };
        let _ = writeln!(s, r#"      <Layer name="{}" layerFunction="CONDUCTOR" side="{}" polarity="POSITIVE"/>"#, layer_name(*l), side);
    }
    let _ = writeln!(s, r#"      <Layer name="DRILL_PTH" layerFunction="DRILL" side="ALL" polarity="POSITIVE"><Span fromLayer="{}" toLayer="{}"/></Layer>"#, layer_name(F_CU), layer_name(B_CU));
    let _ = writeln!(s, r#"      <Stackup name="STACKUP" overallThickness="{}" tolPlus="0.1" tolMinus="0.1" whereMeasured="METAL">"#, f4(b.thickness));
    let _ = writeln!(s, r#"        <StackupGroup name="GROUP" thickness="{}" tolPlus="0.1" tolMinus="0.1">"#, f4(b.thickness));
    for (i, l) in lays.iter().enumerate() {
        let _ = writeln!(s, r#"          <StackupLayer layerOrGroupRef="{}" thickness="0.035" tolPlus="0" tolMinus="0" sequence="{}"/>"#, layer_name(*l), i + 1);
    }
    let _ = writeln!(s, r#"        </StackupGroup>"#);
    let _ = writeln!(s, r#"      </Stackup>"#);
    let _ = writeln!(s, r#"      <Step name="{}">"#, esc(name));
    let _ = writeln!(s, r#"        <Datum x="0" y="0"/>"#);
    if let Some(outer) = b.outer() {
        let _ = writeln!(s, r#"        <Profile><Polygon>"#);
        let first = o(outer[0]);
        let _ = writeln!(s, r#"          <PolyBegin x="{}" y="{}"/>"#, f4(first.x), f4(first.y));
        for q in outer.iter().skip(1).chain(std::iter::once(&outer[0])) {
            let q = o(*q);
            let _ = writeln!(s, r#"          <PolyStepSegment x="{}" y="{}"/>"#, f4(q.x), f4(q.y));
        }
        let _ = writeln!(s, r#"        </Polygon></Profile>"#);
    }
    // packages
    let mut pkgs: Vec<&str> = vec![];
    for (fi, f) in b.footprints.iter().enumerate() {
        if pkgs.contains(&f.fp.id.as_str()) {
            continue;
        }
        pkgs.push(&f.fp.id);
        let (lo, hi) = f.fp.court;
        let _ = writeln!(s, r#"        <Package name="{}" type="OTHER" pinOne="1" height="{}">"#, esc(&f.fp.id), f4(f.fp.height()));
        let _ = writeln!(s, r#"          <Outline><Polygon><PolyBegin x="{}" y="{}"/><PolyStepSegment x="{}" y="{}"/><PolyStepSegment x="{}" y="{}"/><PolyStepSegment x="{}" y="{}"/><PolyStepSegment x="{}" y="{}"/></Polygon><LineDesc lineWidth="0.05" lineEnd="ROUND"/></Outline>"#, f4(lo.x), f4(-lo.y), f4(hi.x), f4(-lo.y), f4(hi.x), f4(-hi.y), f4(lo.x), f4(-hi.y), f4(lo.x), f4(-lo.y));
        for (k, pad) in f.fp.pads.iter().enumerate() {
            let _ = writeln!(s, r#"          <Pin number="{}" type="{}" electricalType="ELECTRICAL"><Location x="{}" y="{}"/><StandardPrimitiveRef id="{}"/></Pin>"#, esc(&pad.num), if pad.smd() { "SURFACE" } else { "THRU" }, f4(pad.pos.x), f4(-pad.pos.y), pad_refs[fi][k]);
        }
        let _ = writeln!(s, r#"        </Package>"#);
    }
    for f in &b.footprints {
        let q = o(f.pos);
        let _ = writeln!(s, r#"        <Component refDes="{}" packageRef="{}" layerRef="{}" mountType="{}" part="{}"><Xform rotation="{}" mirror="{}"/><Location x="{}" y="{}"/></Component>"#, esc(&f.reference), esc(&f.fp.id), layer_name(f.side()), if f.fp.smd() { "SMT" } else { "THMT" }, esc(&f.value), f4(f.rot), f.bottom, f4(q.x), f4(q.y));
    }
    // logical nets
    for net in b.nets() {
        let _ = writeln!(s, r#"        <LogicalNet name="{}">"#, esc(&net));
        for f in &b.footprints {
            for (k, pad) in f.fp.pads.iter().enumerate() {
                if f.nets.get(k) == Some(&net) {
                    let _ = writeln!(s, r#"          <PinRef componentRef="{}" pin="{}"/>"#, esc(&f.reference), esc(&pad.num));
                }
            }
        }
        let _ = writeln!(s, r#"        </LogicalNet>"#);
    }
    // copper features per layer
    for l in &lays {
        let _ = writeln!(s, r#"        <LayerFeature layerRef="{}">"#, layer_name(*l));
        for (fi, f) in b.footprints.iter().enumerate() {
            for (k, pad) in f.fp.pads.iter().enumerate() {
                if f.pad_layers(k, b.layers) & (1 << l) == 0 {
                    continue;
                }
                let q = o(f.pad_pos(k));
                let _ = writeln!(s, r#"          <Set net="{}"><Pad padUsage="TERMINATION"><Xform rotation="{}"/><Location x="{}" y="{}"/><StandardPrimitiveRef id="{}"/><PinRef componentRef="{}" pin="{}"/></Pad></Set>"#, esc(&f.nets[k]), f4(f.rot + pad.rot), f4(q.x), f4(q.y), pad_refs[fi][k], esc(&f.reference), esc(&pad.num));
            }
        }
        for (i, v) in b.vias.iter().enumerate() {
            let q = o(v.pos);
            let _ = writeln!(s, r#"          <Set net="{}" padUsage="VIA"><Pad><Location x="{}" y="{}"/><StandardPrimitiveRef id="{}"/></Pad></Set>"#, esc(&v.net), f4(q.x), f4(q.y), via_prims[i]);
        }
        for t in b.tracks.iter().filter(|t| t.layer == *l) {
            let (a, e) = (o(t.a), o(t.b));
            let _ = writeln!(s, r#"          <Set net="{}"><Features><Line startX="{}" startY="{}" endX="{}" endY="{}"><LineDesc lineWidth="{}" lineEnd="ROUND"/></Line></Features></Set>"#, esc(&t.net), f4(a.x), f4(a.y), f4(e.x), f4(e.y), f4(t.width));
        }
        for z in b.zones.iter().filter(|z| z.layer == *l) {
            for shape in &z.fill {
                let _ = writeln!(s, r#"          <Set net="{}"><Features><Contour><Polygon>"#, esc(&z.net));
                let pts: Vec<P> = shape[0].iter().map(|q| o(p(q[0], q[1]))).collect();
                let _ = writeln!(s, r#"            <PolyBegin x="{}" y="{}"/>"#, f4(pts[0].x), f4(pts[0].y));
                for q in pts.iter().skip(1).chain(std::iter::once(&pts[0])) {
                    let _ = writeln!(s, r#"            <PolyStepSegment x="{}" y="{}"/>"#, f4(q.x), f4(q.y));
                }
                let _ = writeln!(s, r#"          </Polygon>"#);
                for h in &shape[1..] {
                    let hp: Vec<P> = h.iter().map(|q| o(p(q[0], q[1]))).collect();
                    let _ = writeln!(s, r#"          <Cutout><PolyBegin x="{}" y="{}"/>"#, f4(hp[0].x), f4(hp[0].y));
                    for q in hp.iter().skip(1).chain(std::iter::once(&hp[0])) {
                        let _ = writeln!(s, r#"            <PolyStepSegment x="{}" y="{}"/>"#, f4(q.x), f4(q.y));
                    }
                    let _ = writeln!(s, r#"          </Cutout>"#);
                }
                let _ = writeln!(s, r#"          </Contour></Features></Set>"#);
            }
        }
        let _ = writeln!(s, r#"        </LayerFeature>"#);
    }
    // drills
    let _ = writeln!(s, r#"        <LayerFeature layerRef="DRILL_PTH">"#);
    for v in &b.vias {
        let q = o(v.pos);
        let _ = writeln!(s, r#"          <Set net="{}"><Hole name="V" diameter="{}" platingStatus="VIA" plusTol="0" minusTol="0" x="{}" y="{}"/></Set>"#, esc(&v.net), f4(v.drill), f4(q.x), f4(q.y));
    }
    for f in &b.footprints {
        for (k, pad) in f.fp.pads.iter().enumerate() {
            if pad.drill > 0.0 {
                let (a, e) = pad.hole_ends(f.pos, f.rot, f.bottom);
                let (a, e) = (o(a), o(e));
                if pad.slot > pad.drill {
                    let _ = writeln!(s, r#"          <Set net="{}"><SlotCavity name="S" platingStatus="{}" plusTol="0" minusTol="0"><Line startX="{}" startY="{}" endX="{}" endY="{}"><LineDesc lineWidth="{}" lineEnd="ROUND"/></Line></SlotCavity></Set>"#, esc(&f.nets[k]), if pad.npth { "NONPLATED" } else { "PLATED" }, f4(a.x), f4(a.y), f4(e.x), f4(e.y), f4(pad.drill));
                } else {
                    let _ = writeln!(s, r#"          <Set net="{}"><Hole name="H" diameter="{}" platingStatus="{}" plusTol="0" minusTol="0" x="{}" y="{}"/></Set>"#, esc(&f.nets[k]), f4(pad.drill), if pad.npth { "NONPLATED" } else { "PLATED" }, f4(a.x), f4(a.y));
                }
            }
        }
    }
    let _ = writeln!(s, r#"        </LayerFeature>"#);
    let _ = writeln!(s, r#"      </Step>"#);
    let _ = writeln!(s, r#"    </CadData>"#);
    let _ = writeln!(s, r#"  </Ecad>"#);
    let _ = writeln!(s, r#"</IPC-2581>"#);
    s
}
