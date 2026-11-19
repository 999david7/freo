//! Slicer settings in three parts, as in Orca Slicer: the printer (machine), the filament and
//! the process (print quality, strength, speed, support, adhesion). Units: mm, mm/s, mm/s²,
//! °C, seconds; ratios are 0..1.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Flavor {
    Marlin,
    Klipper,
    RepRapFirmware,
    Bambu,
}

impl Flavor {
    pub const ALL: [Flavor; 4] = [Flavor::Klipper, Flavor::Marlin, Flavor::RepRapFirmware, Flavor::Bambu];
    pub fn name(&self) -> &'static str {
        match self {
            Flavor::Marlin => "Marlin 2",
            Flavor::Klipper => "Klipper",
            Flavor::RepRapFirmware => "RepRapFirmware",
            Flavor::Bambu => "Bambu Lab",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BedShape {
    Rect,
    /// round bed of a delta printer, `bed_x` is the diameter, origin in the centre
    Round,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Plate {
    TexturedPei,
    SmoothPei,
    CoolPlate,
    EngineeringPlate,
    Glass,
    Buildtak,
}

impl Plate {
    pub const ALL: [Plate; 6] = [Plate::TexturedPei, Plate::SmoothPei, Plate::CoolPlate, Plate::EngineeringPlate, Plate::Glass, Plate::Buildtak];
    pub fn name(&self) -> &'static str {
        match self {
            Plate::TexturedPei => "Texturierte PEI-Platte",
            Plate::SmoothPei => "Glatte PEI-Platte",
            Plate::CoolPlate => "Cool Plate",
            Plate::EngineeringPlate => "Engineering Plate",
            Plate::Glass => "Glasplatte",
            Plate::Buildtak => "BuildTak",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Printer {
    pub vendor: String,
    pub model: String,
    /// bed size X × Y (for a round bed: diameter in `bed_x`) and the printable height
    pub bed_x: f64,
    pub bed_y: f64,
    pub height: f64,
    pub shape: BedShape,
    /// G-code coordinates have the origin in the bed centre (deltas)
    pub origin_center: bool,
    pub nozzle: f64,
    pub filament_diameter: f64,
    pub flavor: Flavor,
    pub plate: Plate,
    pub direct_drive: bool,
    pub max_speed: f64,
    pub max_accel: f64,
    pub max_z_speed: f64,
    /// Klipper square corner velocity / Marlin jerk, mm/s
    pub corner_velocity: f64,
    pub retract_length: f64,
    pub retract_speed: f64,
    pub deretract_speed: f64,
    pub retract_min_travel: f64,
    pub retract_on_layer_change: bool,
    pub z_hop: f64,
    pub wipe: bool,
    pub wipe_distance: f64,
    pub heated_chamber: bool,
    /// write M201/M203/M204 (Marlin) or SET_VELOCITY_LIMIT (Klipper) at the start
    pub emit_limits: bool,
    /// thumbnail size written into the G-code (0 = none)
    pub thumbnail: u32,
    pub start_gcode: String,
    pub end_gcode: String,
    pub layer_gcode: String,
}

impl Default for Printer {
    fn default() -> Self {
        super::printers::artillery_m1_pro()
    }
}

impl Printer {
    pub fn name(&self) -> String {
        if self.vendor.is_empty() {
            self.model.clone()
        } else {
            format!("{} {}", self.vendor, self.model)
        }
    }
    /// Printable outline on the bed (printer coordinates, origin front left).
    pub fn bed_outline(&self) -> Vec<[f64; 2]> {
        match self.shape {
            BedShape::Rect => vec![[0.0, 0.0], [self.bed_x, 0.0], [self.bed_x, self.bed_y], [0.0, self.bed_y]],
            BedShape::Round => {
                let r = self.bed_x / 2.0;
                (0..96)
                    .map(|i| {
                        let a = std::f64::consts::TAU * i as f64 / 96.0;
                        [r + r * a.cos(), r + r * a.sin()]
                    })
                    .collect()
            }
        }
    }
    /// Bed extent in printer coordinates (front left at 0,0).
    pub fn bed_size(&self) -> (f64, f64) {
        match self.shape {
            BedShape::Rect => (self.bed_x, self.bed_y),
            BedShape::Round => (self.bed_x, self.bed_x),
        }
    }
    pub fn on_bed(&self, x: f64, y: f64) -> bool {
        let e = 1e-6;
        match self.shape {
            BedShape::Rect => x >= -e && y >= -e && x <= self.bed_x + e && y <= self.bed_y + e,
            BedShape::Round => {
                let r = self.bed_x / 2.0;
                (x - r).powi(2) + (y - r).powi(2) <= (r + e) * (r + e)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Material {
    Pla,
    PlaPlus,
    PlaSilk,
    PlaMatte,
    PlaCf,
    Petg,
    PetgCf,
    Abs,
    Asa,
    Tpu,
    Pa,
    PaCf,
    Pc,
    Pva,
    Hips,
    Pp,
}

impl Material {
    pub const ALL: [Material; 16] = [
        Material::Pla,
        Material::PlaPlus,
        Material::PlaSilk,
        Material::PlaMatte,
        Material::PlaCf,
        Material::Petg,
        Material::PetgCf,
        Material::Abs,
        Material::Asa,
        Material::Tpu,
        Material::Pa,
        Material::PaCf,
        Material::Pc,
        Material::Pva,
        Material::Hips,
        Material::Pp,
    ];
    /// Short type name as Orca writes it into `filament_type`.
    pub fn code(&self) -> &'static str {
        match self {
            Material::Pla => "PLA",
            Material::PlaPlus => "PLA+",
            Material::PlaSilk => "PLA Silk",
            Material::PlaMatte => "PLA Matte",
            Material::PlaCf => "PLA-CF",
            Material::Petg => "PETG",
            Material::PetgCf => "PETG-CF",
            Material::Abs => "ABS",
            Material::Asa => "ASA",
            Material::Tpu => "TPU",
            Material::Pa => "PA",
            Material::PaCf => "PA-CF",
            Material::Pc => "PC",
            Material::Pva => "PVA",
            Material::Hips => "HIPS",
            Material::Pp => "PP",
        }
    }
    /// Plastics that warp: printed with the chamber closed and the part fan low.
    pub fn enclosure(&self) -> bool {
        matches!(self, Material::Abs | Material::Asa | Material::Pa | Material::PaCf | Material::Pc | Material::Hips | Material::Pp)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Filament {
    pub name: String,
    pub material: Material,
    /// sRGB colour of the plastic (preview, thumbnail)
    pub color: [u8; 3],
    pub diameter: f64,
    /// g/cm³
    pub density: f64,
    /// price per kg
    pub cost: f64,
    pub flow_ratio: f64,
    pub nozzle_temp: f64,
    pub nozzle_temp_first: f64,
    pub bed_temp: f64,
    pub bed_temp_first: f64,
    pub chamber_temp: f64,
    /// mm³/s
    pub max_volumetric: f64,
    pub fan_min: f64,
    pub fan_max: f64,
    /// part fan off for the first n layers
    pub fan_off_layers: u32,
    pub fan_always_on: bool,
    /// fan on overhangs and bridges (0..1)
    pub overhang_fan: f64,
    /// layers shorter than this (s) run at full fan
    pub slow_down_layer_time: f64,
    /// layers shorter than this (s) get more fan, between fan_min and fan_max
    pub fan_cooling_layer_time: f64,
    pub slow_down_for_layer_cooling: bool,
    pub slow_down_min_speed: f64,
    /// filament specific retraction; None = the printer's
    pub retract_length: Option<f64>,
    pub z_hop: Option<f64>,
    pub pressure_advance: Option<f64>,
    /// auxiliary (side) part fan and exhaust fan of printers that have them, percent
    pub aux_fan: f64,
    pub exhaust_fan: f64,
}

impl Default for Filament {
    fn default() -> Self {
        super::printers::filament_presets()[0].clone()
    }
}

impl Filament {
    /// mm² cross section of the raw filament.
    pub fn area(&self) -> f64 {
        std::f64::consts::PI * (self.diameter / 2.0).powi(2)
    }
    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.color[0], self.color[1], self.color[2])
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum InfillPattern {
    Rectilinear,
    Grid,
    Line,
    Triangles,
    Cubic,
    Honeycomb,
    Gyroid,
    Concentric,
    Monotonic,
    ZigZag,
}

impl InfillPattern {
    pub const SPARSE: [InfillPattern; 8] = [
        InfillPattern::Grid,
        InfillPattern::Gyroid,
        InfillPattern::Rectilinear,
        InfillPattern::Line,
        InfillPattern::Triangles,
        InfillPattern::Cubic,
        InfillPattern::Honeycomb,
        InfillPattern::Concentric,
    ];
    pub const SOLID: [InfillPattern; 4] = [InfillPattern::Monotonic, InfillPattern::Rectilinear, InfillPattern::ZigZag, InfillPattern::Concentric];
    pub fn name(&self) -> &'static str {
        match self {
            InfillPattern::Rectilinear => "Geradlinig",
            InfillPattern::Grid => "Gitter",
            InfillPattern::Line => "Linie",
            InfillPattern::Triangles => "Dreiecke",
            InfillPattern::Cubic => "Kubisch",
            InfillPattern::Honeycomb => "Bienenwabe",
            InfillPattern::Gyroid => "Gyroid",
            InfillPattern::Concentric => "Konzentrisch",
            InfillPattern::Monotonic => "Monoton",
            InfillPattern::ZigZag => "Zickzack",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum WallOrder {
    InnerOuter,
    OuterInner,
    InnerOuterInner,
}

impl WallOrder {
    pub const ALL: [WallOrder; 3] = [WallOrder::InnerOuter, WallOrder::OuterInner, WallOrder::InnerOuterInner];
    pub fn name(&self) -> &'static str {
        match self {
            WallOrder::InnerOuter => "Innen/Außen",
            WallOrder::OuterInner => "Außen/Innen",
            WallOrder::InnerOuterInner => "Innen/Außen/Innen",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Seam {
    Aligned,
    Nearest,
    Back,
    Random,
}

impl Seam {
    pub const ALL: [Seam; 4] = [Seam::Aligned, Seam::Nearest, Seam::Back, Seam::Random];
    pub fn name(&self) -> &'static str {
        match self {
            Seam::Aligned => "Ausgerichtet",
            Seam::Nearest => "Am nächsten",
            Seam::Back => "Hinten",
            Seam::Random => "Zufällig",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Brim {
    None,
    Outer,
    Inner,
    Both,
}

impl Brim {
    pub const ALL: [Brim; 4] = [Brim::None, Brim::Outer, Brim::Inner, Brim::Both];
    pub fn name(&self) -> &'static str {
        match self {
            Brim::None => "Keine",
            Brim::Outer => "Nur außen",
            Brim::Inner => "Nur innen",
            Brim::Both => "Außen und innen",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Ironing {
    None,
    Top,
    Topmost,
}

impl Ironing {
    pub const ALL: [Ironing; 3] = [Ironing::None, Ironing::Top, Ironing::Topmost];
    pub fn name(&self) -> &'static str {
        match self {
            Ironing::None => "Kein Glätten",
            Ironing::Top => "Obere Flächen",
            Ironing::Topmost => "Nur oberste Fläche",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Fuzzy {
    None,
    Outer,
    All,
}

impl Fuzzy {
    pub const ALL: [Fuzzy; 3] = [Fuzzy::None, Fuzzy::Outer, Fuzzy::All];
    pub fn name(&self) -> &'static str {
        match self {
            Fuzzy::None => "Keine",
            Fuzzy::Outer => "Außenwand",
            Fuzzy::All => "Alle Wände",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Process {
    pub name: String,
    // ---- Qualität ----
    pub layer_height: f64,
    pub first_layer_height: f64,
    pub line_width: f64,
    pub first_layer_width: f64,
    pub outer_wall_width: f64,
    pub inner_wall_width: f64,
    pub top_width: f64,
    pub solid_width: f64,
    pub sparse_width: f64,
    pub support_width: f64,
    pub seam: Seam,
    /// gap left at the seam of a closed loop, fraction of the nozzle diameter
    pub seam_gap: f64,
    pub elephant_foot: f64,
    /// round off polygons with less deviation than this (mm)
    pub resolution: f64,
    pub ironing: Ironing,
    pub ironing_flow: f64,
    pub ironing_spacing: f64,
    pub ironing_speed: f64,
    pub fuzzy: Fuzzy,
    pub fuzzy_thickness: f64,
    pub fuzzy_point_dist: f64,
    pub detect_overhang_wall: bool,
    // ---- Festigkeit ----
    pub wall_loops: u32,
    pub wall_order: WallOrder,
    pub only_one_wall_top: bool,
    pub top_layers: u32,
    pub bottom_layers: u32,
    pub top_pattern: InfillPattern,
    pub bottom_pattern: InfillPattern,
    pub solid_pattern: InfillPattern,
    pub infill_density: f64,
    pub infill_pattern: InfillPattern,
    pub infill_angle: f64,
    pub infill_overlap: f64,
    pub min_sparse_area: f64,
    pub infill_combine_connect: bool,
    pub spiral_vase: bool,
    // ---- Geschwindigkeit ----
    pub first_layer_speed: f64,
    pub first_layer_infill_speed: f64,
    pub outer_wall_speed: f64,
    pub inner_wall_speed: f64,
    pub sparse_speed: f64,
    pub solid_speed: f64,
    pub top_speed: f64,
    pub gap_speed: f64,
    pub support_speed: f64,
    pub interface_speed: f64,
    pub bridge_speed: f64,
    pub overhang_speed: f64,
    pub travel_speed: f64,
    pub first_layer_travel_speed: f64,
    pub default_accel: f64,
    pub outer_wall_accel: f64,
    pub top_accel: f64,
    pub first_layer_accel: f64,
    pub travel_accel: f64,
    pub slow_down_layers: u32,
    // ---- Stützen ----
    pub support: bool,
    pub support_threshold: f64,
    pub support_build_plate_only: bool,
    pub support_top_z: f64,
    pub support_bottom_z: f64,
    pub support_xy: f64,
    pub support_spacing: f64,
    pub support_angle: f64,
    pub interface_layers: u32,
    pub interface_spacing: f64,
    pub support_remove_small: bool,
    // ---- Haftung ----
    pub skirt_loops: u32,
    pub skirt_distance: f64,
    pub skirt_height: u32,
    pub skirt_speed: f64,
    pub brim: Brim,
    pub brim_width: f64,
    pub brim_gap: f64,
    // ---- Weiteres ----
    pub bridge_flow: f64,
    pub reduce_infill_retraction: bool,
    pub gcode_comments: bool,
}

impl Default for Process {
    fn default() -> Self {
        super::printers::process_presets(0.4)[3].clone()
    }
}

impl Process {
    /// Extrusion spacing of neighbouring lines (Slic3r flow model): beads are rectangles with
    /// round sides, so adjacent lines overlap by h(1 − π/4).
    pub fn spacing(&self, w: f64, h: f64) -> f64 {
        w - h * (1.0 - std::f64::consts::FRAC_PI_4)
    }
}

/// Bead cross section of a line `w` wide and `h` high (mm²).
pub fn bead_area(w: f64, h: f64) -> f64 {
    let h = h.min(w);
    (w - h) * h + std::f64::consts::PI * (h / 2.0).powi(2)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub printer: Printer,
    pub filament: Filament,
    pub process: Process,
}

impl Settings {
    /// Problems that stop slicing; empty when the settings can be used.
    pub fn validate(&self) -> Vec<String> {
        let (p, f, q) = (&self.printer, &self.filament, &self.process);
        let mut e = vec![];
        let pos = |v: f64| v.is_finite() && v > 0.0;
        if !pos(p.bed_x) || !pos(p.bed_y) || !pos(p.height) {
            e.push("Druckbett und Bauhöhe müssen größer als 0 sein.".into());
        }
        if !pos(p.nozzle) {
            e.push("Düsendurchmesser muss größer als 0 sein.".into());
        }
        if !pos(f.diameter) {
            e.push("Filamentdurchmesser muss größer als 0 sein.".into());
        }
        if !pos(q.layer_height) || !pos(q.first_layer_height) {
            e.push("Schichthöhe muss größer als 0 sein.".into());
        }
        if q.layer_height > p.nozzle * 0.85 || q.first_layer_height > p.nozzle * 0.9 {
            e.push(format!("Schichthöhe zu groß für eine {:.2}-mm-Düse (höchstens {:.2} mm).", p.nozzle, p.nozzle * 0.85));
        }
        for (n, w) in [
            ("Linienbreite", q.line_width),
            ("Erste Schicht", q.first_layer_width),
            ("Außenwand", q.outer_wall_width),
            ("Innenwand", q.inner_wall_width),
            ("Obere Fläche", q.top_width),
            ("Massive Füllung", q.solid_width),
            ("Füllung", q.sparse_width),
            ("Stützen", q.support_width),
        ] {
            if !pos(w) || w < p.nozzle * 0.6 || w > p.nozzle * 2.5 {
                e.push(format!("Linienbreite „{}“ ({:.2} mm) muss zwischen {:.2} und {:.2} mm liegen.", n, w, p.nozzle * 0.6, p.nozzle * 2.5));
            }
        }
        for (n, s) in [
            ("Erste Schicht", q.first_layer_speed),
            ("Außenwand", q.outer_wall_speed),
            ("Innenwand", q.inner_wall_speed),
            ("Füllung", q.sparse_speed),
            ("Massive Füllung", q.solid_speed),
            ("Obere Fläche", q.top_speed),
            ("Leerfahrt", q.travel_speed),
        ] {
            if !pos(s) {
                e.push(format!("Geschwindigkeit „{}“ muss größer als 0 sein.", n));
            }
        }
        if !(0.0..=1.0).contains(&q.infill_density) {
            e.push("Fülldichte muss zwischen 0 und 100 % liegen.".into());
        }
        if !pos(q.support_spacing) || !pos(q.interface_spacing) {
            e.push("Stützenabstand muss größer als 0 sein.".into());
        }
        if f.nozzle_temp > 500.0 || f.bed_temp > 200.0 {
            e.push("Temperatur unrealistisch hoch.".into());
        }
        e
    }
}
