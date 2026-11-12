//! Live-Simulation studies stored with the part: study type, material, constraints and loads.
//! Units follow the model: mm, N, MPa, t/mm³ for the mechanics; W, °C for heat; m/s for flow.

use crate::kernel::topology::{FaceRef, Topology};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StudyKind { Structural, Thermal, Modal, Fluid }

impl StudyKind {
    pub const ALL: [StudyKind; 4] = [StudyKind::Structural, StudyKind::Thermal, StudyKind::Modal, StudyKind::Fluid];
    pub fn title(self) -> &'static str {
        match self {
            StudyKind::Structural => "Struktur-Simulationsstudie", StudyKind::Thermal => "Thermische Simulationsstudie",
            StudyKind::Modal => "Modal-Simulationsstudie", StudyKind::Fluid => "Fluidsimulationsstudie",
        }
    }
    pub fn text(self) -> &'static str {
        match self {
            StudyKind::Structural => "Spannung und Deformation in einem Volumenmodell auswerten",
            StudyKind::Thermal => "Wärmeleitfähigkeit des Volumenmodells auswerten",
            StudyKind::Modal => "Dynamische Reaktion eines Volumenmodells auswerten",
            StudyKind::Fluid => "Leistung eines internen oder externen Fluidströmungssystems auswerten",
        }
    }
    pub fn short(self) -> &'static str {
        match self { StudyKind::Structural => "Struktur", StudyKind::Thermal => "Thermisch", StudyKind::Modal => "Modal", StudyKind::Fluid => "Fluid" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Fixed, Displacement, Frictionless, Symmetry,
    Force, Moment, Pressure, Gravity, Centrifugal, LinAccel,
    Temperature, Convection, HeatFlow, HeatFlux,
    Inlet, Outlet, Wind,
}

impl ItemKind {
    pub fn name(self) -> &'static str {
        match self {
            ItemKind::Fixed => "Fest", ItemKind::Displacement => "Verschiebung", ItemKind::Frictionless => "Reibungsfrei", ItemKind::Symmetry => "Symmetrie",
            ItemKind::Force => "Kraft", ItemKind::Moment => "Moment", ItemKind::Pressure => "Druck", ItemKind::Gravity => "Gravitation",
            ItemKind::Centrifugal => "Zentrifugal", ItemKind::LinAccel => "Lineare Beschleunigung",
            ItemKind::Temperature => "Temperatur", ItemKind::Convection => "Konvektion", ItemKind::HeatFlow => "Wärmestrom", ItemKind::HeatFlux => "Wärmefluss",
            ItemKind::Inlet => "Einlass", ItemKind::Outlet => "Auslass", ItemKind::Wind => "Anströmung",
        }
    }
    /// The item acts on picked faces (the others act on the whole body or the surroundings).
    pub fn needs_faces(self) -> bool { !matches!(self, ItemKind::Gravity | ItemKind::Centrifugal | ItemKind::LinAccel | ItemKind::Wind) }
    pub fn is_load(self) -> bool {
        matches!(self, ItemKind::Force | ItemKind::Moment | ItemKind::Pressure | ItemKind::Gravity | ItemKind::Centrifugal | ItemKind::LinAccel | ItemKind::HeatFlow | ItemKind::HeatFlux)
    }
    pub fn is_thermal(self) -> bool { matches!(self, ItemKind::Temperature | ItemKind::Convection | ItemKind::HeatFlow | ItemKind::HeatFlux) }
    pub fn is_fluid(self) -> bool { matches!(self, ItemKind::Inlet | ItemKind::Outlet | ItemKind::Wind) }
    /// Whether a study of `kind` (with thermal objects included or not) takes this item.
    pub fn fits(self, kind: StudyKind, thermal: bool) -> bool {
        match kind {
            StudyKind::Structural => (!self.is_thermal() && !self.is_fluid()) || (thermal && self.is_thermal()),
            StudyKind::Modal => matches!(self, ItemKind::Fixed | ItemKind::Frictionless | ItemKind::Symmetry),
            StudyKind::Thermal => self.is_thermal(),
            StudyKind::Fluid => self.is_fluid(),
        }
    }
}

/// A picked face, found again after regeneration by a point on it and its normal there.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimFace { pub face: FaceRef, pub planar: bool }

