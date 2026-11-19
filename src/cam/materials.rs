//! Workpiece materials with cutting data for solid carbide end mills. Values are
//! conservative starting points from tool makers' tables (cutting speed vc, feed per tooth
//! as a fraction of the tool diameter, depth of cut as a fraction of the diameter for a
//! hobby router; mills get more).

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, serde::Serialize, serde::Deserialize)]
pub enum Group {
    Wood,
    Plastic,
    Metal,
}

impl Group {
    pub fn name(&self) -> &'static str {
        match self {
            Group::Wood => "Holz",
            Group::Plastic => "Kunststoff",
            Group::Metal => "Metall",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub name: &'static str,
    pub group: Group,
    /// cutting speed for carbide, m/min
    pub vc: f64,
    /// feed per tooth = fz_k × diameter (mm)
    pub fz_k: f64,
    /// roughing depth of cut = ap_k × diameter on a router
    pub ap_k: f64,
    /// roughing stepover = ae_k × diameter
    pub ae_k: f64,
    /// steepest ramp entry, degrees
    pub ramp: f64,
    /// g/cm³
    pub density: f64,
    pub coolant: bool,
    /// a hobby router cannot cut it safely
    pub needs_mill: bool,
    pub color: [u8; 3],
    /// shading: 0 wood, 1 plastic, 2 metal
    pub look: u8,
    pub note: &'static str,
}

const fn m(name: &'static str, group: Group, vc: f64, fz_k: f64, ap_k: f64, ae_k: f64, ramp: f64, density: f64, coolant: bool, needs_mill: bool, color: [u8; 3], note: &'static str) -> Material {
    let look = match group {
        Group::Wood => 0,
        Group::Plastic => 1,
        Group::Metal => 2,
    };
    Material { name, group, vc, fz_k, ap_k, ae_k, ramp, density, coolant, needs_mill, color, look, note }
}

use Group::*;

pub const MATERIALS: &[Material] = &[
    // ---- wood ----
    m("Weichholz (Kiefer, Fichte)", Wood, 800.0, 0.025, 1.0, 0.45, 10.0, 0.5, false, false, [222, 184, 135], ""),
    m("Hartholz (Eiche)", Wood, 600.0, 0.02, 0.75, 0.45, 8.0, 0.75, false, false, [176, 132, 82], ""),
    m("Buche", Wood, 600.0, 0.02, 0.75, 0.45, 8.0, 0.72, false, false, [214, 168, 120], ""),
    m("Ahorn", Wood, 600.0, 0.02, 0.75, 0.45, 8.0, 0.7, false, false, [232, 206, 160], ""),
    m("Nussbaum", Wood, 600.0, 0.02, 0.75, 0.45, 8.0, 0.65, false, false, [112, 76, 50], ""),
    m("Birke-Multiplex", Wood, 600.0, 0.02, 0.75, 0.45, 8.0, 0.68, false, false, [226, 196, 150], ""),
    m("MDF", Wood, 700.0, 0.025, 1.0, 0.45, 10.0, 0.75, false, false, [176, 142, 100], "Feiner Staub: Absaugung verwenden."),
    m("HDF", Wood, 600.0, 0.02, 0.75, 0.45, 8.0, 0.9, false, false, [150, 112, 74], "Feiner Staub: Absaugung verwenden."),
    m("Bambus", Wood, 500.0, 0.018, 0.6, 0.4, 8.0, 0.7, false, false, [214, 186, 120], ""),
    m("Balsa", Wood, 900.0, 0.03, 1.0, 0.5, 15.0, 0.15, false, false, [236, 224, 196], ""),
    // ---- plastics (20) ----
    m("PMMA (Acryl) gegossen", Plastic, 400.0, 0.02, 0.5, 0.4, 5.0, 1.19, false, false, [214, 232, 238], "Einschneider verwenden, Späne gut abführen (Schmelzgefahr)."),
    m("PMMA (Acryl) extrudiert", Plastic, 300.0, 0.015, 0.4, 0.35, 4.0, 1.19, false, false, [206, 228, 236], "Schmilzt leichter als gegossenes Acryl."),
    m("Polycarbonat (PC)", Plastic, 350.0, 0.02, 0.5, 0.4, 5.0, 1.2, false, false, [220, 226, 230], ""),
    m("HDPE", Plastic, 500.0, 0.025, 0.75, 0.45, 8.0, 0.95, false, false, [244, 244, 240], ""),
    m("UHMW-PE", Plastic, 450.0, 0.025, 0.6, 0.45, 8.0, 0.93, false, false, [246, 246, 244], ""),
    m("Polypropylen (PP)", Plastic, 450.0, 0.025, 0.6, 0.45, 8.0, 0.9, false, false, [238, 238, 228], ""),
    m("POM-C (Acetal)", Plastic, 400.0, 0.02, 0.75, 0.45, 6.0, 1.41, false, false, [248, 248, 244], ""),
    m("POM-H (Delrin)", Plastic, 400.0, 0.02, 0.75, 0.45, 6.0, 1.42, false, false, [250, 250, 246], ""),
    m("PA6 (Nylon)", Plastic, 350.0, 0.02, 0.6, 0.4, 6.0, 1.14, false, false, [240, 236, 222], ""),
    m("PA66", Plastic, 350.0, 0.02, 0.6, 0.4, 6.0, 1.14, false, false, [236, 232, 216], ""),
    m("PET-P", Plastic, 300.0, 0.018, 0.5, 0.4, 5.0, 1.39, false, false, [242, 242, 238], ""),
    m("PVC", Plastic, 300.0, 0.018, 0.5, 0.4, 5.0, 1.4, false, false, [150, 154, 160], "Beim Überhitzen entsteht Salzsäuregas: scharfe Werkzeuge, nicht schmelzen lassen."),
    m("ABS", Plastic, 350.0, 0.02, 0.6, 0.4, 6.0, 1.05, false, false, [230, 226, 214], ""),
    m("PTFE (Teflon)", Plastic, 300.0, 0.02, 0.5, 0.4, 5.0, 2.2, false, false, [250, 250, 250], "Nicht überhitzen: Zersetzungsgase sind giftig."),
    m("PEEK", Plastic, 250.0, 0.012, 0.4, 0.35, 4.0, 1.3, false, false, [200, 180, 150], ""),
    m("PEI (Ultem)", Plastic, 250.0, 0.012, 0.4, 0.35, 4.0, 1.27, false, false, [214, 160, 70], ""),
    m("PVDF", Plastic, 300.0, 0.015, 0.5, 0.4, 5.0, 1.78, false, false, [240, 240, 236], ""),
    m("Hartgewebe (Phenol)", Plastic, 250.0, 0.012, 0.4, 0.35, 4.0, 1.35, false, false, [140, 92, 50], ""),
    m("GFK (G10/FR4)", Plastic, 150.0, 0.008, 0.25, 0.3, 3.0, 1.85, false, false, [196, 206, 140], "Glasstaub ist gesundheitsschädlich: Absaugung und Maske. Sehr abrasiv."),
    m("CFK (Kohlefaser)", Plastic, 180.0, 0.008, 0.25, 0.3, 3.0, 1.6, false, false, [40, 40, 44], "Kohlefaserstaub ist leitfähig und gesundheitsschädlich: Absaugung, Maske, Elektronik schützen."),
    // ---- metals (20) ----
    m("Aluminium 6061-T6 (AlMgSi1)", Metal, 300.0, 0.008, 0.25, 0.25, 2.0, 2.7, true, false, [206, 210, 214], "Mit Luft oder Sprühnebel kühlen, damit Späne nicht kleben."),
    m("Aluminium 7075-T6 (AlZnMgCu)", Metal, 280.0, 0.007, 0.25, 0.25, 2.0, 2.81, true, false, [200, 204, 210], ""),
    m("Aluminium 5083 (AlMg4,5Mn)", Metal, 250.0, 0.007, 0.2, 0.25, 2.0, 2.66, true, false, [208, 210, 212], "Neigt zum Schmieren: scharfe Werkzeuge, Kühlung."),
    m("Aluminium 2024 (AlCuMg)", Metal, 280.0, 0.007, 0.25, 0.25, 2.0, 2.78, true, false, [202, 204, 208], ""),
    m("Aluguss (AlSi)", Metal, 250.0, 0.007, 0.25, 0.25, 2.0, 2.68, true, false, [190, 194, 198], "Abrasiv durch Silizium."),
    m("Messing CuZn39Pb3", Metal, 200.0, 0.007, 0.25, 0.3, 2.0, 8.47, false, false, [214, 180, 90], ""),
    m("Bronze CuSn8", Metal, 120.0, 0.005, 0.15, 0.25, 1.5, 8.8, true, true, [184, 130, 74], ""),
    m("Kupfer Cu-ETP", Metal, 150.0, 0.005, 0.15, 0.25, 1.5, 8.94, true, true, [206, 124, 82], "Schmiert: scharfe, polierte Werkzeuge."),
    m("Baustahl S235", Metal, 120.0, 0.005, 0.1, 0.2, 1.0, 7.85, true, true, [150, 154, 160], ""),
    m("Vergütungsstahl C45", Metal, 110.0, 0.0045, 0.1, 0.2, 1.0, 7.85, true, true, [144, 148, 156], ""),
    m("Vergütungsstahl 42CrMo4", Metal, 90.0, 0.004, 0.08, 0.15, 1.0, 7.72, true, true, [138, 142, 150], ""),
    m("Werkzeugstahl 1.2379 (D2)", Metal, 70.0, 0.0035, 0.06, 0.12, 1.0, 7.7, true, true, [132, 136, 144], ""),
    m("Edelstahl 1.4301 (304)", Metal, 90.0, 0.004, 0.08, 0.15, 1.0, 7.9, true, true, [176, 180, 186], "Verfestigt sich: nie reiben lassen, konstant schneiden."),
    m("Edelstahl 1.4404 (316L)", Metal, 80.0, 0.004, 0.08, 0.15, 1.0, 8.0, true, true, [172, 176, 182], "Verfestigt sich: nie reiben lassen, konstant schneiden."),
    m("Edelstahl 1.4305 (303)", Metal, 110.0, 0.0045, 0.1, 0.2, 1.0, 7.9, true, true, [178, 182, 188], ""),
    m("Grauguss GG25", Metal, 120.0, 0.005, 0.1, 0.2, 1.0, 7.2, false, true, [110, 112, 116], "Graphitstaub: Absaugung."),
    m("Titan Grade 2", Metal, 60.0, 0.004, 0.08, 0.12, 1.0, 4.51, true, true, [160, 158, 150], "Viel Kühlmittel, niedrige Schnittgeschwindigkeit, Späne können brennen."),
    m("Titan Grade 5 (Ti6Al4V)", Metal, 45.0, 0.0035, 0.06, 0.1, 1.0, 4.43, true, true, [150, 148, 142], "Viel Kühlmittel, niedrige Schnittgeschwindigkeit, Späne können brennen."),
    m("Inconel 718", Metal, 25.0, 0.003, 0.04, 0.08, 0.5, 8.19, true, true, [140, 140, 136], "Nur auf steifen Maschinen; Werkzeuge verschleißen schnell."),
    m("Magnesium AZ31", Metal, 400.0, 0.008, 0.3, 0.3, 2.0, 1.77, false, false, [196, 198, 196], "BRANDGEFAHR: Späne sind brennbar. Kein Wasser-Kühlmittel, Klasse-D-Löscher bereithalten."),
];

pub fn by_group(g: Group) -> impl Iterator<Item = (usize, &'static Material)> {
    MATERIALS.iter().enumerate().filter(move |(_, m)| m.group == g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_metals_and_plastics() {
        assert_eq!(by_group(Metal).count(), 20);
        assert_eq!(by_group(Plastic).count(), 20);
        assert!(by_group(Wood).count() >= 10);
        for m in MATERIALS {
            assert!(m.vc > 0.0 && m.fz_k > 0.0 && m.ap_k > 0.0 && m.ae_k > 0.0 && m.ae_k <= 0.5 && m.ramp > 0.0, "{}", m.name);
        }
        let mut names: Vec<_> = MATERIALS.iter().map(|m| m.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), MATERIALS.len());
    }
}
