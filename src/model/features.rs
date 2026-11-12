//! Persistent parametric part model: features, references, parameters and relations.

use super::expr::{self, Vars};
use crate::kernel::math::*;
pub use crate::kernel::topology::{EdgeRef, FaceRef};
use crate::sketch::sketch::Sketch;
use serde::{Deserialize, Serialize};

/// A dimension: literal number or expression of parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dim(pub String);

impl Dim {
    pub fn v(x: f64) -> Dim { Dim(fmt_num(x, 6)) }
    pub fn eval(&self, vars: &Vars) -> Result<f64, String> {
        match self.0.trim().parse::<f64>() { Ok(v) => Ok(v), Err(_) => expr::eval(&self.0, vars) }
    }
    pub fn value(&self, vars: &Vars, fallback: f64) -> f64 { self.eval(vars).unwrap_or(fallback) }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlaneRef {
    Datum(u64),
    Face(FaceRef),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AxisRef {
    Axis(u64),
    WorldX,
    WorldY,
    WorldZ,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepthType { Blind, Symmetric, ThroughAll, ToNext, ToSelected }

impl DepthType {
    pub fn label(self) -> &'static str {
        match self { DepthType::Blind => "Variabel", DepthType::Symmetric => "Symmetrisch", DepthType::ThroughAll => "Durch alle", DepthType::ToNext => "Bis zur nächsten", DepthType::ToSelected => "Bis zur ausgewählten" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side2 { None, Blind, ThroughAll }

impl Side2 {
    pub fn label(self) -> &'static str {
        match self { Side2::None => "Keine", Side2::Blind => "Variabel", Side2::ThroughAll => "Durch alle" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaneKind { Front, Top, Right, Offset, Angle }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatumPlaneDef {
    pub kind: PlaneKind,
    pub reference: Option<PlaneRef>,
    pub offset: Dim,
    pub axis: Option<AxisRef>,
    pub angle: Dim,
    pub flip: bool,
}

impl DatumPlaneDef {
    pub fn builtin(kind: PlaneKind) -> Self { DatumPlaneDef { kind, reference: None, offset: Dim::v(50.0), axis: None, angle: Dim::v(45.0), flip: false } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxisKind { TwoPlanes, World, Normal }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatumAxisDef {
    pub kind: AxisKind,
    pub p1: Option<PlaneRef>,
    pub p2: Option<PlaneRef>,
    pub world: u8,
    pub x: Dim,
    pub y: Dim,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatumPointDef { pub x: Dim, pub y: Dim, pub z: Dim }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchDef { pub plane: Option<PlaneRef>, pub sketch: Sketch }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeDef {
    pub plane: Option<PlaneRef>,
    pub sketch: Sketch,
    pub depth_type: DepthType,
    pub depth: Dim,
    pub side2: Side2,
    pub depth2: Dim,
    pub flip: bool,
    pub remove: bool,
    pub taper: Dim,
    pub thin: bool,
    pub thickness: Dim,
    /// End reference for the "To Selected" depth option.
    #[serde(default)]
    pub to_ref: Option<PlaneRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevolveDef {
    pub plane: Option<PlaneRef>,
    pub sketch: Sketch,
    pub symmetric: bool,
    pub angle: Dim,
    pub flip: bool,
    pub remove: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SweepDef {
    pub plane: Option<PlaneRef>,
    pub trajectory: Sketch,
    pub section: Sketch,
    pub remove: bool,
    /// Zug-KE, Zug-Verbund (section blends into `section2` at the end) or Volumenzug-KE
    #[serde(default)]
    pub mode: SweepMode,
    #[serde(default)]
    pub section2: Sketch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SweepMode {
    #[default]
    Normal,
    /// Zug-Verbund: the section changes into a second section along the trajectory
    Blend,
    /// Volumenzug-KE: the tool keeps its orientation while it moves along the trajectory
    Volume,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendDef {
    pub plane: Option<PlaneRef>,
    pub sections: Vec<Sketch>,
    /// Distances between sections; angles (°) about the centerline of section 1 for a rotational blend.
    pub offsets: Vec<Dim>,
    pub flip: bool,
    pub remove: bool,
    #[serde(default)]
    pub rotational: bool,
}

/// Coordinate system offset and rotated (X, then Y, then Z) from BASIS.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CsysDef { pub x: Dim, pub y: Dim, pub z: Dim, pub rx: Dim, pub ry: Dim, pub rz: Dim }

/// Profile rib: an open sketch closed against the solid and thickened about the sketch plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RibDef {
    pub plane: Option<PlaneRef>,
    pub sketch: Sketch,
    pub thickness: Dim,
    /// Material on the other side of the sketched chain.
    pub flip: bool,
    /// Thickness side: 0 symmetric, 1 side 1, 2 side 2.
    pub side: u8,
}

/// Helical sweep: the profile (with a centerline as axis) sets the radius along the axis, the
/// section is swept through the axis at constant pitch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HelixDef {
    pub plane: Option<PlaneRef>,
    pub profile: Sketch,
    pub section: Sketch,
    pub pitch: Dim,
    pub left_handed: bool,
    pub remove: bool,
}

/// Cosmetic thread on a cylindrical surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadDef {
    pub face: Option<FaceRef>,
    /// Major (nominal) diameter.
    pub diameter: Dim,
    pub pitch: Dim,
    pub length: Dim,
    /// Start at the other end of the surface.
    pub flip: bool,
}

/// Datum curve through datum points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurveDef { pub points: Vec<u64>, pub spline: bool }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoleType { Simple, Counterbore, Countersink, Standard }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThreadStd { #[default] Iso, Unc }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HoleFit { Close, #[default] Medium, Free }

impl HoleFit {
    pub fn label(self) -> &'static str {
        match self { HoleFit::Close => "Fein", HoleFit::Medium => "Mittel", HoleFit::Free => "Grob" }
    }
}

fn default_size() -> String { "M6".into() }
fn yes() -> bool { true }
fn default_thread_depth() -> Dim { Dim::v(12.0) }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleDef {
    pub plane: Option<PlaneRef>,
    pub x: Dim,
    pub y: Dim,
    pub ty: HoleType,
    pub diameter: Dim,
    pub through_all: bool,
    pub depth: Dim,
    pub drill_tip: bool,
    pub cb_diameter: Dim,
    pub cb_depth: Dim,
    pub cs_diameter: Dim,
    pub cs_angle: Dim,
    pub flip: bool,
    #[serde(default)]
    pub standard: ThreadStd,
    #[serde(default = "default_size")]
    pub size: String,
    #[serde(default = "yes")]
    pub tapped: bool,
    #[serde(default)]
    pub fit: HoleFit,
    #[serde(default = "default_thread_depth")]
    pub thread_depth: Dim,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoundSet {
    pub edges: Vec<EdgeRef>,
    pub radius: Dim,
    /// Variable radius: `radius` at the edge start, `radius_end` at its end.
    #[serde(default)]
    pub variable: bool,
    #[serde(default = "default_radius")]
    pub radius_end: Dim,
    /// Form: circular arc or one of the curvature-continuous corner shapes. The radius is that of
    /// the arc; the smooth shapes pass through its middle and start earlier on both surfaces.
    #[serde(default)]
    pub shape: crate::kernel::corner::CornerShape,
}

fn default_radius() -> Dim { Dim::v(3.0) }

impl RoundSet {
    pub fn new(r: f64) -> RoundSet { RoundSet { edges: vec![], radius: Dim::v(r), variable: false, radius_end: Dim::v(r), shape: Default::default() } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "RoundRepr")]
pub struct RoundDef { pub sets: Vec<RoundSet> }

/// Accepts rounds saved before round sets existed (`edges` + `radius`), and never yields an
/// empty set list.
#[derive(Deserialize)]
#[serde(untagged)]
enum RoundRepr { Sets { sets: Vec<RoundSet> }, Single { edges: Vec<EdgeRef>, radius: Dim } }

impl From<RoundRepr> for RoundDef {
    fn from(r: RoundRepr) -> RoundDef {
        let mut sets = match r {
            RoundRepr::Sets { sets } => sets,
            RoundRepr::Single { edges, radius } => vec![RoundSet { edges, radius, variable: false, radius_end: default_radius(), shape: Default::default() }],
        };
        if sets.is_empty() { sets.push(RoundSet::new(3.0)); }
        RoundDef { sets }
    }
}

impl RoundDef {
    pub fn edge_count(&self) -> usize { self.sets.iter().map(|s| s.edges.len()).sum() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChamferKind { DxD, D1xD2, AnglexD }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChamferDef { pub edges: Vec<EdgeRef>, pub kind: ChamferKind, pub d1: Dim, pub d2: Dim, pub angle: Dim, /** Eckfase: cut the corner where the edges meet */ #[serde(default)] pub corner: bool }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellDef { pub faces: Vec<FaceRef>, pub thickness: Dim }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftDef { pub faces: Vec<FaceRef>, pub hinge: Option<PlaneRef>, pub angle: Dim, pub flip: bool, /** Variable Zugschräge: angle at the far end of the faces */ #[serde(default)] pub angle2: Option<Dim> }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternKind { Direction, Axis, Fill, Curve, Table }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FillGrid { #[default] Square, Diamond, Triangle }

impl FillGrid {
    pub fn label(self) -> &'static str {
        match self { FillGrid::Square => "Quadrat", FillGrid::Diamond => "Raute", FillGrid::Triangle => "Dreieck" }
    }
}

fn d20() -> Dim { Dim::v(20.0) }
fn d0() -> Dim { Dim::v(0.0) }
fn d5() -> Dim { Dim::v(5.0) }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternDef {
    pub features: Vec<u64>,
    pub kind: PatternKind,
    pub dir1: AxisRef,
    pub count1: Dim,
    pub spacing1: Dim,
    pub use_dir2: bool,
    pub dir2: AxisRef,
    pub count2: Dim,
    pub spacing2: Dim,
    pub axis: AxisRef,
    pub axis_count: Dim,
    pub axis_angle: Dim,
    /// Fill region / curve sketch for fill and curve patterns.
    #[serde(default)]
    pub sketch_plane: Option<PlaneRef>,
    #[serde(default)]
    pub sketch: Sketch,
    /// Mustertabelle: one translation (x, y, z) per instance
    #[serde(default)]
    pub table: Vec<[f64; 3]>,
    #[serde(default)]
    pub fill_grid: FillGrid,
    #[serde(default = "d20")]
    pub fill_spacing: Dim,
    #[serde(default = "d0")]
    pub fill_margin: Dim,
    #[serde(default = "d0")]
    pub fill_rotation: Dim,
    #[serde(default = "d5")]
    pub curve_count: Dim,
    #[serde(default)]
    pub curve_by_spacing: bool,
    #[serde(default = "d20")]
    pub curve_spacing: Dim,
    #[serde(default)]
    pub curve_follow: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MirrorDef { pub plane: Option<PlaneRef>, pub features: Vec<u64> }

/// One face of imported geometry: vertices (x, y, z, nx, ny, nz) in mm and triangles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportFace { pub v: Vec<[f32; 6]>, pub t: Vec<[u32; 3]> }

/// Geometry read from a neutral file (STEP, STL, OBJ), stored in the part like Creo's import feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportDef { pub source: String, pub format: String, pub faces: Vec<ImportFace> }

/// Surface (Sammelfläche) features of the Formen / Editieren / Flächen groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfOp { Fill, Offset, Thicken, Solidify, Merge, Trim, Intersect, Project, Extend, Split, Copy, Replace, Boundary, Style, Freestyle }

impl SurfOp {
    pub fn name(&self) -> &'static str {
        match self {
            SurfOp::Fill => "Füllen", SurfOp::Offset => "Versatz", SurfOp::Thicken => "Aufdicken", SurfOp::Solidify => "Volumenkörper erzeugen",
            SurfOp::Merge => "Zusammenführen", SurfOp::Trim => "Trimmen", SurfOp::Intersect => "Schnittpunkt", SurfOp::Project => "Projizieren",
            SurfOp::Extend => "Verlängern", SurfOp::Split => "Teilen", SurfOp::Copy => "Bearbeitbare Kopie", SurfOp::Replace => "Ersetzen",
            SurfOp::Boundary => "Berandungsverbund", SurfOp::Style => "Stil", SurfOp::Freestyle => "Freistil",
        }
    }
    /// operations whose result is a quilt (the others change the solid or make curves)
    pub fn makes_quilt(&self) -> bool { !matches!(self, SurfOp::Thicken | SurfOp::Solidify | SurfOp::Replace | SurfOp::Intersect | SurfOp::Project) }
    pub fn needs_sketch(&self) -> bool { matches!(self, SurfOp::Fill | SurfOp::Project) }
}

fn d_five() -> Dim { Dim::v(5.0) }
fn style_size() -> [f64; 3] { [80.0, 60.0, 40.0] }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceDef {
    pub op: SurfOp,
    /// sketch plane (Füllen, Projizieren, Stil, Freistil) or cutting plane (Trimmen, Schnittpunkt, Teilen, Ersetzen)
    #[serde(default)]
    pub plane: Option<PlaneRef>,
    #[serde(default)]
    pub sketch: Sketch,
    /// input quilts or curves (feature ids)
    #[serde(default)]
    pub inputs: Vec<u64>,
    /// solid faces (Versatz, Bearbeitbare Kopie, Ersetzen)
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    #[serde(default = "d_five")]
    pub distance: Dim,
    #[serde(default)]
    pub flip: bool,
    /// Aufdicken / Volumenkörper erzeugen: remove material
    #[serde(default)]
    pub remove: bool,
    /// Stil: 4 × 4 control point heights
    #[serde(default)]
    pub heights: Vec<f64>,
    /// Stil: width, length; Freistil: box size
    #[serde(default = "style_size")]
    pub size: [f64; 3],
    /// Freistil: subdivision level
    #[serde(default)]
    pub level: u32,
}

/// Körper features: a new body, a Boolean between bodies, or a body split by a plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyOp { New, Union, Subtract, Intersect, Split }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyDef {
    pub op: BodyOp,
    /// body acted on (0 = Körper 1, else the id of the feature that made the body)
    #[serde(default)]
    pub target: u64,
    /// body combined into the target (Boolesche Operationen)
    #[serde(default)]
    pub tool: u64,
    /// Körper teilen: splitting plane
    #[serde(default)]
    pub plane: Option<PlaneRef>,
    #[serde(default)]
    pub flip: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    DatumPlane(DatumPlaneDef),
    DatumAxis(DatumAxisDef),
    DatumPoint(DatumPointDef),
    Csys,
    Sketch(SketchDef),
    Extrude(ExtrudeDef),
    Revolve(RevolveDef),
    Sweep(SweepDef),
    Blend(BlendDef),
    Hole(HoleDef),
    Round(RoundDef),
    Chamfer(ChamferDef),
    Shell(ShellDef),
    Draft(DraftDef),
    Pattern(PatternDef),
    Mirror(MirrorDef),
    Import(ImportDef),
    DatumCsys(CsysDef),
    Rib(RibDef),
    Helix(HelixDef),
    Thread(ThreadDef),
    Curve(CurveDef),
    Surface(SurfaceDef),
    Body(BodyDef),
}

impl FeatureKind {
    pub fn new_extrude() -> Self {
        FeatureKind::Extrude(ExtrudeDef { plane: None, sketch: Sketch::default(), depth_type: DepthType::Blind, depth: Dim::v(20.0), side2: Side2::None, depth2: Dim::v(10.0), flip: false, remove: false, taper: Dim::v(0.0), thin: false, thickness: Dim::v(2.0), to_ref: None })
    }
    pub fn new_revolve() -> Self { FeatureKind::Revolve(RevolveDef { plane: None, sketch: Sketch::default(), symmetric: false, angle: Dim::v(360.0), flip: false, remove: false }) }
    pub fn new_sweep() -> Self { FeatureKind::Sweep(SweepDef { plane: None, trajectory: Sketch::default(), section: Sketch::default(), remove: false, mode: SweepMode::Normal, section2: Sketch::default() }) }
    pub fn new_blend() -> Self { FeatureKind::Blend(BlendDef { plane: None, sections: vec![Sketch::default()], offsets: vec![], flip: false, remove: false, rotational: false }) }
    pub fn new_rotational_blend() -> Self { FeatureKind::Blend(BlendDef { plane: None, sections: vec![Sketch::default()], offsets: vec![], flip: false, remove: false, rotational: true }) }
    pub fn new_csys() -> Self { let z = || Dim::v(0.0); FeatureKind::DatumCsys(CsysDef { x: z(), y: z(), z: z(), rx: z(), ry: z(), rz: z() }) }
    pub fn new_rib() -> Self { FeatureKind::Rib(RibDef { plane: None, sketch: Sketch::default(), thickness: Dim::v(4.0), flip: false, side: 0 }) }
    pub fn new_helix() -> Self { FeatureKind::Helix(HelixDef { plane: None, profile: Sketch::default(), section: Sketch::default(), pitch: Dim::v(5.0), left_handed: false, remove: false }) }
    pub fn new_thread() -> Self { FeatureKind::Thread(ThreadDef { face: None, diameter: Dim::v(10.0), pitch: Dim::v(1.5), length: Dim::v(10.0), flip: false }) }
    pub fn new_surface(op: SurfOp) -> Self {
        FeatureKind::Surface(SurfaceDef { op, plane: None, sketch: Sketch::default(), inputs: vec![], faces: vec![], distance: Dim::v(if op == SurfOp::Extend { 10.0 } else { 5.0 }), flip: false, remove: false, heights: vec![0.0; 16], size: style_size(), level: 2 })
    }
    pub fn new_body(op: BodyOp) -> Self { FeatureKind::Body(BodyDef { op, target: 0, tool: 0, plane: None, flip: false }) }
    pub fn new_curve() -> Self { FeatureKind::Curve(CurveDef { points: vec![], spline: true }) }
    pub fn new_sketch() -> Self { FeatureKind::Sketch(SketchDef { plane: None, sketch: Sketch::default() }) }
    pub fn new_hole() -> Self {
        FeatureKind::Hole(HoleDef { plane: None, x: Dim::v(0.0), y: Dim::v(0.0), ty: HoleType::Simple, diameter: Dim::v(10.0), through_all: false, depth: Dim::v(20.0), drill_tip: true, cb_diameter: Dim::v(18.0), cb_depth: Dim::v(6.0), cs_diameter: Dim::v(20.0), cs_angle: Dim::v(90.0), flip: false, standard: ThreadStd::Iso, size: "M6".into(), tapped: true, fit: HoleFit::Medium, thread_depth: Dim::v(12.0) })
    }
    pub fn new_round() -> Self { FeatureKind::Round(RoundDef { sets: vec![RoundSet::new(3.0)] }) }
    pub fn new_chamfer() -> Self { FeatureKind::Chamfer(ChamferDef { edges: vec![], kind: ChamferKind::DxD, d1: Dim::v(2.0), d2: Dim::v(2.0), angle: Dim::v(45.0), corner: false }) }
    pub fn new_shell() -> Self { FeatureKind::Shell(ShellDef { faces: vec![], thickness: Dim::v(2.0) }) }
    pub fn new_draft() -> Self { FeatureKind::Draft(DraftDef { faces: vec![], hinge: None, angle: Dim::v(5.0), flip: false, angle2: None }) }
    pub fn new_pattern() -> Self {
        FeatureKind::Pattern(PatternDef { features: vec![], kind: PatternKind::Direction, dir1: AxisRef::WorldX, count1: Dim::v(3.0), spacing1: Dim::v(20.0), use_dir2: false, dir2: AxisRef::WorldZ, count2: Dim::v(2.0), spacing2: Dim::v(20.0), axis: AxisRef::WorldY, axis_count: Dim::v(6.0), axis_angle: Dim::v(60.0),
            sketch_plane: None, sketch: Sketch::default(), fill_grid: FillGrid::Square, fill_spacing: Dim::v(20.0), fill_margin: Dim::v(0.0), fill_rotation: Dim::v(0.0),
            curve_count: Dim::v(5.0), curve_by_spacing: false, curve_spacing: Dim::v(20.0), curve_follow: false, table: vec![[20.0, 0.0, 0.0], [40.0, 0.0, 0.0]] })
    }
    pub fn new_mirror() -> Self { FeatureKind::Mirror(MirrorDef { plane: None, features: vec![] }) }
    pub fn new_plane() -> Self { FeatureKind::DatumPlane(DatumPlaneDef::builtin(PlaneKind::Offset)) }
    pub fn new_axis() -> Self { FeatureKind::DatumAxis(DatumAxisDef { kind: AxisKind::TwoPlanes, p1: None, p2: None, world: 1, x: Dim::v(0.0), y: Dim::v(0.0) }) }
    pub fn new_point() -> Self { FeatureKind::DatumPoint(DatumPointDef { x: Dim::v(0.0), y: Dim::v(0.0), z: Dim::v(0.0) }) }

    pub fn type_name(&self) -> &'static str {
        match self {
            FeatureKind::DatumPlane(_) => "Bezugsebene",
            FeatureKind::DatumAxis(_) => "Bezugsachse",
            FeatureKind::DatumPoint(_) => "Bezugspunkt",
            FeatureKind::Csys => "Koordinatensystem",
            FeatureKind::Sketch(_) => "Skizze",
            FeatureKind::Extrude(d) => if d.remove { "Profil (Schnitt)" } else { "Profil" },
            FeatureKind::Revolve(d) => if d.remove { "Drehen (Schnitt)" } else { "Drehen" },
            FeatureKind::Sweep(d) => if d.remove { "Zug (Schnitt)" } else { "Zug" },
            FeatureKind::Blend(d) => match (d.rotational, d.remove) { (false, false) => "Verbund", (false, true) => "Verbund (Schnitt)", (true, false) => "Rotationsverbund", (true, true) => "Rotationsverbund (Schnitt)" },
            FeatureKind::Hole(_) => "Bohrung",
            FeatureKind::Round(_) => "Rundung",
            FeatureKind::Chamfer(_) => "Fase",
            FeatureKind::Shell(_) => "Schale",
            FeatureKind::Draft(_) => "Formschräge",
            FeatureKind::Pattern(_) => "Muster",
            FeatureKind::Mirror(_) => "Spiegeln",
            FeatureKind::Import(_) => "Import-KE",
            FeatureKind::DatumCsys(_) => "Koordinatensystem",
            FeatureKind::Rib(_) => "Profilrippe",
            FeatureKind::Helix(d) => if d.remove { "Spiralzug (Schnitt)" } else { "Spiralzug" },
            FeatureKind::Thread(_) => "Kosmetisches Gewinde",
            FeatureKind::Curve(_) => "Bezugskurve",
            FeatureKind::Surface(d) => d.op.name(),
            FeatureKind::Body(d) => match d.op { BodyOp::New => "Körper", BodyOp::Split => "Körper teilen", _ => "Boolesche Operation" },
        }
    }

    pub fn prefix(&self) -> &'static str {
        match self {
            FeatureKind::DatumPlane(_) => "DTM",
            FeatureKind::DatumAxis(_) => "A_",
            FeatureKind::DatumPoint(_) => "PNT",
            FeatureKind::Csys => "CS",
            FeatureKind::Sketch(_) => "Skizze ",
            FeatureKind::Extrude(_) => "Profil ",
            FeatureKind::Revolve(_) => "Drehen ",
            FeatureKind::Sweep(_) => "Zug ",
            FeatureKind::Blend(d) => if d.rotational { "Rotationsverbund " } else { "Verbund " },
            FeatureKind::Hole(_) => "Bohrung ",
            FeatureKind::Round(_) => "Rundung ",
            FeatureKind::Chamfer(_) => "Fase ",
            FeatureKind::Shell(_) => "Schale ",
            FeatureKind::Draft(_) => "Formschräge ",
            FeatureKind::Pattern(_) => "Muster ",
            FeatureKind::Mirror(_) => "Spiegeln ",
            FeatureKind::Import(_) => "Import ",
            FeatureKind::DatumCsys(_) => "CS",
            FeatureKind::Rib(_) => "Rippe ",
            FeatureKind::Helix(_) => "Spiralzug ",
            FeatureKind::Thread(_) => "Gewinde ",
            FeatureKind::Curve(_) => "Kurve ",
            FeatureKind::Surface(d) => match d.op { SurfOp::Fill => "Füllen ", SurfOp::Offset => "Versatz ", SurfOp::Thicken => "Aufdicken ", SurfOp::Solidify => "Volumenkörper erzeugen ", SurfOp::Merge => "Zusammenführen ", SurfOp::Trim => "Trimmen ", SurfOp::Intersect => "Schnittpunkt ", SurfOp::Project => "Projizieren ", SurfOp::Extend => "Verlängern ", SurfOp::Split => "Teilen ", SurfOp::Copy => "Kopie ", SurfOp::Replace => "Ersetzen ", SurfOp::Boundary => "Berandungsverbund ", SurfOp::Style => "Stil ", SurfOp::Freestyle => "Freistil " },
            FeatureKind::Body(d) => match d.op { BodyOp::New => "Körper ", BodyOp::Split => "Körper teilen ", _ => "Boolesche Operation " },
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            FeatureKind::DatumPlane(_) => "▱",
            FeatureKind::DatumAxis(_) => "╱",
            FeatureKind::DatumPoint(_) => "✕",
            FeatureKind::Csys => "⌖",
            FeatureKind::Sketch(_) => "✎",
            FeatureKind::Extrude(_) => "▣",
            FeatureKind::Revolve(_) => "⟳",
            FeatureKind::Sweep(_) => "∿",
            FeatureKind::Blend(_) => "⧈",
            FeatureKind::Hole(_) => "◎",
            FeatureKind::Round(_) => "◜",
            FeatureKind::Chamfer(_) => "◸",
            FeatureKind::Shell(_) => "☐",
            FeatureKind::Draft(_) => "⏢",
            FeatureKind::Pattern(_) => "⁙",
            FeatureKind::Mirror(_) => "⇋",
            FeatureKind::Import(_) => "⇩",
            FeatureKind::DatumCsys(_) => "⌖",
            FeatureKind::Rib(_) => "◢",
            FeatureKind::Helix(_) => "⌇",
            FeatureKind::Thread(_) => "≋",
            FeatureKind::Curve(_) => "∼",
            FeatureKind::Surface(_) => "◇",
            FeatureKind::Body(_) => "■",
        }
    }

    pub fn produces_tool(&self) -> bool {
        matches!(self, FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Sweep(_) | FeatureKind::Blend(_) | FeatureKind::Hole(_) | FeatureKind::Rib(_) | FeatureKind::Helix(_))
    }

    pub fn is_datum(&self) -> bool {
        matches!(self, FeatureKind::DatumPlane(_) | FeatureKind::DatumAxis(_) | FeatureKind::DatumPoint(_) | FeatureKind::Csys | FeatureKind::Sketch(_) | FeatureKind::DatumCsys(_) | FeatureKind::Curve(_))
    }

    /// Profil and Drehen can also take an existing sketch as their section.
    pub fn accepts_sketch(&self) -> bool { matches!(self, FeatureKind::Extrude(_) | FeatureKind::Revolve(_)) }

    pub fn has_sketch(&self) -> bool {
        matches!(self, FeatureKind::Sketch(_) | FeatureKind::Extrude(_) | FeatureKind::Revolve(_) | FeatureKind::Sweep(_) | FeatureKind::Blend(_) | FeatureKind::Rib(_) | FeatureKind::Helix(_))
            || matches!(self, FeatureKind::Surface(d) if d.op.needs_sketch())
    }

    pub fn sketch_plane(&self) -> Option<PlaneRef> {
        match self {
            FeatureKind::Extrude(d) => d.plane,
            FeatureKind::Revolve(d) => d.plane,
            FeatureKind::Sweep(d) => d.plane,
            FeatureKind::Blend(d) => d.plane,
            FeatureKind::Sketch(d) => d.plane,
            FeatureKind::Rib(d) => d.plane,
            FeatureKind::Helix(d) => d.plane,
            FeatureKind::Pattern(d) if matches!(d.kind, PatternKind::Fill | PatternKind::Curve) => d.sketch_plane,
            FeatureKind::Surface(d) if d.op.needs_sketch() => d.plane,
            _ => None,
        }
    }

    pub fn set_sketch_plane(&mut self, r: PlaneRef) {
        match self {
            FeatureKind::Extrude(d) => { d.plane = Some(r); if matches!(r, PlaneRef::Face(_)) && d.remove { d.flip = true; } }
            FeatureKind::Revolve(d) => d.plane = Some(r),
            FeatureKind::Sweep(d) => d.plane = Some(r),
            FeatureKind::Blend(d) => d.plane = Some(r),
            FeatureKind::Sketch(d) => d.plane = Some(r),
            FeatureKind::Rib(d) => d.plane = Some(r),
            FeatureKind::Helix(d) => d.plane = Some(r),
            FeatureKind::Pattern(d) => d.sketch_plane = Some(r),
            FeatureKind::Surface(d) => d.plane = Some(r),
            FeatureKind::Body(d) => d.plane = Some(r),
            _ => {}
        }
    }

    /// Feature ids this feature depends on.
    pub fn references(&self) -> Vec<u64> {
        let mut out = vec![];
        let plane = |p: &Option<PlaneRef>, out: &mut Vec<u64>| if let Some(PlaneRef::Datum(id)) = p { out.push(*id) };
        let axis = |a: &AxisRef, out: &mut Vec<u64>| if let AxisRef::Axis(id) = a { out.push(if *id >= 1000 { id / 1000 } else { *id }) };
        match self {
            FeatureKind::DatumPlane(d) => { plane(&d.reference, &mut out); if let Some(a) = &d.axis { axis(a, &mut out) } }
            FeatureKind::DatumAxis(d) => { plane(&d.p1, &mut out); plane(&d.p2, &mut out); }
            FeatureKind::Sketch(d) => plane(&d.plane, &mut out),
            FeatureKind::Extrude(d) => plane(&d.plane, &mut out),
            FeatureKind::Revolve(d) => plane(&d.plane, &mut out),
            FeatureKind::Sweep(d) => plane(&d.plane, &mut out),
            FeatureKind::Blend(d) => plane(&d.plane, &mut out),
            FeatureKind::Hole(d) => plane(&d.plane, &mut out),
            FeatureKind::Draft(d) => plane(&d.hinge, &mut out),
            FeatureKind::Pattern(d) => { out.extend(&d.features); axis(&d.dir1, &mut out); axis(&d.dir2, &mut out); axis(&d.axis, &mut out); plane(&d.sketch_plane, &mut out); }
            FeatureKind::Mirror(d) => { plane(&d.plane, &mut out); out.extend(&d.features); }
            FeatureKind::Rib(d) => plane(&d.plane, &mut out),
            FeatureKind::Helix(d) => plane(&d.plane, &mut out),
            FeatureKind::Curve(d) => out.extend(&d.points),
            FeatureKind::Surface(d) => { plane(&d.plane, &mut out); out.extend(&d.inputs); }
            FeatureKind::Body(d) => { plane(&d.plane, &mut out); out.extend([d.target, d.tool].into_iter().filter(|b| *b != 0)); }
            _ => {}
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: u64,
    pub name: String,
    pub suppressed: bool,
    pub hidden: bool,
    pub kind: FeatureKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parameter { pub name: String, pub expr: String, pub note: String }

/// A family table column: a parameter value or whether a feature is present.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FamilyColumn { Parameter(String), Feature(u64) }

/// One row of the family table; a value of `*` keeps the generic's value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FamilyInstance { pub name: String, pub values: Vec<String> }

/// Creo's family table: instances of the generic part that differ in parameters and features.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FamilyTable {
    pub columns: Vec<FamilyColumn>,
    pub instances: Vec<FamilyInstance>,
    /// The instance shown in the graphics window (None = generic).
    #[serde(default)]
    pub active: Option<usize>,
}

impl FamilyTable {
    pub fn is_empty(&self) -> bool { self.instances.is_empty() }
}

/// True for the family table's "feature present" values (Creo's Y / German J).
pub fn family_yes(v: &str) -> bool { matches!(v.trim().to_uppercase().as_str(), "J" | "Y" | "JA" | "YES" | "1") }

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Appearance { pub color: [f32; 3], pub opacity: f32, pub shininess: f32 }

impl Default for Appearance { fn default() -> Self { Appearance { color: [0.74, 0.76, 0.79], opacity: 1.0, shininess: 0.45 } } }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelData {
    pub name: String,
    pub features: Vec<Feature>,
    pub parameters: Vec<Parameter>,
    pub relations: String,
    pub next_id: u64,
    pub appearance: Appearance,
    /// kg/mm³
    pub density: f64,
    pub insert_index: Option<usize>,
    #[serde(default)]
    pub family: FamilyTable,
    /// Named orientations saved in the part (Ansicht ▸ Neu orientieren ▸ Gespeicherte Orientierungen).
    #[serde(default = "default_views")]
    pub views: Vec<SavedView>,
    /// Kombinations-Zustände (Anmerkungen erstellen): orientation and display kept together.
    #[serde(default)]
    pub combos: Vec<ComboState>,
    /// Standard Alle as last updated (None: standard orientation, fitted)
    #[serde(default)]
    pub combo_default: Option<ComboState>,
    /// 3D annotations (Anmerkungen erstellen) and the Anmerkungs-KEs grouping them.
    #[serde(default)]
    pub annots: Vec<Annot3>,
    #[serde(default)]
    pub annot_groups: Vec<AnnotGroup>,
    /// body each feature adds to (missing = Körper 1); new features go into `default_body`
    #[serde(default)]
    pub feature_body: std::collections::HashMap<u64, u64>,
    #[serde(default)]
    pub default_body: u64,
    /// Komponentenschnittstelle: placement references offered when the part is assembled
    #[serde(default)]
    pub interface: Vec<IfaceItem>,
    /// Live-Simulation studies and the one being worked on
    #[serde(default)]
    pub sims: Vec<super::sim::Study>,
    #[serde(default)]
    pub sim_active: usize,
}

/// One reference of the component interface and the constraint it takes part in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IfaceItem { pub ty: super::assembly::AsmCType, pub r: IfaceRef, pub name: String }

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum IfaceRef { Plane(u64), Axis(u64), Point(u64), Surface { face: FaceRef, planar: bool } }

/// What a 3D annotation is attached to, kept by geometry so it survives regeneration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ARef {
    /// a face, found again by a point on it and its normal
    Face { p: glam::DVec3, n: glam::DVec3 },
    /// an edge, found again as the one passing nearest this point
    Edge { p: glam::DVec3 },
    Plane(u64),
    Axis(u64),
    Point(u64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AnnotKind {
    /// driven dimension (Bemaßung) or reference dimension (Referenzbemaßung), optional ± tolerance (upper, lower)
    Dim { reference: bool, tol: Option<(f64, f64)> },
    /// ordinate dimension measured from a base plane
    Ordinate { base: ARef },
    Note { text: String },
    Finish { kind: super::drawing::FinishKind, value: String },
    Datum { letter: String },
    DatumTarget { label: String },
    Gtol { ch: super::drawing::GChar, diameter: bool, value: f64, mat: super::drawing::MatCond, datums: Vec<String> },
}

/// A 3D annotation: its kind, the geometry it refers to and where its text sits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annot3 {
    pub id: u64,
    pub kind: AnnotKind,
    pub refs: Vec<ARef>,
    pub pos: glam::DVec3,
    /// annotation plane it was placed on
    pub plane: String,
    /// Anmerkungs-KE it belongs to
    pub group: Option<u64>,
}

/// Anmerkungs-KE (or Bezugsziel-Anmerkungs-KE): a named group of annotations in the model tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnnotGroup { pub id: u64, pub name: String, pub datum_target: bool }

/// A Kombinations-Zustand: the view (orientation, centre, zoom) and display settings it restores.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComboState {
    pub name: String,
    pub rot: glam::DQuat,
    pub target: glam::DVec3,
    pub scale: f64,
    /// index into the app's display styles
    pub style: u8,
    pub transparent: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedView { pub name: String, pub rot: glam::DQuat }

/// The HTL start part ships with the saved orientation GOODVIEW (a trimetric view).
pub fn default_views() -> Vec<SavedView> {
    vec![SavedView { name: "GOODVIEW".into(), rot: glam::DQuat::from_axis_angle(glam::DVec3::Y, 30f64.to_radians()) * glam::DQuat::from_axis_angle(glam::DVec3::X, -(22f64.to_radians())) }]
}

pub const FRONT_ID: u64 = 1;
pub const TOP_ID: u64 = 2;
pub const RIGHT_ID: u64 = 3;
pub const CSYS_ID: u64 = 4;

impl ModelData {
    pub fn new_part(name: &str) -> ModelData {
        let f = |id, name: &str, kind| Feature { id, name: name.into(), suppressed: false, hidden: false, kind };
        ModelData {
            name: name.into(),
            features: vec![
                // names of the German Creo start part (HTL template)
                f(CSYS_ID, "BASIS", FeatureKind::Csys),
                f(RIGHT_ID, "RECHTS", FeatureKind::DatumPlane(DatumPlaneDef::builtin(PlaneKind::Right))),
                f(TOP_ID, "OBEN", FeatureKind::DatumPlane(DatumPlaneDef::builtin(PlaneKind::Top))),
                f(FRONT_ID, "VORNE", FeatureKind::DatumPlane(DatumPlaneDef::builtin(PlaneKind::Front))),
            ],
            parameters: vec![],
            relations: String::new(),
            next_id: 10,
            appearance: Appearance::default(),
            density: 7.85e-6,
            insert_index: None,
            family: FamilyTable::default(),
            views: default_views(),
            combos: vec![],
            combo_default: None,
            annots: vec![],
            annot_groups: vec![],
            feature_body: Default::default(),
            default_body: 0,
            interface: vec![],
            sims: vec![],
            sim_active: 0,
        }
    }

    /// A new part from the start-part template: the default datums plus the axes A_X, A_Y, A_Z
    /// through BASIS, like the HTL Creo template.
    pub fn new_part_template(name: &str) -> ModelData {
        let mut m = ModelData::new_part(name);
        for (i, n) in ["A_X", "A_Y", "A_Z"].iter().enumerate() {
            let kind = FeatureKind::DatumAxis(DatumAxisDef { kind: AxisKind::World, p1: None, p2: None, world: i as u8, x: Dim::v(0.0), y: Dim::v(0.0) });
            m.features.push(Feature { id: 5 + i as u64, name: n.to_string(), suppressed: false, hidden: false, kind });
        }
        m
    }

    pub fn feature(&self, id: u64) -> Option<&Feature> { self.features.iter().find(|f| f.id == id) }
    pub fn index_of(&self, id: u64) -> Option<usize> { self.features.iter().position(|f| f.id == id) }

    pub fn next_name(&self, kind: &FeatureKind) -> String {
        let p = kind.prefix();
        (1..).map(|n| format!("{}{}", p, n)).find(|c| !self.features.iter().any(|f| &f.name == c)).unwrap()
    }

    /// The part as instance `i` of its family table: parameters and feature suppression taken
    /// from the instance row, without a family table of its own.
    pub fn instance(&self, i: usize) -> Result<ModelData, String> {
        let inst = self.family.instances.get(i).ok_or("Die Instanz existiert nicht.")?;
        let mut m = self.clone();
        m.family = FamilyTable::default();
        m.name = inst.name.clone();
        for (col, v) in self.family.columns.iter().zip(&inst.values) {
            let v = v.trim();
            if v.is_empty() || v == "*" { continue; }
            match col {
                FamilyColumn::Parameter(name) => {
                    let p = m.parameters.iter_mut().find(|p| p.name.eq_ignore_ascii_case(name)).ok_or(format!("Parameter {} fehlt.", name))?;
                    p.expr = v.to_string();
                }
                FamilyColumn::Feature(id) => if let Some(f) = m.features.iter_mut().find(|f| f.id == *id) { f.suppressed = !family_yes(v); },
            }
        }
        Ok(m)
    }

    /// The model as shown: the active family table instance, or the generic.
    pub fn shown(&self) -> std::borrow::Cow<'_, ModelData> {
        match self.family.active.and_then(|i| self.instance(i).ok()) {
            Some(m) => std::borrow::Cow::Owned(m),
            None => std::borrow::Cow::Borrowed(self),
        }
    }

    /// Evaluate parameters, then relations, into a variable table.
    pub fn evaluate_parameters(&self) -> (Vars, Vec<String>) {
        let mut vars = Vars::new();
        let mut errors = vec![];
        for p in &self.parameters {
            match expr::eval(&p.expr, &vars) {
                Ok(v) => { vars.insert(p.name.to_uppercase(), v); }
                Err(e) => errors.push(format!("{}: {}", p.name, e)),
            }
        }
        for (ln, raw) in self.relations.lines().enumerate() {
            let line = raw.split("/*").next().unwrap_or("").trim();
            if line.is_empty() { continue; }
            let parts: Vec<&str> = line.splitn(2, '=').map(|s| s.trim()).collect();
            if parts.len() != 2 || parts[0].is_empty() { errors.push(format!("Beziehungszeile {}: NAME = Ausdruck erwartet", ln + 1)); continue; }
            match expr::eval(parts[1], &vars) {
                Ok(v) => { vars.insert(parts[0].to_uppercase(), v); }
                Err(e) => errors.push(format!("Beziehungszeile {}: {}", ln + 1, e)),
            }
        }
        (vars, errors)
    }
}
