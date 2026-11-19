//! 3D models of footprints (KiCad's `(model …)`): resolving the path with KiCad's
//! variables, loading STEP (preferred over WRL, as KiCad ships both), caching, and placing
//! the mesh on the board.

use crate::model::features::ImportFace;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

static PROJECT_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);
static EXTRA_DIRS: RwLock<Vec<PathBuf>> = RwLock::new(Vec::new());

/// Folder of the open project (for `${KIPRJMOD}`).
pub fn set_project_dir(d: Option<PathBuf>) {
    *PROJECT_DIR.write().unwrap() = d;
}

/// Additional 3D model folders (searched by file name).
pub fn add_model_dir(d: PathBuf) {
    let mut g = EXTRA_DIRS.write().unwrap();
    if !g.contains(&d) {
        g.push(d);
    }
}

/// KiCad's standard 3D model folders on this machine.
fn kicad_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    for var in ["KICAD9_3DMODEL_DIR", "KICAD8_3DMODEL_DIR", "KICAD7_3DMODEL_DIR", "KICAD6_3DMODEL_DIR", "KISYS3DMOD"] {
        if let Some(x) = std::env::var_os(var) {
            v.push(PathBuf::from(x));
        }
    }
    v.push(PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport/3dmodels"));
    v.push(PathBuf::from("/usr/share/kicad/3dmodels"));
    v.push(PathBuf::from("/usr/local/share/kicad/3dmodels"));
    v.push(PathBuf::from("C:\\Program Files\\KiCad\\9.0\\share\\kicad\\3dmodels"));
    v.push(PathBuf::from("C:\\Program Files\\KiCad\\8.0\\share\\kicad\\3dmodels"));
    v
}

/// Expands `${VAR}` and finds the file; a .wrl name is also tried as .step/.stp.
pub fn resolve(model: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![];
    let mut s = model.to_string();
    if let Some(start) = s.find("${") {
        if let Some(end) = s[start..].find('}') {
            let var = s[start + 2..start + end].to_string();
            let rest = s[start + end + 1..].trim_start_matches(['/', '\\']).to_string();
            let bases: Vec<PathBuf> = if var == "KIPRJMOD" {
                PROJECT_DIR.read().unwrap().iter().cloned().collect()
            } else {
                std::env::var_os(&var).map(PathBuf::from).into_iter().chain(kicad_dirs()).collect()
            };
            for b in bases {
                candidates.push(b.join(&rest));
            }
            s = rest;
        }
    } else {
        let p = PathBuf::from(&s);
        if p.is_absolute() {
            candidates.push(p);
        } else {
            if let Some(d) = PROJECT_DIR.read().unwrap().as_ref() {
                candidates.push(d.join(&s));
            }
            for b in kicad_dirs() {
                candidates.push(b.join(&s));
            }
        }
    }
    // folders added by the user: by file name
    if let Some(name) = Path::new(&s).file_name() {
        for d in EXTRA_DIRS.read().unwrap().iter() {
            candidates.push(d.join(name));
        }
    }
    for c in candidates {
        for ext in ["step", "stp", "STEP", "STP"] {
            let alt = c.with_extension(ext);
            if alt.is_file() {
                return Some(alt);
            }
        }
        if c.is_file() && c.extension().is_some_and(|e| matches!(e.to_string_lossy().to_lowercase().as_str(), "step" | "stp" | "stl" | "obj")) {
            return Some(c);
        }
    }
    None
}

type Faces = Arc<Vec<ImportFace>>;
static CACHE: Mutex<Option<HashMap<PathBuf, Option<Faces>>>> = Mutex::new(None);

/// The model's faces (model coordinates, mm, z up), loaded once per file.
pub fn load(path: &Path) -> Option<Faces> {
    let mut g = CACHE.lock().unwrap();
    let map = g.get_or_insert_with(HashMap::new);
    if let Some(x) = map.get(path) {
        return x.clone();
    }
    let lower = path.to_string_lossy().to_lowercase();
    let res = std::fs::read(path).ok().and_then(|bytes| {
        let r = if lower.ends_with(".stl") {
            crate::model::step_import::import_stl(&bytes)
        } else if lower.ends_with(".obj") {
            crate::model::step_import::import_obj(&String::from_utf8_lossy(&bytes))
        } else {
            crate::model::step_import::import(&String::from_utf8_lossy(&bytes))
        };
        r.ok()
    });
    let faces = res.map(|r| Arc::new(r.faces.iter().map(|f| ImportFace { v: f.verts.iter().map(|(p, n)| [p.x as f32, p.y as f32, p.z as f32, n.x as f32, n.y as f32, n.z as f32]).collect(), t: f.tris.clone() }).collect::<Vec<_>>()));
    map.insert(path.to_path_buf(), faces.clone());
    faces
}

/// KiCad applies the model rotation (degrees about X, Y, Z) clockwise when looking along
/// each axis: the angles are negated here.
pub fn rotate(v: [f64; 3], r: [f64; 3]) -> [f64; 3] {
    let (mut x, mut y, mut z) = (v[0], v[1], v[2]);
    let (sx, cx) = (-r[0]).to_radians().sin_cos();
    (y, z) = (y * cx - z * sx, y * sx + z * cx);
    let (sy, cy) = (-r[1]).to_radians().sin_cos();
    (x, z) = (x * cy + z * sy, -x * sy + z * cy);
    let (sz, cz) = (-r[2]).to_radians().sin_cos();
    (x, y) = (x * cz - y * sz, x * sz + y * cz);
    [x, y, z]
}
