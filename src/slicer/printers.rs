//! Printer, filament and process presets. The Artillery M1 Pro is taken from Artillery's own
//! Orca Slicer profile (machine "Artillery M1 Pro 0.4 nozzle", filament "Artillery PLA Basic",
//! process "0.20mm Standard"); the other printers use their manufacturers' build volumes with
//! plain start/end G-code that works on every firmware of their kind.

use super::config::*;

const M1_START: &str = r#";===== START G-CODE  =====================
;===== machine: M1 PRO =========================
;===== size: X260 Y260 Z260 =====================
;===== Filament Temperature Configuration =====================
;Nozzle_Preheat_Temp:140
;Bed_Preheat_Temp:[bed_temperature_initial_layer_single]
{if (nozzle_temperature_initial_layer[0] >250)};Filament_Common_Extrusion_Temp:[nozzle_temperature_initial_layer]{else};Filament_Common_Extrusion_Temp:250{endif}
;Nozzle_Init_Layer_Temp:[nozzle_temperature_initial_layer]
;Filament_Softening_Temp:[nozzle_temperature_range_low]
;Chamber_Temp:{chamber_temperature[0]}

;===== Default Settings =====================
M106 S0  ;turn off cool fan
M141 S0  ;turn off PTC
M106 P2 S0  ;turn off chamber fan
M106 P1 S0  ;turn off PTC fan
M106 P3 S0  ;turn off filter fan
M220 S100 ;Reset Feedrate
M221 S100 ;Reset Flowrate
SET_VELOCITY_LIMIT VELOCITY=500;
SET_VELOCITY_LIMIT ACCEL=6000;
SET_VELOCITY_LIMIT MINIMUM_CRUISE_RATIO=0.2;
SET_VELOCITY_LIMIT SQUARE_CORNER_VELOCITY=5;
G21; set units to millimeters
G90

;===== Preheat While Zeroing =====================
M106 P1 S153

M140 S[bed_temperature_initial_layer_single];

{if filament_type[initial_no_support_extruder]=="ABS" || filament_type[initial_no_support_extruder]=="PC" || filament_type[initial_no_support_extruder]=="ASA" || filament_type[initial_no_support_extruder]=="PA" || filament_type[initial_no_support_extruder]=="PA-CF"}
M106 P1 S255
M106 P2 S60
G4 P5000
M191 S47;
M141 S{chamber_temperature[0]};
{endif}

M106 P2 S0
M106 S0;
G4 P500

;===== Wait heating =====================
G28
G1 X20 Y2 Z0  F18000

;===== Set Print Temperature =====================
M106 S120;
G4 P500
M104 S[first_layer_temperature];

{if (bed_temperature_initial_layer_single >0)}
M190 S[bed_temperature_initial_layer_single];
{else}
M140 S[bed_temperature_initial_layer_single];
{endif}

M109 S[first_layer_temperature];
M106 S0;
G4 P500

;===== Wipe Nozzle Extrude Line =====================
G1 X20 Y2 Z0  F18000
M400

G92 E0
G0  E4;
M400

G92 E0
G1 X240 Y2 Z0.2 F1800.0 E11.0;

G92 E0
G1 X240 Y10 Z0.2 F1800.0 E1;

G92 E0
G1 X239.5
G0 E0.2

G92 E0
G0 Y2.5 E0.125

G92 E0
G1 X20 F1800.0 E11;

M400
G92 E0
G1 Z0.6 F600
M400

G1 X{first_layer_print_min[0]} Y{first_layer_print_min[1]} F9000
M400

;===== END G-CODE  =====================
"#;

const M1_END: &str = r#";===== G-CODE START =====================
;===== machine: M1 PRO =========================

;===== Raise Z-Axis =====================
M400 ; wait for buffer to clear
G90
G92 E0 ; zero the extruder
G1 E-0.8 F1800 ; retract

