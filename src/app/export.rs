//! Neutral-format export (STEP, STL, OBJ, PLY, VRML, glTF, 3MF) and mass-property analysis.

use crate::kernel::math::*;
use crate::model::features::ModelData;
use crate::model::regen::RegenResult;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format { Step, StlBinary, StlAscii, Obj, Ply, Vrml, Gltf, ThreeMf }

impl Format {
    pub fn ext(&self) -> &'static str {
        match self {
            Format::Step => "stp", Format::StlBinary | Format::StlAscii => "stl", Format::Obj => "obj", Format::Ply => "ply",
            Format::Vrml => "wrl", Format::Gltf => "gltf", Format::ThreeMf => "3mf",
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Format::Step => "STEP AP214 (facettiert)", Format::StlBinary => "STL (binär)", Format::StlAscii => "STL (ASCII)", Format::Obj => "Wavefront OBJ",
            Format::Ply => "PLY (binär)", Format::Vrml => "VRML 2.0", Format::Gltf => "glTF 2.0", Format::ThreeMf => "3MF (3D-Druck)",
        }
    }
    /// What the format is typically used for, shown in the export dialog.
    pub fn hint(&self) -> &'static str {
        match self {
            Format::Step => "Austausch mit anderen CAD-Systemen",
            Format::StlBinary | Format::StlAscii => "3D-Druck, Slicer, Netzbearbeitung",
            Format::Obj => "Rendering, Blender, Spiele-Engines",
            Format::Ply => "Scannen, Punktwolken, MeshLab",
            Format::Vrml => "Web-3D und ältere Viewer, mit Farbe",
            Format::Gltf => "Web, AR, Three.js, Blender, mit Farbe",
            Format::ThreeMf => "3D-Druck (Prusa, Bambu, Cura), mit Einheit mm und Farbe",
        }
    }
    pub const ALL: [Format; 8] = [Format::Step, Format::StlBinary, Format::StlAscii, Format::Obj, Format::Ply, Format::Vrml, Format::Gltf, Format::ThreeMf];
}

/// Indexed triangle mesh: (positions, triangles), polygons fanned into triangles.
pub fn indexed(r: &RegenResult) -> (Vec<V3>, Vec<[u32; 3]>) {
    let t = &r.topo;
    let mut tris = vec![];
    for p in &t.polys {
        for k in 1..p.len().saturating_sub(1) { tris.push([p[0] as u32, p[k] as u32, p[k + 1] as u32]); }
    }
    (t.positions.clone(), tris)
}

fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, e) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 { c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 }; }
        *e = c;
    }
    !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

