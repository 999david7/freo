//! Drawings (Creo .drw): sheets carrying views of a part or assembly, shown model dimensions,
//! created (driven) dimensions, notes, a bill-of-materials table with balloons, and the
//! drawing format with its title block.

use super::assembly::AssemblyData;
use super::features::ModelData;
pub use super::tolerance::{TolMode, Tolerance};
use crate::kernel::math::*;
use crate::kernel::topology::EdgeRef;
use glam::DQuat;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SheetSize { A4, A3, A2, A1, A0, A, B, C, D, E }

impl SheetSize {
    pub const ALL: [SheetSize; 10] = [SheetSize::A4, SheetSize::A3, SheetSize::A2, SheetSize::A1, SheetSize::A0, SheetSize::A, SheetSize::B, SheetSize::C, SheetSize::D, SheetSize::E];
    pub fn name(&self) -> &'static str {
        match self {
            SheetSize::A4 => "A4", SheetSize::A3 => "A3", SheetSize::A2 => "A2", SheetSize::A1 => "A1", SheetSize::A0 => "A0",
            SheetSize::A => "A", SheetSize::B => "B", SheetSize::C => "C", SheetSize::D => "D", SheetSize::E => "E",
        }
    }
    /// Landscape width and height in mm.
    pub fn landscape(&self) -> (f64, f64) {
        match self {
            SheetSize::A4 => (297.0, 210.0), SheetSize::A3 => (420.0, 297.0), SheetSize::A2 => (594.0, 420.0),
            SheetSize::A1 => (841.0, 594.0), SheetSize::A0 => (1189.0, 841.0),
            SheetSize::A => (279.4, 215.9), SheetSize::B => (431.8, 279.4), SheetSize::C => (558.8, 431.8),
            SheetSize::D => (863.6, 558.8), SheetSize::E => (1117.6, 863.6),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation { Landscape, Portrait }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Projection { Third, First }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewKind { General, Projection, Detail }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orient { Default, Front, Back, Top, Bottom, Left, Right }

impl Orient {
    pub const ALL: [Orient; 7] = [Orient::Default, Orient::Front, Orient::Back, Orient::Top, Orient::Bottom, Orient::Left, Orient::Right];
    pub fn name(&self) -> &'static str {
        match self {
            Orient::Default => "Standardausrichtung", Orient::Front => "FRONT", Orient::Back => "BACK", Orient::Top => "TOP",
            Orient::Bottom => "BOTTOM", Orient::Left => "LEFT", Orient::Right => "RIGHT",
        }
    }
    /// View → world rotation, as the 3D standard orientations.
    pub fn rotation(&self) -> DQuat {
        use std::f64::consts::{FRAC_PI_2, PI};
        match self {
            Orient::Default => DQuat::from_axis_angle(V3::Y, 45f64.to_radians()) * DQuat::from_axis_angle(V3::X, -(35.264f64.to_radians())),
            Orient::Front => DQuat::IDENTITY,
            Orient::Back => DQuat::from_axis_angle(V3::Y, PI),
            Orient::Top => DQuat::from_axis_angle(V3::X, -FRAC_PI_2),
            Orient::Bottom => DQuat::from_axis_angle(V3::X, FRAC_PI_2),
            Orient::Left => DQuat::from_axis_angle(V3::Y, -FRAC_PI_2),
            Orient::Right => DQuat::from_axis_angle(V3::Y, FRAC_PI_2),
        }
    }
    pub fn pictorial(&self) -> bool { *self == Orient::Default }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineStyle { Hidden, NoHidden, Wireframe }

impl LineStyle {
    pub const ALL: [LineStyle; 3] = [LineStyle::Wireframe, LineStyle::Hidden, LineStyle::NoHidden];
    pub fn name(&self) -> &'static str { match self { LineStyle::Hidden => "Verdeckt", LineStyle::NoHidden => "Keine verdeckten", LineStyle::Wireframe => "Drahtmodell" } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjSide { Right, Left, Top, Bottom }

impl ProjSide {
    pub fn label(self) -> &'static str {
        match self { ProjSide::Right => "rechte", ProjSide::Left => "linke", ProjSide::Top => "obere", ProjSide::Bottom => "untere" }
    }

    /// Placement side from the offset of the pointer to the parent view centre.
    pub fn from_offset(d: V2) -> ProjSide {
        if d.x.abs() >= d.y.abs() { if d.x >= 0.0 { ProjSide::Right } else { ProjSide::Left } } else if d.y >= 0.0 { ProjSide::Top } else { ProjSide::Bottom }
    }
    pub fn horizontal(&self) -> bool { matches!(self, ProjSide::Right | ProjSide::Left) }
}

/// Rotation (view → world) of a projection view placed on `side` of a view with `parent`.
/// Third angle shows on the right what is seen from the right; first angle the opposite.
pub fn projection_rotation(parent: DQuat, side: ProjSide, proj: Projection) -> DQuat {
    use std::f64::consts::FRAC_PI_2;
    let seen = match (side, proj) {
        (s, Projection::Third) => s,
        (ProjSide::Right, Projection::First) => ProjSide::Left,
        (ProjSide::Left, Projection::First) => ProjSide::Right,
        (ProjSide::Top, Projection::First) => ProjSide::Bottom,
        (ProjSide::Bottom, Projection::First) => ProjSide::Top,
    };
    let m = match seen {
        ProjSide::Right => DQuat::from_axis_angle(V3::Y, FRAC_PI_2),
        ProjSide::Left => DQuat::from_axis_angle(V3::Y, -FRAC_PI_2),
        ProjSide::Top => DQuat::from_axis_angle(V3::X, -FRAC_PI_2),
        ProjSide::Bottom => DQuat::from_axis_angle(V3::X, FRAC_PI_2),
    };
    (parent * m).normalize()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Detail {
    /// centre in the parent view's model coordinates
    pub center: V2,
    /// radius in model millimetres
    pub radius: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawView {
    pub id: u32,
    pub name: String,
    pub kind: ViewKind,
    pub parent: Option<u32>,
    pub side: Option<ProjSide>,
    /// view → world rotation
    pub rot: DQuat,
    pub orient: Option<Orient>,
    /// own scale (general and detail views); None uses the sheet scale
    pub scale: Option<f64>,
    /// centre on the sheet, mm
    pub pos: V2,
    pub style: LineStyle,
    /// centre lines and centre marks
    pub axes: bool,
    /// datum plane (parallel to the view) through which the view is sectioned
    #[serde(default)]
    pub section: Option<u64>,
    #[serde(default)]
    pub detail: Option<Detail>,
    /// letter used for section and detail labels
    #[serde(default)]
    pub letter: String,
    #[serde(default)]
    pub exploded: bool,
}

impl DrawView {
    pub fn new(id: u32, kind: ViewKind, rot: DQuat, pos: V2) -> DrawView {
        DrawView { id, name: format!("new_view_{}", id), kind, parent: None, side: None, rot, orient: None, scale: None, pos, style: LineStyle::Hidden, axes: true, section: None, detail: None, letter: String::new(), exploded: false }
    }
    /// world → view rotation used for projection
    pub fn world_to_view(&self) -> DQuat { self.rot.inverse() }
}

/// Which point of a referenced edge a created dimension attaches to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefAt { Start, End, Center, Edge }

fn half() -> f64 { 0.5 }

/// Reference of a created dimension or annotation: a model point, re-resolved through its
/// edge after the model changes (`t` is the arc-length fraction along the edge for `Edge`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DimRef { pub point: V3, pub edge: Option<EdgeRef>, pub at: RefAt, #[serde(default)] pub radius: f64, #[serde(default = "half")] pub t: f64 }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DimType { Horizontal, Vertical, Aligned, Radius, Diameter }

/// A dimension created in the drawing (driven: it measures the view geometry).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawnDim {
    pub id: u32,
    pub view: u32,
    pub ty: DimType,
    pub a: DimRef,
    pub b: Option<DimRef>,
    /// text position on the sheet
    pub label: V2,
    /// text override (the measured value replaces "@D")
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tol: Tolerance,
}

/// A model dimension shown in a view; `key` identifies it in the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShownDim { pub key: String, pub view: u32, pub label: Option<V2>, #[serde(default)] pub tol: Tolerance }

/// Geometric characteristic of a feature control frame (ISO 1101 / ASME Y14.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GChar { Straightness, Flatness, Circularity, Cylindricity, LineProfile, SurfaceProfile, Angularity, Perpendicularity, Parallelism, Position, Concentricity, Symmetry, CircularRunout, TotalRunout }

impl GChar {
    pub const ALL: [GChar; 14] = [GChar::Straightness, GChar::Flatness, GChar::Circularity, GChar::Cylindricity, GChar::LineProfile, GChar::SurfaceProfile, GChar::Angularity, GChar::Perpendicularity, GChar::Parallelism, GChar::Position, GChar::Concentricity, GChar::Symmetry, GChar::CircularRunout, GChar::TotalRunout];
    pub fn name(&self) -> &'static str {
        match self {
            GChar::Straightness => "Geradheit", GChar::Flatness => "Ebenheit", GChar::Circularity => "Rundheit", GChar::Cylindricity => "Zylinderform",
            GChar::LineProfile => "Linienform", GChar::SurfaceProfile => "Flächenform", GChar::Angularity => "Neigung",
            GChar::Perpendicularity => "Rechtwinkligkeit", GChar::Parallelism => "Parallelität", GChar::Position => "Position",
            GChar::Concentricity => "Konzentrizität", GChar::Symmetry => "Symmetrie", GChar::CircularRunout => "Lauf", GChar::TotalRunout => "Gesamtlauf",
        }
    }
    /// Form tolerances take no datum references.
    pub fn form(&self) -> bool { matches!(self, GChar::Straightness | GChar::Flatness | GChar::Circularity | GChar::Cylindricity) }
}

/// Material condition modifier: regardless of feature size, Ⓜ or Ⓛ.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MatCond { #[default] Rfs, Mmc, Lmc }

impl MatCond {
    pub const ALL: [MatCond; 3] = [MatCond::Rfs, MatCond::Mmc, MatCond::Lmc];
    pub fn name(&self) -> &'static str { match self { MatCond::Rfs => "RFS (keine)", MatCond::Mmc => "Ⓜ  Maximum-Material", MatCond::Lmc => "Ⓛ  Minimum-Material" } }
    pub fn letter(&self) -> Option<&'static str> { match self { MatCond::Rfs => None, MatCond::Mmc => Some("M"), MatCond::Lmc => Some("L") } }
}

/// A dimension an annotation is attached to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DimTarget { Shown(String), Drawn(u32) }

/// Where an annotation points: a leader to model geometry in a view, or onto a dimension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Attach { Geom(DimRef), Dim(DimTarget) }

/// Feature control frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gtol {
    pub id: u32,
    pub view: u32,
    pub ch: GChar,
    /// cylindrical (⌀) tolerance zone
    pub diameter: bool,
    pub value: f64,
    pub mat: MatCond,
    /// primary, secondary and tertiary datum references
    pub datums: Vec<(String, MatCond)>,
    pub target: Attach,
    /// lower-left corner of the frame, relative to the view position
    pub pos: V2,
}

/// Datum feature symbol (boxed letter with a filled triangle on the feature).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatumSym { pub id: u32, pub view: u32, pub letter: String, pub at: DimRef, /** box centre, relative to the view position */ pub pos: V2 }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishKind { Any, Removal, NoRemoval }

impl FinishKind {
    pub const ALL: [FinishKind; 3] = [FinishKind::Any, FinishKind::Removal, FinishKind::NoRemoval];
    pub fn name(&self) -> &'static str { match self { FinishKind::Any => "Beliebiges Verfahren", FinishKind::Removal => "Materialabtrag gefordert", FinishKind::NoRemoval => "Materialabtrag unzulässig" } }
}

/// Surface texture symbol (ISO 1302) standing on an edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finish {
    pub id: u32,
    pub view: u32,
    pub kind: FinishKind,
    /// requirement, e.g. "Ra 1.6"
    pub value: String,
    /// production method, e.g. "GROUND"
    #[serde(default)]
    pub method: String,
    pub at: DimRef,
    /// stand on the other side of the edge
    #[serde(default)]
    pub flip: bool,
}

