//! Leiterplatten (PCB) design: a KiCad-style workflow — schematic with a symbol library,
//! netlist and ERC, footprints, board layout with ratsnest, routing, vias, copper zones and
//! DRC, and the manufacturing outputs (Gerber RS-274X, Excellon, BOM, a 3D body).
//!
//! All lengths are millimetres. Schematic coordinates have y pointing down (as on paper and
//! in KiCad); board coordinates have y pointing down too (KiCad convention), which the
//! Gerber writer flips.

pub mod autoroute;
pub mod board;
pub mod body3d;
pub mod bom;
pub mod demo;
pub mod drc;
pub mod fab;
pub mod footprints;
pub mod geom;
pub mod gerber;
pub mod kicad;
pub mod model3d;
pub mod models;
pub mod route;
pub mod stroke;
pub mod schematic;
pub mod sexpr;
pub mod symbols;

pub use board::Board;
pub use schematic::Schematic;

use serde::{Deserialize, Serialize};

/// A PCB project: one schematic sheet and the board made from it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub schematic: Schematic,
    pub board: Board,
    /// footprints made in the footprint editor (project library)
    #[serde(default)]
    pub footprints: Vec<footprints::Footprint>,
}

impl Project {
    pub fn new(name: &str) -> Project {
        Project { name: name.into(), schematic: Schematic::default(), board: Board::default(), footprints: vec![] }
    }
}