{if max_layer_z < printable_height}G1 Z{z_offset+min(max_layer_z+2, printable_height)} F300 ; Move print head up{endif}
G90
G1 X200 Y260 F21000
G1 X200 Y275 F6000
G1 X160 Y275 F6000
G1 X137 Y275 F3000
M400
TIMELAPSE_TAKE_FRAME
{if max_layer_z < 50}G1 Z50 F300 ; Move print head further up{endif}
;===== Default Settings =====================
M400
M106 S255
M141 S0
M106 P1 S255
M104 S160
G4 P10000
M109 S160
M106 S0  ;turn off cool fan
M141 S0  ;turn off PTC
M106 P2 S0  ;turn off chamber fan
M106 P1 S0  ;turn off PTC fan
M106 P3 S0  ;turn off filter fan
M220 S100 ;Reset Feedrate
M221 S100 ;Reset Flowrate
SET_VELOCITY_LIMIT VELOCITY=500;
SET_VELOCITY_LIMIT ACCEL=6000;
SET_VELOCITY_LIMIT MINIMUM_CRUISE_RATIO=0.2;
SET_VELOCITY_LIMIT SQUARE_CORNER_VELOCITY=5;
G21; set units to millimeters
M104 S0
M140 S0
M400
M84 X Y Z E ; disable motors
M400
;===== G-CODE END =====================
"#;

const M1_LAYER: &str = "G92 E0\n{if layer_num==2}\n;filter_fan\nM106 P3 S{during_print_exhaust_fan_speed[0]*255/100.0}\n;auxiliary_fan\nM106 P2 S{additional_cooling_fan_speed[0]*255/100.0}\n{endif}\n";

/// Plain start G-code: heat, home, prime line along the front edge.
const GENERIC_START: &str = r#"; ---- Freo start G-code ----
G90 ; absolute positioning
M83 ; relative extrusion
M140 S[bed_temperature_initial_layer_single] ; bed temperature
M104 S{min(nozzle_temperature_initial_layer, 160)} ; preheat nozzle without oozing
G28 ; home all axes
M190 S[bed_temperature_initial_layer_single] ; wait for bed
M109 S[nozzle_temperature_initial_layer] ; wait for nozzle
G92 E0
G1 Z2 F600
G1 X{min(10, printable_width*0.05)} Y2 F6000 ; to the prime line
G1 Z[initial_layer_height] F600
G1 X{min(printable_width*0.6, printable_width-10)} E{(min(printable_width*0.6, printable_width-10)-min(10, printable_width*0.05))*0.05} F1500 ; prime line
G1 Y2.6 F3000
G1 X{min(10, printable_width*0.05)+10} E{(min(printable_width*0.6, printable_width-10)-min(10, printable_width*0.05)-10)*0.05} F1500
G92 E0
G1 Z1 F600
"#;

const DELTA_START: &str = r#"; ---- Freo start G-code (delta) ----
G90 ; absolute positioning
M83 ; relative extrusion
M140 S[bed_temperature_initial_layer_single]
M104 S{min(nozzle_temperature_initial_layer, 160)}
G28 ; home all towers
M190 S[bed_temperature_initial_layer_single]
M109 S[nozzle_temperature_initial_layer]
G92 E0
G1 Z5 F3000
G1 X{-printable_width*0.45} Y0 F6000
G1 Z[initial_layer_height] F600
G1 X{-printable_width*0.45} Y{printable_width*0.3} E{printable_width*0.3*0.05} F1500 ; prime arc start
G92 E0
G1 Z1 F600
"#;

const GENERIC_END: &str = r#"; ---- Freo end G-code ----
G91 ; relative positioning
G1 E-2 F2400 ; retract
G1 Z{min(10, printable_height-max_layer_z)} F600 ; lift
G90
{if bed_shape == "round"}G1 X0 Y0 F6000{else}G1 X5 Y{printable_depth*0.9} F6000 ; present the print{endif}
M106 S0 ; fan off
M104 S0 ; nozzle off
M140 S0 ; bed off
M84 ; motors off
"#;

pub fn artillery_m1_pro() -> Printer {
    Printer {
        vendor: "Artillery".into(),
        model: "M1 Pro".into(),
        bed_x: 260.0,
        bed_y: 260.0,
        height: 260.0,
        shape: BedShape::Rect,
        origin_center: false,
        nozzle: 0.4,
        filament_diameter: 1.75,
        flavor: Flavor::Klipper,
        plate: Plate::TexturedPei,
        direct_drive: true,
        max_speed: 700.0,
        max_accel: 20000.0,
        max_z_speed: 20.0,
        corner_velocity: 5.0,
        retract_length: 0.8,
        retract_speed: 40.0,
        deretract_speed: 40.0,
        retract_min_travel: 1.0,
        retract_on_layer_change: true,
        z_hop: 0.6,
        wipe: true,
        wipe_distance: 2.0,
        heated_chamber: true,
        emit_limits: true,
        thumbnail: 300,
        start_gcode: M1_START.into(),
        end_gcode: M1_END.into(),
        layer_gcode: M1_LAYER.into(),
    }
}

