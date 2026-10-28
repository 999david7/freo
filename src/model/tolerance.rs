//! Dimension tolerances: display modes and ISO 286 limits and fits (standard tolerance
//! grades IT5–IT11 and the common fundamental deviations), sizes up to 500 mm.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TolMode { #[default] Nominal, Limits, PlusMinus, Symmetric, Basic, Reference, Fit }

impl TolMode {
    pub const ALL: [TolMode; 7] = [TolMode::Nominal, TolMode::Limits, TolMode::PlusMinus, TolMode::Symmetric, TolMode::Basic, TolMode::Reference, TolMode::Fit];
    pub fn name(&self) -> &'static str {
        match self {
            TolMode::Nominal => "Nennmaß", TolMode::Limits => "Grenzmaße", TolMode::PlusMinus => "Plus-Minus", TolMode::Symmetric => "± Symmetrisch",
            TolMode::Basic => "Theoretisch genau", TolMode::Reference => "Hilfsmaß", TolMode::Fit => "ISO-Passung",
        }
    }
}

/// Tolerance of a drawing dimension; deviations are signed, in mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    pub mode: TolMode,
    pub upper: f64,
    pub lower: f64,
    /// ISO fit class, e.g. "H7" (hole) or "g6" (shaft)
    pub fit: String,
}

impl Default for Tolerance { fn default() -> Self { Tolerance { mode: TolMode::Nominal, upper: 0.05, lower: -0.05, fit: "H7".into() } } }

impl Tolerance {
    /// (upper, lower) deviations for a nominal size; None when an ISO class does not apply.
    pub fn deviations(&self, nominal: f64) -> Option<(f64, f64)> {
        match self.mode {
            TolMode::Limits | TolMode::PlusMinus => Some((self.upper, self.lower)),
            TolMode::Symmetric => Some((self.upper.abs(), -self.upper.abs())),
            TolMode::Fit => fit_deviations(&self.fit, nominal),
            _ => None,
        }
    }
}

/// Upper bounds (inclusive) of the ISO 286 nominal size ranges, mm.
const RANGES: [f64; 13] = [3.0, 6.0, 10.0, 18.0, 30.0, 50.0, 80.0, 120.0, 180.0, 250.0, 315.0, 400.0, 500.0];

/// Standard tolerance grades (µm) per size range.
const IT: [(u32, [u16; 13]); 7] = [
    (5, [4, 5, 6, 8, 9, 11, 13, 15, 18, 20, 23, 25, 27]),
    (6, [6, 8, 9, 11, 13, 16, 19, 22, 25, 29, 32, 36, 40]),
    (7, [10, 12, 15, 18, 21, 25, 30, 35, 40, 46, 52, 57, 63]),
    (8, [14, 18, 22, 27, 33, 39, 46, 54, 63, 72, 81, 89, 97]),
    (9, [25, 30, 36, 43, 52, 62, 74, 87, 100, 115, 130, 140, 155]),
    (10, [40, 48, 58, 70, 84, 100, 120, 140, 160, 185, 210, 230, 250]),
    (11, [60, 75, 90, 110, 130, 160, 190, 220, 250, 290, 320, 360, 400]),
];

/// Shaft fundamental deviations (µm): f and g give the upper deviation es; k, n, p the lower ei.
const DEV_F: [i32; 13] = [-6, -10, -13, -16, -20, -25, -30, -36, -43, -50, -56, -62, -68];
const DEV_G: [i32; 13] = [-2, -4, -5, -6, -7, -9, -10, -12, -14, -15, -17, -18, -20];
const DEV_K: [i32; 13] = [0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4, 5];
const DEV_N: [i32; 13] = [4, 8, 10, 12, 15, 17, 20, 23, 27, 31, 34, 37, 40];
const DEV_P: [i32; 13] = [6, 12, 15, 18, 22, 26, 32, 37, 43, 50, 56, 62, 68];

/// Classes offered in the UI.
pub const COMMON_FITS: [&str; 16] = ["H7", "H8", "H9", "H11", "G7", "F8", "JS7", "h6", "h7", "h9", "g6", "f7", "js6", "k6", "n6", "p6"];

fn range_index(size: f64) -> Option<usize> {
    if !(size > 0.0) { return None; }
    RANGES.iter().position(|r| size <= *r + 1e-9)
}

/// Standard tolerance IT`grade` (mm) for a nominal size.
pub fn it_grade(grade: u32, size: f64) -> Option<f64> {
    let i = range_index(size)?;
    IT.iter().find(|g| g.0 == grade).map(|g| g.1[i] as f64 / 1000.0)
}

/// Upper and lower deviation (mm) of an ISO 286 class such as "H7", "g6" or "JS7".
pub fn fit_deviations(class: &str, size: f64) -> Option<(f64, f64)> {
    let c = class.trim();
    let split = c.find(|ch: char| ch.is_ascii_digit())?;
    let (letters, grade) = (&c[..split], c[split..].parse::<u32>().ok()?);
    let i = range_index(size)?;
    let it = it_grade(grade, size)? * 1000.0;
    let um = |v: f64| v / 1000.0;
    let (es, ei) = match letters {
        // holes
        "H" => (it, 0.0),
        "G" => { let ei = -DEV_G[i] as f64; (ei + it, ei) }
        "F" => { let ei = -DEV_F[i] as f64; (ei + it, ei) }
        "JS" => (it / 2.0, -it / 2.0),
        // shafts
        "h" => (0.0, -it),
        "g" => { let es = DEV_G[i] as f64; (es, es - it) }
        "f" => { let es = DEV_F[i] as f64; (es, es - it) }
        "js" => (it / 2.0, -it / 2.0),
        "k" => { let ei = if (4..=7).contains(&grade) { DEV_K[i] as f64 } else { 0.0 }; (ei + it, ei) }
        "n" => { let ei = DEV_N[i] as f64; (ei + it, ei) }
        "p" => { let ei = DEV_P[i] as f64; (ei + it, ei) }
        _ => return None,
    };
    Some((um(es), um(ei)))
}