impl SimFace {
    pub fn of(topo: &Topology, f: usize) -> SimFace { SimFace { face: topo.surface_ref(f), planar: topo.faces[f].planar } }
    pub fn resolve(&self, topo: &Topology) -> Option<usize> {
        if self.planar {
            let f = topo.match_face(self.face.point, self.face.normal)?;
            // the stored centre may lie in a hole of the face: only its plane has to agree
            ((self.face.point - topo.faces[f].center).dot(topo.faces[f].normal).abs() <= (topo.bbox.diagonal() * 2e-3).max(1e-3)).then_some(f)
        } else { topo.match_surface(self.face.point, self.face.normal) }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimItem {
    pub id: u64,
    pub name: String,
    pub kind: ItemKind,
    pub faces: Vec<SimFace>,
    /// force N, moment N·mm, displacement mm, acceleration mm/s², rotation axis, flow direction
    pub vec: [f64; 3],
    /// pressure MPa, temperature °C, film coefficient W/(m²·K), heat W, heat flux W/mm², speed m/s, rotation 1/min
    pub value: f64,
    /// ambient temperature °C (Konvektion)
    pub value2: f64,
    /// Verschiebung: which components are prescribed
    pub fixed: [bool; 3],
}

impl SimItem {
    pub fn new(id: u64, name: String, kind: ItemKind) -> SimItem {
        let (vec, value, value2) = match kind {
            ItemKind::Force => ([0.0, -100.0, 0.0], 0.0, 0.0),
            ItemKind::Moment => ([0.0, 0.0, 1000.0], 0.0, 0.0),
            ItemKind::Pressure => ([0.0; 3], 1.0, 0.0),
            ItemKind::Gravity => ([0.0, -9810.0, 0.0], 0.0, 0.0),
            ItemKind::LinAccel => ([0.0, 0.0, 9810.0], 0.0, 0.0),
            ItemKind::Centrifugal => ([0.0, 1.0, 0.0], 3000.0, 0.0),
            ItemKind::Temperature => ([0.0; 3], 100.0, 0.0),
            ItemKind::Convection => ([0.0; 3], 25.0, 20.0),
            ItemKind::HeatFlow => ([0.0; 3], 10.0, 0.0),
            ItemKind::HeatFlux => ([0.0; 3], 0.01, 0.0),
            ItemKind::Inlet => ([0.0; 3], 1.0, 0.0),
            ItemKind::Wind => ([1.0, 0.0, 0.0], 10.0, 0.0),
            _ => ([0.0; 3], 0.0, 0.0),
        };
        SimItem { id, name, kind, faces: vec![], vec, value, value2, fixed: [true; 3] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub name: &'static str,
    /// Elastizitätsmodul MPa
    pub e: f64,
    pub nu: f64,
    /// kg/m³
    pub rho: f64,
    /// Wärmeausdehnung 1/K
    pub alpha: f64,
    /// Wärmeleitfähigkeit W/(m·K)
    pub k: f64,
    /// Streckgrenze MPa
    pub yield_mpa: f64,
}

pub const MATERIALS: [Material; 9] = [
    Material { name: "Stahl", e: 210000.0, nu: 0.30, rho: 7850.0, alpha: 1.2e-5, k: 50.0, yield_mpa: 235.0 },
    Material { name: "Edelstahl", e: 193000.0, nu: 0.29, rho: 8000.0, alpha: 1.6e-5, k: 16.0, yield_mpa: 215.0 },
    Material { name: "Aluminium", e: 69000.0, nu: 0.33, rho: 2700.0, alpha: 2.36e-5, k: 167.0, yield_mpa: 275.0 },
    Material { name: "Kupfer", e: 117000.0, nu: 0.34, rho: 8940.0, alpha: 1.7e-5, k: 390.0, yield_mpa: 70.0 },
    Material { name: "Messing", e: 100000.0, nu: 0.34, rho: 8500.0, alpha: 1.9e-5, k: 120.0, yield_mpa: 200.0 },
    Material { name: "Titan", e: 110000.0, nu: 0.34, rho: 4430.0, alpha: 8.6e-6, k: 7.0, yield_mpa: 880.0 },
    Material { name: "ABS", e: 2300.0, nu: 0.35, rho: 1050.0, alpha: 9.0e-5, k: 0.17, yield_mpa: 40.0 },
    Material { name: "PLA", e: 3500.0, nu: 0.36, rho: 1240.0, alpha: 6.8e-5, k: 0.13, yield_mpa: 60.0 },
    Material { name: "Nylon (PA6)", e: 2700.0, nu: 0.39, rho: 1140.0, alpha: 8.0e-5, k: 0.25, yield_mpa: 70.0 },
];

pub fn material(name: &str) -> Material { MATERIALS.iter().copied().find(|m| m.name == name).unwrap_or(MATERIALS[0]) }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fluid { pub name: &'static str, /** kg/m³ */ pub rho: f64, /** kinematic viscosity m²/s */ pub nu: f64 }

pub const FLUIDS: [Fluid; 3] = [
    Fluid { name: "Luft", rho: 1.204, nu: 1.516e-5 },
    Fluid { name: "Wasser", rho: 998.2, nu: 1.004e-6 },
    Fluid { name: "Öl (ISO VG 46)", rho: 870.0, nu: 4.6e-5 },
];

pub fn fluid(name: &str) -> Fluid { FLUIDS.iter().copied().find(|m| m.name == name).unwrap_or(FLUIDS[0]) }

fn default_modes() -> usize { 4 }
fn default_ref_temp() -> f64 { 20.0 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Study {
    pub id: u64,
    pub name: String,
    pub kind: StudyKind,
    pub material: String,
    pub items: Vec<SimItem>,
    /// Studie ▸ Thermische Objekte einschließen: thermal conditions load a structural study
    #[serde(default)]
    pub include_thermal: bool,
    #[serde(default)]
    pub fluid: String,
    #[serde(default = "default_modes")]
    pub modes: usize,
    /// stress-free temperature °C
    #[serde(default = "default_ref_temp")]
    pub ref_temp: f64,
    #[serde(default)]
    pub next_item: u64,
}

impl Study {
    pub fn new(id: u64, name: String, kind: StudyKind) -> Study {
        Study { id, name, kind, material: MATERIALS[0].name.into(), items: vec![], include_thermal: false, fluid: FLUIDS[0].name.into(), modes: 4, ref_temp: 20.0, next_item: 1 }
    }
    pub fn item_name(&self, kind: ItemKind) -> String {
        let n = self.items.iter().filter(|i| i.kind == kind).count() + 1;
        format!("{} {}", kind.name(), n)
    }
}