/// How a printer moves: sets the default speeds and retraction of a table entry.
#[derive(Clone, Copy)]
enum Kind {
    /// bed slinger, Marlin, ~150 mm/s
    Slinger,
    /// bed slinger with Klipper / input shaping, ~300-500 mm/s
    FastSlinger,
    /// CoreXY with Klipper or Bambu firmware, 500+ mm/s
    CoreXy,
    /// bowden bed slinger
    Bowden,
    Delta,
}

type Row = (&'static str, &'static str, f64, f64, f64, Flavor, Kind, Plate);

/// Build volumes from the manufacturers' data sheets.
const TABLE: &[Row] = &[
    // Artillery
    ("Artillery", "Sidewinder X1", 300.0, 300.0, 400.0, Flavor::Marlin, Kind::Slinger, Plate::Glass),
    ("Artillery", "Sidewinder X2", 300.0, 300.0, 400.0, Flavor::Marlin, Kind::Slinger, Plate::Glass),
    ("Artillery", "Sidewinder X3 Plus", 300.0, 300.0, 400.0, Flavor::Marlin, Kind::Slinger, Plate::TexturedPei),
    ("Artillery", "Sidewinder X3 Pro", 240.0, 240.0, 260.0, Flavor::Marlin, Kind::Slinger, Plate::TexturedPei),
    ("Artillery", "Sidewinder X4 Plus", 300.0, 300.0, 400.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Artillery", "Sidewinder X4 Pro", 240.0, 240.0, 260.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Artillery", "Genius", 220.0, 220.0, 250.0, Flavor::Marlin, Kind::Slinger, Plate::Buildtak),
    ("Artillery", "Genius Pro", 220.0, 220.0, 250.0, Flavor::Marlin, Kind::Slinger, Plate::Glass),
    ("Artillery", "Hornet", 220.0, 220.0, 250.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    // Bambu Lab
    ("Bambu Lab", "X1 Carbon", 256.0, 256.0, 256.0, Flavor::Bambu, Kind::CoreXy, Plate::TexturedPei),
    ("Bambu Lab", "X1E", 256.0, 256.0, 256.0, Flavor::Bambu, Kind::CoreXy, Plate::TexturedPei),
    ("Bambu Lab", "P1S", 256.0, 256.0, 256.0, Flavor::Bambu, Kind::CoreXy, Plate::TexturedPei),
    ("Bambu Lab", "P1P", 256.0, 256.0, 256.0, Flavor::Bambu, Kind::CoreXy, Plate::TexturedPei),
    ("Bambu Lab", "A1", 256.0, 256.0, 256.0, Flavor::Bambu, Kind::FastSlinger, Plate::TexturedPei),
    ("Bambu Lab", "A1 mini", 180.0, 180.0, 180.0, Flavor::Bambu, Kind::FastSlinger, Plate::TexturedPei),
    // Prusa Research
    ("Prusa", "MK4S", 250.0, 210.0, 220.0, Flavor::Marlin, Kind::FastSlinger, Plate::SmoothPei),
    ("Prusa", "MK4", 250.0, 210.0, 220.0, Flavor::Marlin, Kind::FastSlinger, Plate::SmoothPei),
    ("Prusa", "MK3S+", 250.0, 210.0, 210.0, Flavor::Marlin, Kind::Slinger, Plate::SmoothPei),
    ("Prusa", "MINI+", 180.0, 180.0, 180.0, Flavor::Marlin, Kind::Bowden, Plate::SmoothPei),
    ("Prusa", "Core One", 250.0, 220.0, 270.0, Flavor::Marlin, Kind::CoreXy, Plate::SmoothPei),
    ("Prusa", "XL", 360.0, 360.0, 360.0, Flavor::Marlin, Kind::CoreXy, Plate::SmoothPei),
    // Creality
    ("Creality", "Ender-3", 220.0, 220.0, 250.0, Flavor::Marlin, Kind::Bowden, Plate::Buildtak),
    ("Creality", "Ender-3 V2", 220.0, 220.0, 250.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    ("Creality", "Ender-3 S1", 220.0, 220.0, 270.0, Flavor::Marlin, Kind::Slinger, Plate::SmoothPei),
    ("Creality", "Ender-3 S1 Pro", 220.0, 220.0, 270.0, Flavor::Marlin, Kind::Slinger, Plate::SmoothPei),
    ("Creality", "Ender-3 V3 SE", 220.0, 220.0, 250.0, Flavor::Marlin, Kind::Slinger, Plate::SmoothPei),
    ("Creality", "Ender-3 V3 KE", 220.0, 220.0, 240.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Creality", "Ender-3 V3", 220.0, 220.0, 250.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Creality", "Ender-5 S1", 220.0, 220.0, 280.0, Flavor::Marlin, Kind::Slinger, Plate::SmoothPei),
    ("Creality", "Ender-5 Pro", 220.0, 220.0, 300.0, Flavor::Marlin, Kind::Bowden, Plate::Buildtak),
    ("Creality", "CR-10", 300.0, 300.0, 400.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    ("Creality", "CR-10 S5", 500.0, 500.0, 500.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    ("Creality", "CR-10 Smart Pro", 300.0, 300.0, 400.0, Flavor::Marlin, Kind::Slinger, Plate::Glass),
    ("Creality", "K1", 220.0, 220.0, 250.0, Flavor::Klipper, Kind::CoreXy, Plate::SmoothPei),
    ("Creality", "K1C", 220.0, 220.0, 250.0, Flavor::Klipper, Kind::CoreXy, Plate::SmoothPei),
    ("Creality", "K1 Max", 300.0, 300.0, 300.0, Flavor::Klipper, Kind::CoreXy, Plate::SmoothPei),
    ("Creality", "K2 Plus", 350.0, 350.0, 350.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Creality", "Ender-3 Max Neo", 300.0, 300.0, 320.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    // Elegoo
    ("Elegoo", "Neptune 3", 220.0, 220.0, 280.0, Flavor::Marlin, Kind::Slinger, Plate::SmoothPei),
    ("Elegoo", "Neptune 3 Pro", 225.0, 225.0, 280.0, Flavor::Marlin, Kind::Slinger, Plate::SmoothPei),
    ("Elegoo", "Neptune 3 Plus", 320.0, 320.0, 400.0, Flavor::Marlin, Kind::Slinger, Plate::SmoothPei),
    ("Elegoo", "Neptune 3 Max", 420.0, 420.0, 500.0, Flavor::Marlin, Kind::Slinger, Plate::SmoothPei),
    ("Elegoo", "Neptune 4", 225.0, 225.0, 265.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Elegoo", "Neptune 4 Pro", 225.0, 225.0, 265.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Elegoo", "Neptune 4 Plus", 320.0, 320.0, 385.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Elegoo", "Neptune 4 Max", 420.0, 420.0, 480.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Elegoo", "Centauri Carbon", 256.0, 256.0, 256.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    // Anycubic
    ("Anycubic", "Kobra 2", 220.0, 220.0, 250.0, Flavor::Marlin, Kind::FastSlinger, Plate::TexturedPei),
    ("Anycubic", "Kobra 2 Pro", 220.0, 220.0, 250.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Anycubic", "Kobra 2 Plus", 320.0, 320.0, 400.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Anycubic", "Kobra 2 Max", 420.0, 420.0, 500.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Anycubic", "Kobra 3", 250.0, 250.0, 260.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Anycubic", "Kobra S1", 250.0, 250.0, 250.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Anycubic", "Vyper", 245.0, 245.0, 260.0, Flavor::Marlin, Kind::Slinger, Plate::TexturedPei),
    ("Anycubic", "i3 Mega", 210.0, 210.0, 205.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    // Sovol
    ("Sovol", "SV06", 220.0, 220.0, 250.0, Flavor::Marlin, Kind::Slinger, Plate::TexturedPei),
    ("Sovol", "SV06 Plus", 300.0, 300.0, 340.0, Flavor::Marlin, Kind::Slinger, Plate::TexturedPei),
    ("Sovol", "SV07", 220.0, 220.0, 250.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Sovol", "SV07 Plus", 300.0, 300.0, 350.0, Flavor::Klipper, Kind::FastSlinger, Plate::TexturedPei),
    ("Sovol", "SV08", 350.0, 350.0, 345.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    // Voron Design
    ("Voron", "0.2", 120.0, 120.0, 120.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Voron", "Trident 250", 250.0, 250.0, 250.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Voron", "Trident 300", 300.0, 300.0, 250.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Voron", "Trident 350", 350.0, 350.0, 250.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Voron", "2.4 250", 250.0, 250.0, 250.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Voron", "2.4 300", 300.0, 300.0, 300.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Voron", "2.4 350", 350.0, 350.0, 350.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    // Qidi
    ("Qidi", "X-Plus 3", 280.0, 280.0, 270.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Qidi", "X-Max 3", 325.0, 325.0, 315.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Qidi", "Q1 Pro", 245.0, 245.0, 240.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Qidi", "Plus4", 305.0, 305.0, 280.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    // Flashforge
    ("Flashforge", "Adventurer 5M", 220.0, 220.0, 220.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Flashforge", "Adventurer 5M Pro", 220.0, 220.0, 220.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    ("Flashforge", "AD5X", 220.0, 220.0, 220.0, Flavor::Klipper, Kind::CoreXy, Plate::TexturedPei),
    // Anker
    ("AnkerMake", "M5", 235.0, 235.0, 250.0, Flavor::Marlin, Kind::FastSlinger, Plate::TexturedPei),
    ("AnkerMake", "M5C", 220.0, 220.0, 250.0, Flavor::Marlin, Kind::FastSlinger, Plate::TexturedPei),
    // others
    ("Ultimaker", "S3", 230.0, 190.0, 200.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    ("Ultimaker", "S5", 330.0, 240.0, 300.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    ("Ultimaker", "2+", 223.0, 220.0, 205.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    ("Tronxy", "X5SA", 330.0, 330.0, 400.0, Flavor::Marlin, Kind::Bowden, Plate::Buildtak),
    ("Two Trees", "Sapphire Pro", 235.0, 235.0, 235.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    ("Kingroon", "KP3S", 180.0, 180.0, 180.0, Flavor::Marlin, Kind::Slinger, Plate::Buildtak),
    ("BIQU", "B1", 235.0, 235.0, 270.0, Flavor::Marlin, Kind::Bowden, Plate::Glass),
    ("Mingda", "Magician X", 230.0, 230.0, 260.0, Flavor::Marlin, Kind::Slinger, Plate::Buildtak),
    // deltas: bed diameter × height
    ("FLSun", "V400", 300.0, 300.0, 410.0, Flavor::Klipper, Kind::Delta, Plate::Glass),
    ("FLSun", "Super Racer", 260.0, 260.0, 330.0, Flavor::Marlin, Kind::Delta, Plate::Glass),
    ("Anycubic", "Kossel Linear Plus", 230.0, 230.0, 300.0, Flavor::Marlin, Kind::Delta, Plate::Glass),
];

fn from_row(r: &Row) -> Printer {
    let (vendor, model, x, y, z, flavor, kind, plate) = *r;
    let (max_speed, max_accel, corner, retract, rspeed, hop, direct) = match kind {
        Kind::Slinger => (200.0, 1500.0, 8.0, 0.8, 40.0, 0.4, true),
        Kind::FastSlinger => (500.0, 8000.0, 8.0, 0.8, 40.0, 0.4, true),
        Kind::CoreXy => (500.0, 15000.0, 5.0, 0.8, 30.0, 0.4, true),
        Kind::Bowden => (150.0, 1000.0, 10.0, 5.0, 45.0, 0.4, false),
        Kind::Delta => (300.0, 3000.0, 10.0, 4.0, 45.0, 0.4, false),
    };
    let delta = matches!(kind, Kind::Delta);
    let ultimaker = vendor == "Ultimaker";
    Printer {
        vendor: vendor.into(),
        model: model.into(),
        bed_x: x,
        bed_y: if delta { x } else { y },
        height: z,
        shape: if delta { BedShape::Round } else { BedShape::Rect },
        origin_center: delta,
        nozzle: 0.4,
        filament_diameter: if ultimaker { 2.85 } else { 1.75 },
        flavor,
        plate,
        direct_drive: direct,
        max_speed,
        max_accel,
        max_z_speed: if delta { max_speed } else { 12.0 },
        corner_velocity: corner,
        retract_length: if ultimaker { 6.5 } else { retract },
        retract_speed: rspeed,
        deretract_speed: rspeed,
        retract_min_travel: if direct { 1.0 } else { 2.0 },
        retract_on_layer_change: true,
        z_hop: hop,
        wipe: false,
        wipe_distance: 1.0,
        heated_chamber: false,
        emit_limits: false,
        thumbnail: if matches!(flavor, Flavor::Klipper) { 300 } else { 0 },
        start_gcode: if delta { DELTA_START.into() } else { GENERIC_START.into() },
        end_gcode: GENERIC_END.into(),
        layer_gcode: String::new(),
    }
}

/// Every built-in printer: the Artillery M1 Pro first, then the table by vendor.
pub fn all_printers() -> Vec<Printer> {
    let mut v = vec![artillery_m1_pro()];
    v.extend(TABLE.iter().map(from_row));
    v
}

/// A printer of any size the user describes (Benutzerdefiniert).
pub fn custom_printer(x: f64, y: f64, z: f64, round: bool) -> Printer {
    let mut p = from_row(&("", "Benutzerdefinierter Drucker", x, y, z, Flavor::Marlin, if round { Kind::Delta } else { Kind::Slinger }, Plate::TexturedPei));
    p.vendor = String::new();
    p
}

#[allow(clippy::too_many_arguments)]
fn fil(name: &str, m: Material, color: [u8; 3], density: f64, cost: f64, t: f64, t1: f64, bed: f64, bed1: f64, mvs: f64, fan: (f64, f64), always: bool, overhang_fan: f64, slow_time: f64) -> Filament {
    Filament {
        name: name.into(),
        material: m,
        color,
        diameter: 1.75,
        density,
        cost,
        flow_ratio: 0.98,
        nozzle_temp: t,
        nozzle_temp_first: t1,
        bed_temp: bed,
        bed_temp_first: bed1,
        chamber_temp: if m.enclosure() { 47.0 } else { 0.0 },
        max_volumetric: mvs,
        fan_min: fan.0,
        fan_max: fan.1,
        fan_off_layers: 1,
        fan_always_on: always,
        overhang_fan,
        slow_down_layer_time: slow_time,
        fan_cooling_layer_time: 60.0,
        slow_down_for_layer_cooling: true,
        slow_down_min_speed: 15.0,
        retract_length: None,
        z_hop: None,
        pressure_advance: None,
        aux_fan: if m.enclosure() { 0.0 } else { 50.0 },
        exhaust_fan: if m.enclosure() { 0.0 } else { 60.0 },
    }
}

pub fn filament_presets() -> Vec<Filament> {
    let mut pla = fil("Artillery PLA Basic", Material::Pla, [236, 236, 236], 1.24, 20.0, 220.0, 220.0, 55.0, 55.0, 21.0, (1.0, 1.0), true, 1.0, 6.0);
    pla.fan_cooling_layer_time = 100.0;
    pla.pressure_advance = Some(0.032);
    vec![
        pla,
        fil("Generic PLA", Material::Pla, [235, 90, 40], 1.24, 20.0, 210.0, 215.0, 60.0, 60.0, 15.0, (1.0, 1.0), true, 1.0, 8.0),
        fil("Generic PLA+", Material::PlaPlus, [40, 40, 44], 1.24, 22.0, 220.0, 225.0, 60.0, 60.0, 15.0, (1.0, 1.0), true, 1.0, 8.0),
        fil("Generic PLA Silk", Material::PlaSilk, [212, 175, 55], 1.24, 25.0, 225.0, 225.0, 60.0, 60.0, 7.5, (1.0, 1.0), true, 1.0, 8.0),
        fil("Generic PLA Matte", Material::PlaMatte, [92, 110, 88], 1.32, 22.0, 220.0, 220.0, 60.0, 60.0, 15.0, (1.0, 1.0), true, 1.0, 8.0),
        fil("Generic PLA-CF", Material::PlaCf, [48, 48, 50], 1.21, 35.0, 230.0, 230.0, 60.0, 60.0, 12.0, (1.0, 1.0), true, 1.0, 8.0),
        fil("Generic PETG", Material::Petg, [30, 110, 200], 1.27, 22.0, 240.0, 245.0, 75.0, 80.0, 12.0, (0.3, 0.5), true, 0.9, 8.0),
        fil("Generic PETG-CF", Material::PetgCf, [52, 52, 54], 1.29, 35.0, 250.0, 250.0, 75.0, 80.0, 10.0, (0.3, 0.5), true, 0.9, 8.0),
        fil("Generic ABS", Material::Abs, [220, 30, 36], 1.04, 22.0, 250.0, 255.0, 100.0, 100.0, 16.0, (0.1, 0.3), false, 0.8, 12.0),
        fil("Generic ASA", Material::Asa, [245, 245, 240], 1.07, 25.0, 255.0, 260.0, 100.0, 100.0, 15.0, (0.1, 0.3), false, 0.8, 12.0),
        fil("Generic TPU 95A", Material::Tpu, [40, 160, 90], 1.21, 30.0, 225.0, 230.0, 40.0, 40.0, 3.6, (1.0, 1.0), true, 1.0, 8.0),
        fil("Generic PA", Material::Pa, [230, 226, 210], 1.14, 40.0, 270.0, 275.0, 90.0, 90.0, 8.0, (0.0, 0.2), false, 0.6, 12.0),
        fil("Generic PA-CF", Material::PaCf, [44, 44, 46], 1.2, 60.0, 280.0, 285.0, 90.0, 90.0, 8.0, (0.0, 0.2), false, 0.6, 12.0),
        fil("Generic PC", Material::Pc, [210, 225, 235], 1.2, 40.0, 270.0, 275.0, 110.0, 110.0, 12.0, (0.0, 0.2), false, 0.6, 12.0),
        fil("Generic PVA", Material::Pva, [250, 248, 225], 1.23, 70.0, 220.0, 220.0, 55.0, 55.0, 8.0, (1.0, 1.0), true, 1.0, 8.0),
        fil("Generic HIPS", Material::Hips, [250, 250, 250], 1.04, 25.0, 240.0, 245.0, 100.0, 100.0, 8.0, (0.1, 0.3), false, 0.8, 12.0),
        fil("Generic PP", Material::Pp, [235, 232, 220], 0.9, 30.0, 230.0, 235.0, 80.0, 80.0, 8.0, (0.2, 0.4), true, 0.8, 10.0),
    ]
}

/// Filament colour swatches offered in the colour menu.
pub const COLORS: &[(&str, [u8; 3])] = &[
    ("Weiß", [236, 236, 236]),
    ("Schwarz", [34, 34, 36]),
    ("Grau", [140, 142, 146]),
    ("Silber", [192, 196, 200]),
    ("Rot", [214, 32, 40]),
    ("Orange", [240, 120, 24]),
    ("Gelb", [246, 206, 30]),
    ("Grün", [93, 190, 60]),
    ("Dunkelgrün", [24, 110, 56]),
    ("Hellblau", [90, 180, 230]),
    ("Blau", [28, 84, 190]),
    ("Violett", [120, 60, 170]),
    ("Pink", [236, 104, 170]),
    ("Braun", [120, 80, 50]),
    ("Beige", [222, 200, 160]),
    ("Gold", [212, 175, 55]),
    ("Transparent", [210, 228, 236]),
];

/// Speed set of a printer class (outer, inner, sparse, solid, top, travel, accel, outer
/// accel, top accel, first layer accel).
fn speeds(p: &Printer) -> [f64; 10] {
    if p.max_speed >= 600.0 {
        // the Artillery M1 Pro profile
        [200.0, 300.0, 270.0, 250.0, 200.0, 700.0, 10000.0, 5000.0, 2000.0, 500.0]
    } else if p.max_speed >= 450.0 {
        [160.0, 250.0, 250.0, 220.0, 150.0, 500.0, 8000.0, 4000.0, 2000.0, 500.0]
    } else if p.max_speed >= 250.0 {
        [100.0, 150.0, 150.0, 120.0, 80.0, 250.0, 3000.0, 2000.0, 1500.0, 500.0]
    } else if p.max_speed >= 180.0 {
        [50.0, 80.0, 100.0, 80.0, 50.0, 150.0, 1000.0, 800.0, 800.0, 500.0]
    } else {
        [40.0, 60.0, 80.0, 60.0, 40.0, 120.0, 800.0, 600.0, 600.0, 400.0]
    }
}

/// Orca's six quality presets for a printer (0.08 … 0.28 mm on a 0.4 mm nozzle; other
/// nozzles scale). Index 3 is "Standard".
pub fn process_presets_for(p: &Printer) -> Vec<Process> {
    let n = p.nozzle;
    let k = n / 0.4;
    let s = speeds(p);
    let names = ["Extra fein", "Fein", "Optimal", "Standard", "Entwurf", "Extra Entwurf"];
    let hs = [0.08, 0.12, 0.16, 0.20, 0.24, 0.28];
    names
        .iter()
        .zip(hs)
        .map(|(name, h0)| {
            let h = ((h0 * k) * 100.0).round() / 100.0;
            let top = ((1.0 / h).ceil() as u32).max(3);
            let bottom = ((0.6 / h).ceil() as u32).max(3);
            let lw = |f: f64| ((n * f) * 100.0).round() / 100.0;
            Process {
                name: format!("{:.2} mm {}", h, name),
                layer_height: h,
                first_layer_height: ((0.2 * k).max(h) * 100.0).round() / 100.0,
                line_width: lw(1.05),
                first_layer_width: lw(1.25),
                outer_wall_width: lw(1.05),
                inner_wall_width: lw(1.125),
                top_width: lw(1.05),
                solid_width: lw(1.05),
                sparse_width: lw(1.125),
                support_width: lw(1.05),
                seam: Seam::Aligned,
                seam_gap: 0.1,
                elephant_foot: 0.15,
                resolution: 0.012,
                ironing: Ironing::None,
                ironing_flow: 0.15,
                ironing_spacing: 0.1,
                ironing_speed: 15.0,
                fuzzy: Fuzzy::None,
                fuzzy_thickness: 0.3,
                fuzzy_point_dist: 0.8,
                detect_overhang_wall: true,
                wall_loops: 2,
                wall_order: WallOrder::InnerOuter,
                only_one_wall_top: true,
                top_layers: top,
                bottom_layers: bottom,
                top_pattern: InfillPattern::Monotonic,
                bottom_pattern: InfillPattern::Monotonic,
                solid_pattern: InfillPattern::ZigZag,
                infill_density: 0.15,
                infill_pattern: InfillPattern::Grid,
                infill_angle: 45.0,
                infill_overlap: 0.15,
                min_sparse_area: 15.0,
                infill_combine_connect: true,
                spiral_vase: false,
                first_layer_speed: 30.0,
                first_layer_infill_speed: if s[0] >= 150.0 { 80.0 } else { 40.0 },
                outer_wall_speed: s[0],
                inner_wall_speed: s[1],
                sparse_speed: s[2],
                solid_speed: s[3],
                top_speed: s[4],
                gap_speed: s[3],
                support_speed: (s[2] * 0.55).round(),
                interface_speed: (s[0] * 0.4).round().max(30.0),
                bridge_speed: 50.0_f64.min(s[0]),
                overhang_speed: 30.0_f64.min(s[0]),
                travel_speed: s[5].min(p.max_speed),
                first_layer_travel_speed: s[5].min(p.max_speed),
                default_accel: s[6].min(p.max_accel),
                outer_wall_accel: s[7].min(p.max_accel),
                top_accel: s[8].min(p.max_accel),
                first_layer_accel: s[9].min(p.max_accel),
                travel_accel: p.max_accel,
                slow_down_layers: 3,
                support: false,
                support_threshold: 30.0,
                support_build_plate_only: false,
                support_top_z: ((0.16 * k).max(h * 0.8) * 100.0).round() / 100.0,
                support_bottom_z: ((0.2 * k).max(h) * 100.0).round() / 100.0,
                support_xy: 0.5,
                support_spacing: 2.5,
                support_angle: 0.0,
                interface_layers: 2,
                interface_spacing: 0.5,
                support_remove_small: true,
                skirt_loops: 0,
                skirt_distance: 2.0,
                skirt_height: 1,
                skirt_speed: 50.0,
                brim: Brim::None,
                brim_width: 5.0,
                brim_gap: 0.1,
                bridge_flow: 0.95,
                reduce_infill_retraction: true,
                gcode_comments: false,
            }
        })
        .collect()
}

pub fn process_presets(nozzle: f64) -> Vec<Process> {
    let mut p = artillery_m1_pro();
    p.nozzle = nozzle;
    process_presets_for(&p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid() {
        for p in all_printers() {
            for q in process_presets_for(&p) {
                let s = Settings { printer: p.clone(), filament: filament_presets()[0].clone(), process: q };
                assert!(s.validate().is_empty(), "{}: {:?}", p.name(), s.validate());
            }
        }
        let m1 = artillery_m1_pro();
        assert_eq!((m1.bed_x, m1.bed_y, m1.height), (260.0, 260.0, 260.0));
        let std = &process_presets_for(&m1)[3];
        assert_eq!(std.layer_height, 0.2);
        assert_eq!((std.top_layers, std.bottom_layers), (5, 3));
        assert_eq!((std.outer_wall_speed, std.inner_wall_speed, std.sparse_speed), (200.0, 300.0, 270.0));
        assert!(all_printers().len() > 80);
    }
}