/// Minimal ZIP writer (stored entries), enough for the 3MF package.
fn zip_stored(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![];
    let mut central = vec![];
    for (name, data) in entries {
        let off = out.len() as u32;
        let crc = crc32(data);
        let n = name.as_bytes();
        let local = |sig: u32, out: &mut Vec<u8>, central: bool| {
            out.extend_from_slice(&sig.to_le_bytes());
            if central { out.extend_from_slice(&20u16.to_le_bytes()); }
            for v in [20u16, 0, 0, 0, 0x21] { out.extend_from_slice(&v.to_le_bytes()); }
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(n.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
        };
        local(0x0403_4b50, &mut out, false);
        out.extend_from_slice(n);
        out.extend_from_slice(data);
        local(0x0201_4b50, &mut central, true);
        for v in [0u16, 0, 0] { central.extend_from_slice(&v.to_le_bytes()); }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(n);
    }
    let cd_off = out.len() as u32;
    let cd_len = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, entries.len() as u16, entries.len() as u16] { out.extend_from_slice(&v.to_le_bytes()); }
    out.extend_from_slice(&cd_len.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn xml(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }

pub fn triangles(r: &RegenResult) -> Vec<[V3; 3]> {
    let t = &r.topo;
    let mut out = vec![];
    for p in &t.polys {
        for k in 1..p.len() - 1 { out.push([t.positions[p[0]], t.positions[p[k]], t.positions[p[k + 1]]]); }
    }
    out
}

pub fn write(r: &RegenResult, data: &ModelData, fmt: Format, path: &Path) -> Result<(), String> {
    let tris = triangles(r);
    if tris.is_empty() { return Err("Das Modell hat keine Volumengeometrie.".into()); }
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    let e = |x: std::io::Error| x.to_string();
    match fmt {
        Format::Step => {
            let text = step_text(r, &data.name, &path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
            f.write_all(text.as_bytes()).map_err(e)?;
        }
        Format::StlBinary => {
            let mut header = [0u8; 80];
            let name = format!("Freo {}", data.name);
            header[..name.len().min(80)].copy_from_slice(&name.as_bytes()[..name.len().min(80)]);
            f.write_all(&header).map_err(e)?;
            f.write_all(&(tris.len() as u32).to_le_bytes()).map_err(e)?;
            for t in &tris {
                let n = safe_norm((t[1] - t[0]).cross(t[2] - t[0]), V3::Z);
                for v in std::iter::once(n).chain(t.iter().copied()) {
                    for c in [v.x as f32, v.y as f32, v.z as f32] { f.write_all(&c.to_le_bytes()).map_err(e)?; }
                }
                f.write_all(&[0, 0]).map_err(e)?;
            }
        }
        Format::StlAscii => {
            writeln!(f, "solid {}", data.name).map_err(e)?;
            for t in &tris {
                let n = safe_norm((t[1] - t[0]).cross(t[2] - t[0]), V3::Z);
                writeln!(f, "  facet normal {:e} {:e} {:e}\n    outer loop", n.x, n.y, n.z).map_err(e)?;
                for v in t { writeln!(f, "      vertex {:e} {:e} {:e}", v.x, v.y, v.z).map_err(e)?; }
                writeln!(f, "    endloop\n  endfacet").map_err(e)?;
            }
            writeln!(f, "endsolid {}", data.name).map_err(e)?;
        }
        Format::Ply => {
            let (pos, tri) = indexed(r);
            write!(f, "ply\nformat binary_little_endian 1.0\ncomment Freo export of {}, units mm\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nelement face {}\nproperty list uchar int vertex_indices\nend_header\n", data.name, pos.len(), tri.len()).map_err(e)?;
            for p in &pos { for c in [p.x as f32, p.y as f32, p.z as f32] { f.write_all(&c.to_le_bytes()).map_err(e)?; } }
            for t in &tri {
                f.write_all(&[3u8]).map_err(e)?;
                for i in t { f.write_all(&(*i as i32).to_le_bytes()).map_err(e)?; }
            }
        }
        Format::Vrml => {
            let (pos, tri) = indexed(r);
            let c = data.appearance.color;
            writeln!(f, "#VRML V2.0 utf8\n# Freo export of {} (mm)\nShape {{\n  appearance Appearance {{ material Material {{ diffuseColor {} {} {} }} }}\n  geometry IndexedFaceSet {{\n    solid TRUE\n    creaseAngle 0.5\n    coord Coordinate {{ point [", data.name, c[0], c[1], c[2]).map_err(e)?;
            for p in &pos { writeln!(f, "      {} {} {},", p.x, p.y, p.z).map_err(e)?; }
            writeln!(f, "    ] }}\n    coordIndex [").map_err(e)?;
            for t in &tri { writeln!(f, "      {}, {}, {}, -1,", t[0], t[1], t[2]).map_err(e)?; }
            writeln!(f, "    ]\n  }}\n}}").map_err(e)?;
        }
        Format::Gltf => {
            // un-indexed so every triangle gets its own flat normal (crisp CAD edges)
            let mut buf: Vec<u8> = vec![];
            let (mut lo, mut hi) = (V3::splat(f64::MAX), V3::splat(f64::MIN));
            for t in &tris { for v in t { lo = lo.min(*v); hi = hi.max(*v); } }
            for t in &tris { for v in t { for c in [v.x as f32 / 1000.0, v.y as f32 / 1000.0, v.z as f32 / 1000.0] { buf.extend_from_slice(&c.to_le_bytes()); } } }
            let npos = buf.len();
            for t in &tris {
                let n = safe_norm((t[1] - t[0]).cross(t[2] - t[0]), V3::Z);
                for _ in 0..3 { for c in [n.x as f32, n.y as f32, n.z as f32] { buf.extend_from_slice(&c.to_le_bytes()); } }
            }
            let c = data.appearance.color;
            let count = tris.len() * 3;
            let json = format!(
                r#"{{"asset":{{"version":"2.0","generator":"Freo"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"name":"{name}"}}],"meshes":[{{"name":"{name}","primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1}},"material":0}}]}}],"materials":[{{"name":"{name}","pbrMetallicRoughness":{{"baseColorFactor":[{r},{g},{b},1.0],"metallicFactor":0.3,"roughnessFactor":0.5}}}}],"buffers":[{{"byteLength":{len},"uri":"data:application/octet-stream;base64,{data}"}}],"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":{npos},"target":34962}},{{"buffer":0,"byteOffset":{npos},"byteLength":{nn},"target":34962}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":{count},"type":"VEC3","min":[{x0},{y0},{z0}],"max":[{x1},{y1},{z1}]}},{{"bufferView":1,"componentType":5126,"count":{count},"type":"VEC3"}}]}}"#,
                name = data.name.replace('"', "'"), r = c[0], g = c[1], b = c[2], len = buf.len(), data = base64(&buf), npos = npos, nn = buf.len() - npos, count = count,
                x0 = lo.x as f32 / 1000.0, y0 = lo.y as f32 / 1000.0, z0 = lo.z as f32 / 1000.0, x1 = hi.x as f32 / 1000.0, y1 = hi.y as f32 / 1000.0, z1 = hi.z as f32 / 1000.0);
            f.write_all(json.as_bytes()).map_err(e)?;
        }
        Format::ThreeMf => {
            let (pos, tri) = indexed(r);
            let c = data.appearance.color;
            let hex = format!("#{:02X}{:02X}{:02X}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
            let mut model = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"de-DE\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n <metadata name=\"Title\">{}</metadata>\n <metadata name=\"Application\">Freo</metadata>\n <resources>\n  <basematerials id=\"1\"><base name=\"{}\" displaycolor=\"{}\"/></basematerials>\n  <object id=\"2\" type=\"model\" name=\"{}\" pid=\"1\" pindex=\"0\">\n   <mesh>\n    <vertices>\n", xml(&data.name), xml(&data.name), hex, xml(&data.name));
            for p in &pos { model.push_str(&format!("     <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>\n", p.x as f32, p.y as f32, p.z as f32)); }
            model.push_str("    </vertices>\n    <triangles>\n");
            for t in &tri { model.push_str(&format!("     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>\n", t[0], t[1], t[2])); }
            model.push_str("    </triangles>\n   </mesh>\n  </object>\n </resources>\n <build>\n  <item objectid=\"2\"/>\n </build>\n</model>\n");
            let types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>";
            let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>";
            let zip = zip_stored(&[("[Content_Types].xml", types.as_bytes().to_vec()), ("_rels/.rels", rels.as_bytes().to_vec()), ("3D/3dmodel.model", model.into_bytes())]);
            f.write_all(&zip).map_err(e)?;
        }
        Format::Obj => {
            let t = &r.topo;
            writeln!(f, "# Freo export of {}\no {}", data.name, data.name).map_err(e)?;
            for p in &t.positions { writeln!(f, "v {} {} {}", p.x, p.y, p.z).map_err(e)?; }
            for p in &t.polys {
                let idx: Vec<String> = p.iter().map(|i| (i + 1).to_string()).collect();
                writeln!(f, "f {}", idx.join(" ")).map_err(e)?;
            }
        }
    }
    Ok(())
}

pub struct MassProps {
    pub volume: f64,
    pub area: f64,
    pub mass: f64,
    pub cog: V3,
    /// inertia tensor about the centre of gravity (kg·mm²): xx, yy, zz, xy, yz, zx
    pub inertia: [f64; 6],
    pub bbox: BBox,
}

/// Exact polyhedral mass properties via the divergence theorem (tetrahedra from the origin).
pub fn mass_properties(r: &RegenResult, density: f64) -> MassProps {
    let tris = triangles(r);
    let (mut vol, mut area) = (0.0, 0.0);
    let mut c = V3::ZERO;
    let mut ii = [0.0f64; 6]; // integrals of x², y², z², xy, yz, zx
    for t in &tris {
        let (a, b, cc) = (t[0], t[1], t[2]);
        area += (b - a).cross(cc - a).length() * 0.5;
        let v = a.dot(b.cross(cc)) / 6.0;
        vol += v;
        c += (a + b + cc) * (v / 4.0);
        let f = |i: usize, j: usize| -> f64 {
            let (pa, pb, pc) = (a.to_array(), b.to_array(), cc.to_array());
            v / 20.0 * (2.0 * (pa[i] * pa[j] + pb[i] * pb[j] + pc[i] * pc[j]) + pa[i] * pb[j] + pa[j] * pb[i] + pa[i] * pc[j] + pa[j] * pc[i] + pb[i] * pc[j] + pb[j] * pc[i])
        };
        ii[0] += f(0, 0); ii[1] += f(1, 1); ii[2] += f(2, 2);
        ii[3] += f(0, 1); ii[4] += f(1, 2); ii[5] += f(2, 0);
    }
    let cog = if vol.abs() > 1e-12 { c / vol } else { V3::ZERO };
    let m = vol * density;
    let (xx, yy, zz) = (ii[0] - vol * cog.x * cog.x, ii[1] - vol * cog.y * cog.y, ii[2] - vol * cog.z * cog.z);
    let inertia = [
        density * (yy + zz), density * (xx + zz), density * (xx + yy),
        -density * (ii[3] - vol * cog.x * cog.y), -density * (ii[4] - vol * cog.y * cog.z), -density * (ii[5] - vol * cog.z * cog.x),
    ];
    MassProps { volume: vol, area, mass: m, cog, inertia, bbox: r.topo.bbox }
}

fn step_real(v: f64) -> String {
    let v = if v.abs() < 1e-12 { 0.0 } else { v };
    let mut s = format!("{:.6}", v);
    while s.ends_with('0') { s.pop(); }
    if !s.contains('.') { s.push('.'); }
    s
}

fn step_str(s: &str) -> String { s.replace('\\', "\\\\").replace('\'', "''") }

/// ISO 10303-21 (AP214) text of the solid as a faceted B-rep in millimetres.
pub fn step_text(r: &RegenResult, name: &str, file_name: &str) -> String {
    let t = &r.topo;
    let name = step_str(name);
    let mut out = String::new();
    let stamp = {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let days = secs / 86400;
        // civil date from days since 1970-01-01
        let z = days as i64 + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, m, d, (secs / 3600) % 24, (secs / 60) % 60, secs % 60)
    };
    out.push_str("ISO-10303-21;\nHEADER;\n");
    out.push_str("FILE_DESCRIPTION(('Freo faceted solid model'),'2;1');\n");
    out.push_str(&format!("FILE_NAME('{}','{}',('Freo'),(''),'Freo STEP writer','Freo','');\n", step_str(file_name), stamp));
    out.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n");
    let mut id = 0usize;
    let mut ent = |out: &mut String, body: String| -> usize { id += 1; out.push_str(&format!("#{}={};\n", id, body)); id };
    let app = ent(&mut out, "APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into());
    ent(&mut out, format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{})", app));
    let pctx = ent(&mut out, format!("PRODUCT_CONTEXT('',#{},'mechanical')", app));
    let prod = ent(&mut out, format!("PRODUCT('{0}','{0}','',(#{1}))", name, pctx));
    ent(&mut out, format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{}))", prod));
    let pdf = ent(&mut out, format!("PRODUCT_DEFINITION_FORMATION('','',#{})", prod));
    let pdc = ent(&mut out, format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{},'design')", app));
    let pd = ent(&mut out, format!("PRODUCT_DEFINITION('design','',#{},#{})", pdf, pdc));
    let pds = ent(&mut out, format!("PRODUCT_DEFINITION_SHAPE('','',#{})", pd));
    let len_u = ent(&mut out, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let ang_u = ent(&mut out, "(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let sol_u = ent(&mut out, "(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let unc = ent(&mut out, format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-05),#{},'distance_accuracy_value','confusion accuracy')", len_u));
    let ctx = ent(&mut out, format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{},#{},#{}))REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))", unc, len_u, ang_u, sol_u));
    let origin = ent(&mut out, "CARTESIAN_POINT('',(0.,0.,0.))".into());
    let dz = ent(&mut out, "DIRECTION('',(0.,0.,1.))".into());
    let dx = ent(&mut out, "DIRECTION('',(1.,0.,0.))".into());
    let axis = ent(&mut out, format!("AXIS2_PLACEMENT_3D('',#{},#{},#{})", origin, dz, dx));
    let mut point_ids = vec![0usize; t.positions.len()];
    let mut used = vec![false; t.positions.len()];
    for p in &t.polys { for &v in p { used[v] = true; } }
    for (i, p) in t.positions.iter().enumerate() {
        if used[i] { point_ids[i] = ent(&mut out, format!("CARTESIAN_POINT('',({},{},{}))", step_real(p.x), step_real(p.y), step_real(p.z))); }
    }
    // one closed shell per body (assembly components are separate bodies)
    let mut bodies: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
    for (pi, p) in t.polys.iter().enumerate() {
        let mut idx: Vec<usize> = vec![];
        for &v in p { if idx.last() != Some(&v) { idx.push(v); } }
        if idx.len() > 1 && idx.first() == idx.last() { idx.pop(); }
        if idx.len() < 3 { continue; }
        let pts: Vec<String> = idx.iter().map(|v| format!("#{}", point_ids[*v])).collect();
        let lp = ent(&mut out, format!("POLY_LOOP('',({}))", pts.join(",")));
        let bound = ent(&mut out, format!("FACE_OUTER_BOUND('',#{},.T.)", lp));
        let face = ent(&mut out, format!("FACE('',(#{}))", bound));
        bodies.entry(t.tags[pi] >> crate::kernel::topology::BODY_SHIFT).or_default().push(face);
    }
    let mut items = vec![format!("#{}", axis)];
    for (k, (_, faces)) in bodies.iter().enumerate() {
        let list: Vec<String> = faces.iter().map(|f| format!("#{}", f)).collect();
        let shell = ent(&mut out, format!("CLOSED_SHELL('',({}))", list.join(",")));
        let label = if bodies.len() > 1 { format!("{}_{}", name, k + 1) } else { name.clone() };
        items.push(format!("#{}", ent(&mut out, format!("FACETED_BREP('{}',#{})", label, shell))));
    }
    let rep = ent(&mut out, format!("FACETED_BREP_SHAPE_REPRESENTATION('{}',({}),#{})", name, items.join(","), ctx));
    ent(&mut out, format!("SHAPE_DEFINITION_REPRESENTATION(#{},#{})", pds, rep));
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}
