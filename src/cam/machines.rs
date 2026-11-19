//! CNC machines: travels, spindle and controller. The table holds the manufacturers'
//! nominal working areas and spindles; every value can be edited, and a machine of any
//! size can be added.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Controller {
    Grbl,
    GrblHal,
    LinuxCnc,
    Mach3,
    Mach4,
    Uccnc,
    Masso,
    Centroid,
    Fanuc,
    Haas,
}

impl Controller {
    pub const ALL: [Controller; 10] = [
        Controller::Grbl,
        Controller::GrblHal,
        Controller::LinuxCnc,
        Controller::Mach3,
        Controller::Mach4,
        Controller::Uccnc,
        Controller::Masso,
        Controller::Centroid,
        Controller::Fanuc,
        Controller::Haas,
    ];
    pub fn name(&self) -> &'static str {
        match self {
            Controller::Grbl => "GRBL",
            Controller::GrblHal => "grblHAL / FluidNC",
            Controller::LinuxCnc => "LinuxCNC",
            Controller::Mach3 => "Mach3",
            Controller::Mach4 => "Mach4",
            Controller::Uccnc => "UCCNC",
            Controller::Masso => "Masso",
            Controller::Centroid => "Centroid Acorn",
            Controller::Fanuc => "Fanuc",
            Controller::Haas => "Haas",
        }
    }
    /// Understands M6 tool changes with tool length offsets.
    pub fn tool_change(&self) -> bool {
        !matches!(self, Controller::Grbl)
    }
    pub fn ext(&self) -> &'static str {
        match self {
            Controller::Grbl | Controller::GrblHal => "nc",
            Controller::LinuxCnc => "ngc",
            Controller::Mach3 | Controller::Mach4 | Controller::Uccnc | Controller::Masso => "tap",
            Controller::Centroid => "nc",
            Controller::Fanuc | Controller::Haas => "nc",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MachineKind {
    /// gantry router: light, fast spindle, soft materials and aluminium
    Router,
    /// milling machine: rigid, all metals
    Mill,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Machine {
    pub brand: String,
    pub model: String,
    pub kind: MachineKind,
    /// travels X, Y, Z (mm)
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// tallest workpiece under the spindle (mm)
    pub clearance: f64,
    pub rpm_min: f64,
    pub rpm_max: f64,
    /// spindle power, W
    pub power: f64,
    /// fastest feed / rapid, mm/min
    pub feed_max: f64,
    pub rapid: f64,
    pub controller: Controller,
    pub atc: bool,
    pub coolant: bool,
}

impl Default for Machine {
    fn default() -> Self {
        all_machines().into_iter().next().unwrap()
    }
}

impl Machine {
    pub fn name(&self) -> String {
        format!("{} {}", self.brand, self.model).trim().to_string()
    }
    /// How much harder than a router the machine can be pushed.
    pub fn rigidity(&self) -> f64 {
        match self.kind {
            MachineKind::Router => 1.0,
            MachineKind::Mill => 2.5,
        }
    }
    pub fn validate(&self) -> Option<String> {
        let pos = |v: f64| v.is_finite() && v > 0.0;
        if !pos(self.x) || !pos(self.y) || !pos(self.z) || !pos(self.clearance) {
            return Some("Verfahrwege der Maschine müssen größer als 0 sein.".into());
        }
        if !pos(self.rpm_max) || self.rpm_min < 0.0 || self.rpm_min > self.rpm_max {
            return Some("Spindeldrehzahl: Minimum muss unter dem Maximum liegen.".into());
        }
        if !pos(self.feed_max) || !pos(self.rapid) {
            return Some("Vorschub und Eilgang der Maschine müssen größer als 0 sein.".into());
        }
        None
    }
}

type Row = (&'static str, &'static str, MachineKind, f64, f64, f64, f64, f64, f64, f64, f64, Controller);

use Controller::*;
use MachineKind::*;

/// Brand, model, kind, X, Y, Z travel, work height, rpm min, rpm max, power W, max feed,
/// controller — nominal values from the makers' data sheets.
const TABLE: &[Row] = &[
    ("Carbide 3D", "Shapeoko 4 Standard", Router, 444.0, 444.0, 95.0, 95.0, 10000.0, 30000.0, 710.0, 5000.0, Grbl),
    ("Carbide 3D", "Shapeoko 4 XL", Router, 838.0, 406.0, 95.0, 95.0, 10000.0, 30000.0, 710.0, 5000.0, Grbl),
    ("Carbide 3D", "Shapeoko 4 XXL", Router, 838.0, 838.0, 95.0, 95.0, 10000.0, 30000.0, 710.0, 5000.0, Grbl),
    ("Carbide 3D", "Shapeoko 5 Pro 4×2", Router, 1219.0, 610.0, 152.0, 152.0, 10000.0, 24000.0, 1500.0, 10000.0, Grbl),
    ("Carbide 3D", "Shapeoko 5 Pro 4×4", Router, 1219.0, 1219.0, 152.0, 152.0, 10000.0, 24000.0, 1500.0, 10000.0, Grbl),
    ("Carbide 3D", "Nomad 3", Mill, 203.0, 203.0, 76.0, 76.0, 2000.0, 10000.0, 130.0, 2000.0, Grbl),
    ("Onefinity", "Machinist", Router, 406.0, 406.0, 133.0, 133.0, 10000.0, 30000.0, 710.0, 10000.0, GrblHal),
    ("Onefinity", "Woodworker", Router, 816.0, 816.0, 133.0, 133.0, 10000.0, 30000.0, 710.0, 10000.0, GrblHal),
    ("Onefinity", "Journeyman", Router, 1220.0, 816.0, 133.0, 133.0, 10000.0, 30000.0, 710.0, 10000.0, GrblHal),
    ("Sienci Labs", "LongMill MK2 30×30", Router, 775.0, 826.0, 114.0, 114.0, 10000.0, 30000.0, 710.0, 4000.0, Grbl),
    ("Sienci Labs", "LongMill MK2 48×30", Router, 1232.0, 826.0, 114.0, 114.0, 10000.0, 30000.0, 710.0, 4000.0, Grbl),
    ("Genmitsu", "3018-PROVer", Router, 300.0, 180.0, 45.0, 45.0, 1000.0, 10000.0, 60.0, 1000.0, Grbl),
    ("Genmitsu", "4040-PRO", Router, 400.0, 400.0, 78.0, 78.0, 1000.0, 10000.0, 300.0, 2500.0, Grbl),
    ("Genmitsu", "PROVerXL 4030", Router, 400.0, 300.0, 110.0, 110.0, 10000.0, 30000.0, 710.0, 4000.0, Grbl),
    ("FoxAlien", "Masuter Pro", Router, 400.0, 400.0, 55.0, 55.0, 1000.0, 10000.0, 300.0, 2500.0, Grbl),
    ("Two Trees", "TTC450", Router, 460.0, 440.0, 80.0, 80.0, 1000.0, 10000.0, 500.0, 3000.0, Grbl),
    ("BobsCNC", "E4", Router, 610.0, 610.0, 85.0, 85.0, 16000.0, 27000.0, 1000.0, 5000.0, Grbl),
    ("Inventables", "X-Carve 1000 mm", Router, 750.0, 750.0, 65.0, 65.0, 10000.0, 30000.0, 710.0, 8000.0, Grbl),
    ("OpenBuilds", "LEAD 1010", Router, 730.0, 810.0, 90.0, 90.0, 10000.0, 30000.0, 710.0, 8000.0, Grbl),
    ("Makera", "Carvera", Mill, 360.0, 240.0, 140.0, 140.0, 6000.0, 15000.0, 200.0, 3000.0, GrblHal),
    ("Makera", "Carvera Air", Mill, 300.0, 200.0, 140.0, 140.0, 6000.0, 15000.0, 200.0, 3000.0, GrblHal),
    ("Bantam Tools", "Desktop CNC", Mill, 140.0, 114.0, 88.0, 88.0, 2000.0, 26000.0, 200.0, 2500.0, Grbl),
    ("Snapmaker", "2.0 A350 CNC", Router, 320.0, 350.0, 275.0, 275.0, 6000.0, 12000.0, 50.0, 3000.0, Grbl),
    ("Stepcraft", "D.420", Router, 300.0, 420.0, 140.0, 140.0, 5000.0, 25000.0, 1000.0, 3000.0, Uccnc),
    ("Stepcraft", "D.600", Router, 420.0, 600.0, 140.0, 140.0, 5000.0, 25000.0, 1000.0, 3000.0, Uccnc),
    ("Stepcraft", "D.840", Router, 600.0, 840.0, 140.0, 140.0, 5000.0, 25000.0, 1000.0, 3000.0, Uccnc),
    ("CNC-Step", "High-Z S-720", Router, 720.0, 420.0, 110.0, 110.0, 6000.0, 24000.0, 2200.0, 3000.0, Mach3),
    ("CNC-Step", "High-Z S-1000", Router, 1000.0, 600.0, 110.0, 110.0, 6000.0, 24000.0, 2200.0, 3000.0, Mach3),
    ("Avid CNC", "PRO4848", Router, 1257.0, 1257.0, 203.0, 203.0, 6000.0, 24000.0, 3000.0, 10000.0, Mach4),
    ("Avid CNC", "PRO4896", Router, 1257.0, 2477.0, 203.0, 203.0, 6000.0, 24000.0, 3000.0, 10000.0, Mach4),
    ("Laguna", "SWIFT 4×4", Router, 1300.0, 1300.0, 200.0, 200.0, 6000.0, 24000.0, 3700.0, 15000.0, Mach4),
    ("Laguna", "SWIFT 4×8", Router, 1300.0, 2540.0, 200.0, 200.0, 6000.0, 24000.0, 3700.0, 15000.0, Mach4),
    ("ShopBot", "Desktop MAX", Router, 965.0, 635.0, 140.0, 140.0, 8000.0, 24000.0, 1500.0, 9000.0, Centroid),
    ("ShopBot", "PRSalpha 96-48", Router, 2464.0, 1270.0, 152.0, 152.0, 8000.0, 24000.0, 3700.0, 18000.0, Centroid),
    ("Proxxon", "MF 70 CNC", Mill, 134.0, 46.0, 80.0, 80.0, 5000.0, 20000.0, 100.0, 300.0, LinuxCnc),
    ("Tormach", "PCNC 440", Mill, 254.0, 159.0, 254.0, 254.0, 100.0, 10000.0, 750.0, 2540.0, LinuxCnc),
    ("Tormach", "PCNC 770M", Mill, 356.0, 190.0, 330.0, 330.0, 175.0, 10000.0, 1100.0, 3810.0, LinuxCnc),
    ("Tormach", "PCNC 1100M", Mill, 457.0, 241.0, 413.0, 413.0, 100.0, 5140.0, 1500.0, 2790.0, LinuxCnc),
    ("Datron", "neo", Mill, 520.0, 420.0, 220.0, 220.0, 2000.0, 40000.0, 2000.0, 22000.0, Fanuc),
    ("Haas", "Desktop Mill", Mill, 305.0, 152.0, 152.0, 152.0, 1000.0, 15000.0, 750.0, 1270.0, Haas),
    ("Haas", "Mini Mill", Mill, 406.0, 305.0, 254.0, 254.0, 100.0, 10000.0, 5600.0, 15200.0, Haas),
    ("Haas", "DT-1", Mill, 508.0, 406.0, 394.0, 394.0, 100.0, 15000.0, 11200.0, 30500.0, Haas),
    ("Haas", "DM-1", Mill, 508.0, 406.0, 394.0, 394.0, 100.0, 15000.0, 11200.0, 30500.0, Haas),
    ("Haas", "TM-1", Mill, 762.0, 305.0, 406.0, 406.0, 100.0, 4000.0, 5600.0, 5080.0, Haas),
    ("Haas", "VF-1", Mill, 508.0, 406.0, 508.0, 508.0, 100.0, 8100.0, 22400.0, 16500.0, Haas),
    ("Haas", "VF-2", Mill, 762.0, 406.0, 508.0, 508.0, 100.0, 8100.0, 22400.0, 16500.0, Haas),
    ("Haas", "VF-3", Mill, 1016.0, 508.0, 635.0, 635.0, 100.0, 8100.0, 22400.0, 16500.0, Haas),
    ("Haas", "VF-4", Mill, 1270.0, 508.0, 635.0, 635.0, 100.0, 8100.0, 22400.0, 16500.0, Haas),
    ("Fanuc", "Robodrill α-D21MiB5", Mill, 700.0, 400.0, 330.0, 330.0, 100.0, 10000.0, 18500.0, 30000.0, Fanuc),
    ("DMG Mori", "CMX 600 V", Mill, 600.0, 560.0, 510.0, 510.0, 20.0, 12000.0, 13000.0, 30000.0, Fanuc),
    ("DMG Mori", "DMU 50 (3-Achs)", Mill, 650.0, 520.0, 475.0, 475.0, 20.0, 14000.0, 13000.0, 30000.0, Fanuc),
    ("Hurco", "VM10i", Mill, 660.0, 406.0, 508.0, 508.0, 10.0, 12000.0, 11000.0, 25400.0, Fanuc),
];

fn from_row(r: &Row) -> Machine {
    let (brand, model, kind, x, y, z, clearance, rpm_min, rpm_max, power, feed_max, controller) = *r;
    Machine {
        brand: brand.into(),
        model: model.into(),
        kind,
        x,
        y,
        z,
        clearance,
        rpm_min,
        rpm_max,
        power,
        feed_max,
        rapid: feed_max,
        controller,
        atc: matches!(controller, Haas | Fanuc) || brand == "Makera",
        coolant: kind == Mill && power >= 1000.0,
    }
}

pub fn all_machines() -> Vec<Machine> {
    TABLE.iter().map(from_row).collect()
}

pub fn custom_machine() -> Machine {
    Machine { brand: String::new(), model: "Eigene Maschine".into(), ..from_row(&TABLE[0]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sane() {
        let all = all_machines();
        assert!(all.len() >= 50);
        for m in &all {
            assert!(m.validate().is_none(), "{}: {:?}", m.name(), m.validate());
        }
    }
}
