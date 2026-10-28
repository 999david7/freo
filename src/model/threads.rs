//! Thread and clearance-hole tables for standard holes (ISO metric coarse, UNC).

use super::features::{HoleFit, ThreadStd};

pub struct ThreadSpec {
    pub name: &'static str,
    /// major (nominal) diameter, mm
    pub major: f64,
    /// thread pitch, mm
    pub pitch: f64,
    /// tap drill diameter, mm
    pub tap: f64,
    /// clearance hole diameters (close, medium, free), mm
    pub clear: [f64; 3],
}

const fn iso(name: &'static str, major: f64, pitch: f64, tap: f64, clear: [f64; 3]) -> ThreadSpec { ThreadSpec { name, major, pitch, tap, clear } }

/// ISO 261 coarse threads with ISO 273 clearance holes.
pub const ISO: &[ThreadSpec] = &[
    iso("M2", 2.0, 0.4, 1.6, [2.2, 2.4, 2.6]),
    iso("M2.5", 2.5, 0.45, 2.05, [2.7, 2.9, 3.1]),
    iso("M3", 3.0, 0.5, 2.5, [3.2, 3.4, 3.6]),
    iso("M4", 4.0, 0.7, 3.3, [4.3, 4.5, 4.8]),
    iso("M5", 5.0, 0.8, 4.2, [5.3, 5.5, 5.8]),
    iso("M6", 6.0, 1.0, 5.0, [6.4, 6.6, 7.0]),
    iso("M8", 8.0, 1.25, 6.8, [8.4, 9.0, 10.0]),
    iso("M10", 10.0, 1.5, 8.5, [10.5, 11.0, 12.0]),
    iso("M12", 12.0, 1.75, 10.2, [13.0, 13.5, 14.5]),
    iso("M14", 14.0, 2.0, 12.0, [15.0, 15.5, 16.5]),
    iso("M16", 16.0, 2.0, 14.0, [17.0, 17.5, 18.5]),
    iso("M20", 20.0, 2.5, 17.5, [21.0, 22.0, 24.0]),
    iso("M24", 24.0, 3.0, 21.0, [25.0, 26.0, 28.0]),
];

const IN: f64 = 25.4;

const fn unc(name: &'static str, major_in: f64, tpi: f64, tap_in: f64, clear_in: [f64; 3]) -> ThreadSpec {
    ThreadSpec { name, major: major_in * IN, pitch: IN / tpi, tap: tap_in * IN, clear: [clear_in[0] * IN, clear_in[1] * IN, clear_in[2] * IN] }
}

/// Unified coarse threads (ASME B1.1) with common tap and clearance drills.
pub const UNC: &[ThreadSpec] = &[
    unc("#4-40", 0.112, 40.0, 0.0890, [0.1160, 0.1220, 0.1285]),
    unc("#6-32", 0.138, 32.0, 0.1065, [0.1440, 0.1470, 0.1495]),
    unc("#8-32", 0.164, 32.0, 0.1360, [0.1695, 0.1730, 0.1770]),
    unc("#10-24", 0.190, 24.0, 0.1495, [0.1960, 0.1990, 0.2010]),
    unc("1/4-20", 0.250, 20.0, 0.2010, [0.2570, 0.2610, 0.2660]),
    unc("5/16-18", 0.3125, 18.0, 0.2570, [0.3230, 0.3280, 0.3320]),
    unc("3/8-16", 0.375, 16.0, 0.3125, [0.3860, 0.3910, 0.3970]),
    unc("1/2-13", 0.500, 13.0, 0.4219, [0.5156, 0.5234, 0.5312]),
];

pub fn table(s: ThreadStd) -> &'static [ThreadSpec] {
    match s { ThreadStd::Iso => ISO, ThreadStd::Unc => UNC }
}

pub fn find(s: ThreadStd, name: &str) -> Option<&'static ThreadSpec> { table(s).iter().find(|t| t.name == name) }

/// Drill diameter for a standard hole.
pub fn drill(spec: &ThreadSpec, tapped: bool, fit: HoleFit) -> f64 {
    if tapped { spec.tap } else { spec.clear[match fit { HoleFit::Close => 0, HoleFit::Medium => 1, HoleFit::Free => 2 }] }
}

/// Hole callout, e.g. "M6x1" or "1/4-20 UNC".
pub fn callout(s: ThreadStd, spec: &ThreadSpec) -> String {
    match s {
        ThreadStd::Iso => format!("{}x{}", spec.name, crate::kernel::math::fmt_num(spec.pitch, 2)),
        ThreadStd::Unc => format!("{} UNC", spec.name),
    }
}