pub const RA_VALUES: [&str; 8] = ["Ra 0.4", "Ra 0.8", "Ra 1.6", "Ra 3.2", "Ra 6.3", "Ra 12.5", "Ra 25", "Rz 6.3"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note { pub id: u32, pub pos: V2, pub text: String, #[serde(default)] pub leader: Option<V2> }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BomCol { Index, Part, Qty, Mass, File }

impl BomCol {
    pub fn title(&self) -> &'static str { match self { BomCol::Index => "POS.", BomCol::Part => "TEILENUMMER", BomCol::Qty => "MENGE", BomCol::Mass => "MASSE (kg)", BomCol::File => "DATEI" } }
    pub fn width(&self) -> f64 { match self { BomCol::Index => 14.0, BomCol::Part => 56.0, BomCol::Qty => 14.0, BomCol::Mass => 24.0, BomCol::File => 40.0 } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BomTable {
    /// top-left corner on the sheet
    pub pos: V2,
    pub columns: Vec<BomCol>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Balloon {
    pub view: u32,
    pub part: String,
    /// attachment point on the model
    pub anchor: V3,
    /// balloon centre on the sheet
    pub pos: V2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub size: SheetSize,
    pub orient: Orientation,
    /// default view scale
    pub scale: f64,
    pub views: Vec<DrawView>,
    #[serde(default)]
    pub shown: Vec<ShownDim>,
    #[serde(default)]
    pub dims: Vec<DrawnDim>,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub bom: Option<BomTable>,
    #[serde(default)]
    pub balloons: Vec<Balloon>,
    #[serde(default)]
    pub gtols: Vec<Gtol>,
    #[serde(default)]
    pub datums: Vec<DatumSym>,
    #[serde(default)]
    pub finishes: Vec<Finish>,
}

impl Sheet {
    pub fn new(size: SheetSize, orient: Orientation) -> Sheet {
        Sheet { size, orient, scale: 1.0, views: vec![], shown: vec![], dims: vec![], notes: vec![], bom: None, balloons: vec![], gtols: vec![], datums: vec![], finishes: vec![] }
    }
    /// Next datum letter not used on the sheet (I, O and Q are skipped, as in ASME Y14.5).
    pub fn next_datum(&self) -> String {
        let used: Vec<&str> = self.datums.iter().map(|d| d.letter.as_str()).collect();
        ('A'..='Z').filter(|c| !matches!(c, 'I' | 'O' | 'Q')).map(|c| c.to_string()).find(|l| !used.contains(&l.as_str())).unwrap_or("Z".into())
    }
    pub fn dims_mm(&self) -> (f64, f64) {
        let (w, h) = self.size.landscape();
        if self.orient == Orientation::Landscape { (w, h) } else { (h, w) }
    }
    pub fn view(&self, id: u32) -> Option<&DrawView> { self.views.iter().find(|v| v.id == id) }
    pub fn view_mut(&mut self, id: u32) -> Option<&mut DrawView> { self.views.iter_mut().find(|v| v.id == id) }

    /// Scale of a view: projection views use their parent's.
    pub fn view_scale(&self, id: u32) -> f64 {
        let mut cur = self.view(id);
        for _ in 0..32 {
            let Some(v) = cur else { break };
            if v.kind == ViewKind::Projection { cur = v.parent.and_then(|p| self.view(p)); continue; }
            return v.scale.unwrap_or(self.scale);
        }
        self.scale
    }

    /// The view and all views derived from it (projections and details), children last.
    pub fn family(&self, id: u32) -> Vec<u32> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let p = out[i];
            for v in &self.views { if v.parent == Some(p) && !out.contains(&v.id) { out.push(v.id); } }
            i += 1;
        }
        out
    }

    /// Frame inside the border, and the title block rectangle (min, max).
    pub fn frame(&self) -> (V2, V2) { let (w, h) = self.dims_mm(); (V2::new(10.0, 10.0), V2::new(w - 10.0, h - 10.0)) }
    pub fn title_block(&self) -> (V2, V2) { let (_, mx) = self.frame(); (V2::new(mx.x - 180.0, 10.0), V2::new(mx.x, 10.0 + 40.0)) }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingData {
    pub name: String,
    pub sheets: Vec<Sheet>,
    pub next_id: u32,
    pub projection: Projection,
    /// decimal places of dimension values
    pub decimals: usize,
    /// text height in mm
    pub text_height: f64,
    pub title: String,
    pub company: String,
    pub drawn_by: String,
    pub material: String,
    pub date: String,
    /// decimal places of tolerance values (trailing zeros dropped)
    #[serde(default = "three")]
    pub tol_decimals: usize,
    /// general tolerance note in the title block, e.g. "ISO 2768-m"
    #[serde(default)]
    pub general_tol: String,
}

fn three() -> usize { 3 }

pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

impl DrawingData {
    pub fn new(name: &str, model_name: &str, size: SheetSize, orient: Orientation) -> DrawingData {
        DrawingData {
            name: name.into(), sheets: vec![Sheet::new(size, orient)], next_id: 1, projection: Projection::Third, decimals: 2, text_height: 3.5,
            title: model_name.into(), company: "FREO".into(), drawn_by: String::new(), material: String::new(), date: today(),
            tol_decimals: 3, general_tol: "ISO 2768-m".into(),
        }
    }
    pub fn take_id(&mut self) -> u32 { let i = self.next_id; self.next_id += 1; i }
    /// Sheet holding view `id`.
    pub fn sheet_of(&self, id: u32) -> Option<usize> { self.sheets.iter().position(|s| s.view(id).is_some()) }
    pub fn view(&self, id: u32) -> Option<&DrawView> { self.sheets.iter().find_map(|s| s.view(id)) }
    pub fn fmt(&self, v: f64) -> String {
        let v = if v.abs() < 0.5 * 10f64.powi(-(self.decimals as i32)) { 0.0 } else { v };
        format!("{:.*}", self.decimals, v)
    }
    /// A tolerance deviation: signed, "0" when zero.
    pub fn fmt_dev(&self, v: f64) -> String {
        if v.abs() < 0.5 * 10f64.powi(-(self.tol_decimals as i32)) { return "0".into(); }
        let s = fmt_num(v.abs(), self.tol_decimals);
        if v > 0.0 { format!("+{}", s) } else { format!("-{}", s) }
    }
    /// A tolerance value or limit (trailing zeros dropped).
    pub fn fmt_tol(&self, v: f64) -> String { fmt_num(v, self.tol_decimals) }
    /// Next free letter for section / detail labels.
    pub fn next_letter(&self) -> String {
        let used: Vec<String> = self.sheets.iter().flat_map(|s| s.views.iter().map(|v| v.letter.clone())).collect();
        ('A'..='Z').map(|c| c.to_string()).find(|l| !used.contains(l)).unwrap_or("Z".into())
    }
}

/// Standard drawing scales, largest first.
pub const SCALES: [f64; 13] = [10.0, 5.0, 4.0, 2.0, 1.0, 0.5, 0.4, 0.25, 0.2, 0.1, 0.05, 0.02, 0.01];

pub fn scale_text(s: f64) -> String {
    if s >= 1.0 { format!("{}:1", fmt_num(s, 3)) } else { format!("1:{}", fmt_num(1.0 / s, 3)) }
}

/// Largest standard scale at which `need` (model mm) fits into `avail` (sheet mm).
pub fn fit_scale(need: V2, avail: V2) -> f64 {
    let k = (avail.x / need.x.max(1e-9)).min(avail.y / need.y.max(1e-9)) * 1.02;
    SCALES.iter().copied().find(|s| *s <= k).unwrap_or(*SCALES.last().unwrap())
}

/// Creo's default template: front, top and right projection views and a pictorial view,
/// scaled to fit the sheet. `size` is the model's bounding-box size (x, y, z). Third angle
/// puts the top view above the front and the right view to its right; first angle puts
/// them below and to the left.
pub fn standard_views(d: &mut DrawingData, sheet: usize, size: V3) {
    let proj = d.projection;
    let (fmin, fmax) = d.sheets[sheet].frame();
    let tb = d.sheets[sheet].title_block();
    let gap = 22.0;
    let (w, h, dp) = (size.x.max(1.0), size.y.max(1.0), size.z.max(1.0));
    // projected size of the pictorial view
    let iso_rot = Orient::Default.rotation().inverse();
    let (mut lo, mut hi) = (V2::splat(f64::INFINITY), V2::splat(f64::NEG_INFINITY));
    for k in 0..8 {
        let p = V3::new(if k & 1 == 0 { -w } else { w }, if k & 2 == 0 { -h } else { h }, if k & 4 == 0 { -dp } else { dp }) * 0.5;
        let q = (iso_rot * p).truncate();
        lo = lo.min(q);
        hi = hi.max(q);
    }
    let (iw, ih) = (hi.x - lo.x, hi.y - lo.y);
    // the orthographic views set the sheet scale; the pictorial view shrinks into the space left
    let avail = V2::new(fmax.x - fmin.x - 3.0 * gap, fmax.y - tb.1.y - 3.0 * gap);
    let s = fit_scale(V2::new(w + dp, h + dp), avail);
    d.sheets[sheet].scale = s;
    let iso_room = match proj {
        Projection::Third => V2::new(fmax.x - fmin.x - 3.0 * gap - w * s, fmax.y - tb.1.y - 3.0 * gap - h * s),
        Projection::First => V2::new(fmax.x - fmin.x - 4.0 * gap - (w + dp) * s, fmax.y - tb.1.y - 2.0 * gap),
    };
    let iso_s = fit_scale(V2::new(iw, ih), iso_room).min(s);
    let base = V2::new(fmin.x + gap, tb.1.y + gap);
    let front_c = match proj {
        Projection::Third => base + V2::new(w * s / 2.0, h * s / 2.0),
        Projection::First => base + V2::new(dp * s + gap + w * s / 2.0, dp * s + gap + h * s / 2.0),
    };
    let fid = d.take_id();
    let mut front = DrawView::new(fid, ViewKind::General, Orient::Front.rotation(), front_c);
    front.orient = Some(Orient::Front);
    let (top_off, side_off) = match proj {
        Projection::Third => (V2::new(0.0, h * s / 2.0 + gap + dp * s / 2.0), V2::new(w * s / 2.0 + gap + dp * s / 2.0, 0.0)),
        Projection::First => (V2::new(0.0, -(h * s / 2.0 + gap + dp * s / 2.0)), V2::new(-(w * s / 2.0 + gap + dp * s / 2.0), 0.0)),
    };
    let mut views = vec![front.clone()];
    for off in [top_off, side_off] {
        let id = d.take_id();
        let side = ProjSide::from_offset(off);
        let mut v = DrawView::new(id, ViewKind::Projection, projection_rotation(front.rot, side, proj), front_c + off);
        v.parent = Some(fid);
        v.side = Some(side);
        views.push(v);
    }
    let iso_c = match proj {
        Projection::Third => V2::new(front_c.x + w * s / 2.0 + gap + iw * iso_s / 2.0, front_c.y + h * s / 2.0 + gap + ih * iso_s / 2.0),
        Projection::First => V2::new(front_c.x + w * s / 2.0 + gap + iw * iso_s / 2.0, front_c.y + (ih * iso_s - h * s).max(0.0) / 2.0),
    };
    let iso_id = d.take_id();
    let mut iso = DrawView::new(iso_id, ViewKind::General, Orient::Default.rotation(), iso_c);
    iso.orient = Some(Orient::Default);
    if iso_s < s { iso.scale = Some(iso_s); }
    iso.style = LineStyle::NoHidden;
    iso.axes = false;
    views.push(iso);
    d.sheets[sheet].views.extend(views);
}

/// What a drawing file holds: the drawing and the model it documents (also embedded, so the
/// drawing opens when the model file has moved).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrawingFile {
    pub drawing: DrawingData,
    #[serde(default)]
    pub model_path: Option<PathBuf>,
    #[serde(default)]
    pub part: Option<ModelData>,
    #[serde(default)]
    pub assembly: Option<AssemblyData>,
}
