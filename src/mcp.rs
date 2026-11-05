//! MCP (Model Context Protocol) server: lets AI assistants (Claude, ChatGPT, …) drive Freo
//! headless over stdio. JSON-RPC 2.0, one message per line, as in the MCP stdio transport.
//!
//! The server holds one part model in memory. Every change goes through the same model data and
//! regeneration engine the app uses (`model::regen`), so files saved here open unchanged in Freo.
//! Run it with `freo-mcp` or `freo --mcp`.

use crate::kernel::math::*;
use crate::model::features::*;
use crate::model::regen::{self, RegenResult};
use crate::sketch::sketch::{SkCType, SkConstraint, SkKind, Sketch};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// Protocol revisions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Freo is a Creo-style parametric part modeller. Units are millimetres. \
The server holds one part in memory: start with new_part or open_model, add features, then save_model / export_model. \
World axes: Y is up. Default datum planes: VORNE/FRONT (XY plane, normal +Z), OBEN/TOP (XZ plane, normal +Y), RECHTS/RIGHT (YZ plane, normal +X). \
Sketch coordinates (x, y) are local to the sketch plane: on TOP x→world X and y→world −Z; on FRONT x→X, y→Y; on RIGHT x→−Z, y→Y. \
A plane argument is a datum name or id (\"TOP\", \"OBEN\", \"DTM1\", 12), a planar face by direction (\"+Y\" = the outermost face whose normal is +Y), \
or an object {\"face\": index} with an index from list_geometry. Faces use the same local frame as a datum plane with the same normal. \
Edges for rounds and chamfers are indices from list_geometry or a filter such as {\"direction\": \"Y\"} or {\"face_dir\": \"+Y\"}. \
Dimensions take numbers or parameter expressions (\"W/2\"). Every change regenerates the model; a change that makes a feature fail is rolled back and reported as an error.";

// ---------------------------------------------------------------- session

/// The model the server works on and its regenerated geometry.
pub struct Session {
    pub data: ModelData,
    pub result: RegenResult,
    pub path: Option<PathBuf>,
    undo: Vec<ModelData>,
    dirty: bool,
}

impl Default for Session {
    fn default() -> Self { Session::new() }
}

impl Session {
    pub fn new() -> Session {
        let data = ModelData::new_part_template("PRT0001");
        let result = regen::run(&data);
        Session { data, result, path: None, undo: vec![], dirty: false }
    }

    fn replace(&mut self, data: ModelData, path: Option<PathBuf>) {
        self.data = data;
        self.result = regen::run(&self.data);
        self.path = path;
        self.undo.clear();
        self.dirty = false;
    }

    /// Applies a change and regenerates; rolls it back when it fails or makes a feature fail.
    fn commit<T>(&mut self, f: impl FnOnce(&mut ModelData) -> Result<T, String>) -> Result<T, String> {
        let before = self.data.clone();
        let old_errors: HashSet<u64> = self.result.errors.keys().copied().collect();
        let old_param_errors = self.result.param_errors.len();
        let out = match f(&mut self.data) {
            Ok(v) => v,
            Err(e) => { self.data = before; return Err(e); }
        };
        self.result = regen::run(&self.data);
        let mut new: Vec<String> = self.result.errors.iter().filter(|(id, _)| !old_errors.contains(id))
            .map(|(id, e)| format!("{}: {}", self.data.feature(*id).map(|f| f.name.clone()).unwrap_or_else(|| id.to_string()), e)).collect();
        if self.result.param_errors.len() > old_param_errors { new.extend(self.result.param_errors.iter().cloned()); }
        if !new.is_empty() {
            new.sort();
            self.data = before;
            self.result = regen::run(&self.data);
            return Err(format!("Regeneration failed, change rolled back: {}", new.join("; ")));
        }
        self.undo.push(before);
        if self.undo.len() > 100 { self.undo.remove(0); }
        self.dirty = true;
        Ok(out)
    }

    /// Adds a feature at the insert position (end of the tree unless an insert point is set).
    fn add_feature(&mut self, kind: FeatureKind, name: Option<&str>) -> Result<Value, String> {
        let id = self.commit(|d| {
            let id = d.next_id;
            let name = name.map(|s| s.to_string()).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| d.next_name(&kind));
            if d.features.iter().any(|f| f.name.eq_ignore_ascii_case(&name)) { return Err(format!("A feature named {} already exists.", name)); }
            let f = Feature { id, name, suppressed: false, hidden: false, kind };
            match d.insert_index { Some(i) => { d.features.insert(i, f); d.insert_index = Some(i + 1); } None => d.features.push(f) }
            d.next_id += 1;
            Ok(id)
        })?;
        let f = self.data.feature(id).unwrap();
        Ok(json!({ "id": id, "name": f.name, "type": f.kind.type_name(), "model": self.brief() }))
    }

    /// Volume, bounding box and errors after a change.
    fn brief(&self) -> Value {
        let m = crate::app::export::mass_properties(&self.result, self.data.density);
        json!({
            "volume_mm3": r(m.volume),
            "bbox": bbox(&self.result.topo.bbox),
            "errors": self.error_list(),
        })
    }

    fn error_list(&self) -> Vec<Value> {
        let mut v: Vec<Value> = self.data.features.iter().filter_map(|f| self.result.errors.get(&f.id).map(|e| json!({ "feature": f.id, "name": f.name, "error": e }))).collect();
        v.extend(self.result.param_errors.iter().map(|e| json!({ "parameters": e })));
        v
    }

    fn feature_id(&self, v: &Value) -> Result<u64, String> {
        let id = match v {
            Value::Number(n) => n.as_u64().ok_or("feature id must be a positive integer")?,
            Value::String(s) => {
                let s = s.trim();
                if let Ok(n) = s.parse::<u64>() { n } else {
                    let alias = match s.to_uppercase().as_str() {
                        "FRONT" | "VORNE" => Some(FRONT_ID), "TOP" | "OBEN" => Some(TOP_ID), "RIGHT" | "RECHTS" => Some(RIGHT_ID), "CSYS" | "BASIS" | "PRT_CSYS_DEF" => Some(CSYS_ID), _ => None,
                    };
                    match alias.or_else(|| self.data.features.iter().find(|f| f.name.eq_ignore_ascii_case(s)).map(|f| f.id)) {
                        Some(id) => id,
                        None => return Err(format!("No feature named {}. Use list_features to see the model tree.", s)),
                    }
                }
            }
            _ => return Err("feature must be an id or a name".into()),
        };
        if self.data.feature(id).is_none() { return Err(format!("No feature with id {}.", id)); }
        Ok(id)
    }

    fn feature_ids(&self, v: Option<&Value>) -> Result<Vec<u64>, String> {
        match v {
            None | Some(Value::Null) => Ok(vec![]),
            Some(Value::Array(a)) => a.iter().map(|x| self.feature_id(x)).collect(),
            Some(x) => Ok(vec![self.feature_id(x)?]),
        }
    }

    /// The outermost planar face whose normal points along `dir`.
    fn extreme_face(&self, dir: V3) -> Result<usize, String> {
        let t = &self.result.topo;
        (0..t.faces.len()).filter(|&f| t.faces[f].planar && t.faces[f].normal.dot(dir) > 0.999)
            .max_by(|&a, &b| t.faces[a].center.dot(dir).partial_cmp(&t.faces[b].center.dot(dir)).unwrap())
            .ok_or_else(|| format!("The model has no planar face facing {:?}.", [dir.x, dir.y, dir.z]))
    }

    fn face_index(&self, v: &Value) -> Result<usize, String> {
        let t = &self.result.topo;
        match v {
            Value::Number(n) => {
                let i = n.as_u64().ok_or("face index must be a non-negative integer")? as usize;
                if i >= t.faces.len() { return Err(format!("Face {} does not exist (the model has {} faces).", i, t.faces.len())); }
                Ok(i)
            }
            Value::String(s) => self.extreme_face(parse_dir(s)?),
            Value::Object(o) => {
                if let Some(f) = o.get("face") { return self.face_index(f); }
                if let Some(d) = o.get("face_dir") { return self.extreme_face(dir_value(d)?); }
                Err("face must be an index, a direction such as \"+Y\", or {\"face\": index}".into())
            }
            _ => Err("face must be an index or a direction such as \"+Y\"".into()),
        }
    }

    fn face_ref(&self, v: &Value) -> Result<FaceRef, String> {
        if let Value::Object(o) = v {
            if let (Some(p), Some(n)) = (o.get("point"), o.get("normal")) {
                return Ok(FaceRef { point: v3_value(p)?, normal: safe_norm(v3_value(n)?, V3::Y) });
            }
        }
        Ok(self.result.topo.face_ref(self.face_index(v)?))
    }

    /// A sketch / placement plane: datum plane (name or id) or planar face.
    fn plane_ref(&self, v: &Value) -> Result<PlaneRef, String> {
        let is_face = match v {
            Value::Object(_) => true,
            Value::String(s) => s.trim().starts_with('+') || s.trim().starts_with('-'),
            _ => false,
        };
        if is_face {
            let fr = self.face_ref(v)?;
            if let Some(i) = self.result.topo.match_face(fr.point, fr.normal) {
                if !self.result.topo.faces[i].planar { return Err("The face is not planar.".into()); }
            }
            return Ok(PlaneRef::Face(fr));
        }
        let id = self.feature_id(v)?;
        match self.data.feature(id).map(|f| &f.kind) {
            Some(FeatureKind::DatumPlane(_)) => Ok(PlaneRef::Datum(id)),
            _ => Err(format!("{} is not a datum plane.", self.data.feature(id).unwrap().name)),
        }
    }

    fn axis_ref(&self, v: &Value) -> Result<AxisRef, String> {
        if let Value::String(s) = v {
            match s.trim().trim_start_matches('+').to_uppercase().as_str() {
                "X" => return Ok(AxisRef::WorldX), "Y" => return Ok(AxisRef::WorldY), "Z" => return Ok(AxisRef::WorldZ), _ => {}
            }
        }
        let id = self.feature_id(v)?;
        match self.data.feature(id).map(|f| &f.kind) {
            Some(FeatureKind::DatumAxis(_)) => Ok(AxisRef::Axis(id)),
            // a hole's axis
            _ if self.result.axes.iter().any(|a| a.feature == id) => Ok(AxisRef::Axis(self.result.axes.iter().find(|a| a.feature == id).unwrap().id)),
            _ => Err("axis must be \"X\", \"Y\", \"Z\", a datum axis or a hole".into()),
        }
    }

    /// Edge chains picked by index list, "all", or a filter object.
    fn edge_refs(&self, v: &Value) -> Result<Vec<EdgeRef>, String> {
        let t = &self.result.topo;
        let idx: Vec<usize> = match v {
            Value::Array(a) => {
                let mut out = vec![];
                for x in a {
                    let i = x.as_u64().ok_or("edge indices must be integers from list_geometry")? as usize;
                    if i >= t.chains.len() { return Err(format!("Edge {} does not exist (the model has {} edges).", i, t.chains.len())); }
                    out.push(i);
                }
                out
            }
            Value::String(s) if s.eq_ignore_ascii_case("all") => (0..t.chains.len()).collect(),
            Value::Object(o) => {
                let dir = o.get("direction").map(dir_value).transpose()?;
                let face = o.get("face_dir").map(|d| dir_value(d).and_then(|d| self.extreme_face(d))).transpose()?
                    .or(o.get("face").map(|f| self.face_index(f)).transpose()?);
                let convex = o.get("convex").and_then(Value::as_bool);
                let closed = o.get("closed").and_then(Value::as_bool);
                let min_len = o.get("min_length").and_then(Value::as_f64);
                (0..t.chains.len()).filter(|&i| {
                    let c = &t.chains[i];
                    dir.is_none_or(|d| c.is_straight() && (safe_norm(*c.points.last().unwrap() - c.points[0], V3::X)).dot(d).abs() > 0.999)
                        && face.is_none_or(|f| c.face_a == f || c.face_b == f)
                        && convex.is_none_or(|x| c.convex == x)
                        && closed.is_none_or(|x| c.closed == x)
                        && min_len.is_none_or(|l| c.length() >= l)
                }).collect()
            }
            _ => return Err("edges must be a list of edge indices, \"all\", or a filter like {\"direction\": \"Y\"}".into()),
        };
        if idx.is_empty() { return Err("No edges match.".into()); }
        Ok(idx.into_iter().map(|i| t.edge_ref(i)).collect())
    }

    fn plane_desc(&self, p: &Option<PlaneRef>) -> Value {
        match p {
            None => Value::Null,
            Some(PlaneRef::Datum(id)) => json!(self.data.feature(*id).map(|f| f.name.clone()).unwrap_or_else(|| id.to_string())),
            Some(PlaneRef::Face(f)) => json!({ "face_point": v3(f.point), "normal": v3(f.normal) }),
        }
    }

    fn summary(&self, k: &FeatureKind) -> Value {
        let d = |x: &Dim| json!(x.0);
        match k {
            FeatureKind::DatumPlane(p) => match p.kind {
                PlaneKind::Offset => json!({ "reference": self.plane_desc(&p.reference), "offset": d(&p.offset) }),
                PlaneKind::Angle => json!({ "reference": self.plane_desc(&p.reference), "angle": d(&p.angle) }),
                k => json!({ "builtin": format!("{:?}", k) }),
            },
            FeatureKind::Sketch(s) => json!({ "plane": self.plane_desc(&s.plane), "entities": s.sketch.entities.len() }),
            FeatureKind::Extrude(e) => json!({ "plane": self.plane_desc(&e.plane), "depth_type": format!("{:?}", e.depth_type), "depth": d(&e.depth), "remove": e.remove, "flip": e.flip }),
            FeatureKind::Revolve(e) => json!({ "plane": self.plane_desc(&e.plane), "angle": d(&e.angle), "remove": e.remove }),
            FeatureKind::Hole(h) => json!({ "plane": self.plane_desc(&h.plane), "type": format!("{:?}", h.ty), "x": d(&h.x), "y": d(&h.y), "diameter": d(&h.diameter), "through_all": h.through_all, "depth": d(&h.depth) }),
            FeatureKind::Round(rd) => json!({ "sets": rd.sets.iter().map(|s| json!({ "radius": d(&s.radius), "edges": s.edges.len() })).collect::<Vec<_>>() }),
            FeatureKind::Chamfer(c) => json!({ "kind": format!("{:?}", c.kind), "d1": d(&c.d1), "edges": c.edges.len() }),
            FeatureKind::Shell(s) => json!({ "thickness": d(&s.thickness), "faces": s.faces.len() }),
            FeatureKind::Pattern(p) => json!({ "kind": format!("{:?}", p.kind), "features": p.features, "count1": d(&p.count1), "spacing1": d(&p.spacing1) }),
            FeatureKind::Mirror(m) => json!({ "plane": self.plane_desc(&m.plane), "features": m.features }),
            _ => json!({}),
        }
    }
}

// ---------------------------------------------------------------- JSON helpers

fn r(x: f64) -> f64 { let v = (x * 1e4).round() / 1e4; if v == 0.0 { 0.0 } else { v } }
fn v3(p: V3) -> Value { json!([r(p.x), r(p.y), r(p.z)]) }
fn bbox(b: &BBox) -> Value { if b.is_empty() { Value::Null } else { json!({ "min": v3(b.min), "max": v3(b.max), "size": v3(b.size()) }) } }

fn parse_dir(s: &str) -> Result<V3, String> {
    let t = s.trim().to_uppercase();
    let (neg, axis) = match t.strip_prefix('-') { Some(a) => (true, a), None => (false, t.trim_start_matches('+')) };
    let d = match axis { "X" => V3::X, "Y" => V3::Y, "Z" => V3::Z, _ => return Err(format!("Direction {} not understood; use +X, -X, +Y, -Y, +Z or -Z.", s)) };
    Ok(if neg { -d } else { d })
}

fn dir_value(v: &Value) -> Result<V3, String> {
    match v { Value::String(s) => parse_dir(s), _ => Ok(safe_norm(v3_value(v)?, V3::Y)) }
}

fn nums(v: &Value, n: usize, what: &str) -> Result<Vec<f64>, String> {
    let a = v.as_array().filter(|a| a.len() == n).ok_or(format!("{} must be an array of {} numbers", what, n))?;
    a.iter().map(|x| x.as_f64().ok_or(format!("{} must contain numbers", what))).collect()
}
fn v3_value(v: &Value) -> Result<V3, String> { let a = nums(v, 3, "point")?; Ok(V3::new(a[0], a[1], a[2])) }
fn v2_value(v: &Value) -> Result<V2, String> { let a = nums(v, 2, "2D point")?; Ok(V2::new(a[0], a[1])) }

fn dim_value(v: &Value, what: &str) -> Result<Dim, String> {
    match v {
        Value::Number(n) => Ok(Dim::v(n.as_f64().unwrap_or(0.0))),
        Value::String(s) if !s.trim().is_empty() => Ok(Dim(s.trim().to_string())),
        _ => Err(format!("{} must be a number or a parameter expression", what)),
    }
}

struct Args<'a>(&'a Map<String, Value>);

impl<'a> Args<'a> {
    fn get(&self, k: &str) -> Option<&'a Value> { self.0.get(k).filter(|v| !v.is_null()) }
    fn req(&self, k: &str) -> Result<&'a Value, String> { self.get(k).ok_or(format!("Missing argument: {}", k)) }
    fn str(&self, k: &str) -> Option<&'a str> { self.get(k).and_then(Value::as_str) }
    fn req_str(&self, k: &str) -> Result<&'a str, String> { self.str(k).ok_or(format!("Missing string argument: {}", k)) }
    fn bool(&self, k: &str, default: bool) -> bool { self.get(k).and_then(Value::as_bool).unwrap_or(default) }
    fn f64(&self, k: &str) -> Result<Option<f64>, String> { self.get(k).map(|v| v.as_f64().ok_or(format!("{} must be a number", k))).transpose() }
    fn dim(&self, k: &str) -> Result<Option<Dim>, String> { self.get(k).map(|v| dim_value(v, k)).transpose() }
    fn req_dim(&self, k: &str) -> Result<Dim, String> { dim_value(self.req(k)?, k) }
}

/// Paths relative to FREO_WORK_DIR (or the server's working directory).
fn resolve_path(p: &str) -> PathBuf {
    let p = if let Some(rest) = p.strip_prefix("~/") { std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest) } else { PathBuf::from(p) };
    if p.is_absolute() { return p; }
    let base = std::env::var_os("FREO_WORK_DIR").map(PathBuf::from).or_else(|| std::env::current_dir().ok()).unwrap_or_default();
    base.join(p)
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() { out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char); } else { out.push('='); }
        }
    }
    out
}

fn export_format(name: Option<&str>, path: &std::path::Path) -> Result<crate::app::export::Format, String> {
    use crate::app::export::Format;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let key = name.map(|s| s.to_lowercase()).unwrap_or(ext);
    Ok(match key.as_str() {
        "stl" | "stl_binary" => Format::StlBinary,
        "stl_ascii" => Format::StlAscii,
        "obj" => Format::Obj,
        "step" | "stp" => Format::Step,
        "3mf" => Format::ThreeMf,
        "ply" => Format::Ply,
        "vrml" | "wrl" => Format::Vrml,
        "gltf" => Format::Gltf,
        k => return Err(format!("Unknown export format {:?}; use stl, stl_ascii, obj, step, 3mf, ply, vrml or gltf.", k)),
    })
}

// ---------------------------------------------------------------- sketches

/// Builds a sketch from simple shapes. Line and arc end points that coincide are shared, so
/// polylines, rectangles and arcs form closed loops.
fn build_sketch(shapes: &Value) -> Result<Sketch, String> {
    let list = shapes.as_array().ok_or("shapes must be an array of shape objects")?;
    if list.is_empty() { return Err("shapes is empty".into()); }
    let mut s = Sketch::default();
    let mut ends: Vec<(V2, i64)> = vec![];
    let mut pt = |s: &mut Sketch, p: V2| -> i64 {
        if let Some((_, id)) = ends.iter().find(|(q, _)| (*q - p).length() < 1e-7) { return *id; }
        let id = s.add_point(p);
        ends.push((p, id));
        id
    };
    for (i, sh) in list.iter().enumerate() {
        let o = sh.as_object().ok_or(format!("shape {} must be an object", i))?;
        let a = Args(o);
        let ty = a.req_str("type").map_err(|_| format!("shape {} needs a \"type\"", i))?;
        let center = |a: &Args| -> Result<V2, String> {
            if let Some(c) = a.get("center") { return v2_value(c); }
            Ok(V2::new(a.f64("cx")?.unwrap_or(0.0), a.f64("cy")?.unwrap_or(0.0)))
        };
        match ty {
            "rect" | "rectangle" => {
                let (w, h) = (a.f64("width")?.ok_or("rect needs width")?, a.f64("height")?.ok_or("rect needs height")?);
                if w <= 0.0 || h <= 0.0 { return Err("rect width and height must be positive".into()); }
                let c = center(&a)?;
                let p: Vec<i64> = [V2::new(-w, -h), V2::new(w, -h), V2::new(w, h), V2::new(-w, h)].iter().map(|q| pt(&mut s, c + *q * 0.5)).collect();
                for k in 0..4 {
                    let l = s.add_line(p[k], p[(k + 1) % 4]);
                    s.add_constraint(SkConstraint::new(if k % 2 == 0 { SkCType::Horizontal } else { SkCType::Vertical }, vec![], vec![l]));
                }
            }
            "circle" => {
                let rad = match (a.f64("radius")?, a.f64("diameter")?) { (Some(r), _) => r, (_, Some(d)) => d / 2.0, _ => return Err("circle needs radius or diameter".into()) };
                if rad <= 0.0 { return Err("circle radius must be positive".into()); }
                let c = s.add_point(center(&a)?);
                s.add_circle(c, rad);
            }
            "polygon" | "polyline" => {
                let pts = a.req("points")?.as_array().ok_or("points must be an array of [x, y]")?;
                let pts: Vec<V2> = pts.iter().map(v2_value).collect::<Result<_, _>>()?;
                let closed = a.bool("closed", ty == "polygon");
                if pts.len() < 2 || (closed && pts.len() < 3) { return Err("polygon needs at least 3 points (polyline 2)".into()); }
                let ids: Vec<i64> = pts.iter().map(|p| pt(&mut s, *p)).collect();
                let n = if closed { ids.len() } else { ids.len() - 1 };
                for k in 0..n { s.add_line(ids[k], ids[(k + 1) % ids.len()]); }
            }
            "line" | "centerline" => {
                let (p0, p1) = (v2_value(a.req("from")?)?, v2_value(a.req("to")?)?);
                if (p1 - p0).length() < 1e-9 { return Err("line end points coincide".into()); }
                if ty == "centerline" {
                    let (i0, i1) = (s.add_point(p0), s.add_point(p1));
                    s.add_entity(SkKind::Line, vec![i0, i1], 0.0, true, true);
                } else {
                    let (i0, i1) = (pt(&mut s, p0), pt(&mut s, p1));
                    s.add_line(i0, i1);
                }
            }
            "arc" => {
                let c = v2_value(a.req("center")?)?;
                let (p0, p1) = (v2_value(a.req("start")?)?, v2_value(a.req("end")?)?);
                let rad = (p0 - c).length();
                if rad < 1e-9 { return Err("arc start coincides with its center".into()); }
                let p1 = c + safe_norm2(p1 - c, V2::X) * rad;
                let ci = s.add_point(c);
                let (i0, i1) = (pt(&mut s, p0), pt(&mut s, p1));
                s.add_arc(ci, i0, i1);
            }
            t => return Err(format!("Unknown shape type {:?}; use rect, circle, polygon, polyline, line, arc or centerline.", t)),
        }
    }
    Ok(s)
}

fn sketch_json(s: &Sketch) -> Value {
    let ents: Vec<Value> = s.entities.iter().map(|e| {
        let p: Vec<Value> = e.pts.iter().map(|&id| { let q = s.point(id); json!([r(q.x), r(q.y)]) }).collect();
        json!({ "kind": format!("{:?}", e.kind), "points": p, "r": r(e.r), "construction": e.construction, "centerline": e.centerline })
    }).collect();
    json!(ents)
}

// ---------------------------------------------------------------- tools

fn plane_schema(desc: &str) -> Value {
    json!({ "description": desc, "anyOf": [
        { "type": "string", "description": "Datum plane name (TOP/OBEN, FRONT/VORNE, RIGHT/RECHTS, DTM1…) or a face direction such as \"+Y\"" },
        { "type": "integer", "description": "Datum plane feature id" },
        { "type": "object", "description": "{\"face\": index} or {\"face_dir\": \"+Y\"} or {\"point\": [x,y,z], \"normal\": [x,y,z]}" }
    ]})
}

fn edges_schema() -> Value {
    json!({ "description": "Edge indices from list_geometry, \"all\", or a filter object. Filter keys (all optional, combined with AND): direction (\"X\"/\"Y\"/\"Z\" – straight edges parallel to it), face_dir (\"+Y\" – edges of the outermost face facing that way), face (face index), convex (bool), closed (bool, e.g. circular hole edges), min_length (mm).",
        "anyOf": [ { "type": "array", "items": { "type": "integer" } }, { "type": "string", "enum": ["all"] }, { "type": "object" } ] })
}

fn shapes_schema() -> Value {
    json!({ "type": "array", "description": "Sketch shapes in the plane's local 2D coordinates (mm). Closed loops become the profile; loops inside loops become holes. Coinciding line/arc end points are joined.",
        "items": { "type": "object", "properties": {
            "type": { "type": "string", "enum": ["rect", "circle", "polygon", "polyline", "line", "arc", "centerline"] },
            "width": { "type": "number", "description": "rect" }, "height": { "type": "number", "description": "rect" },
            "cx": { "type": "number", "description": "rect/circle centre x (default 0)" }, "cy": { "type": "number", "description": "rect/circle centre y (default 0)" },
            "radius": { "type": "number", "description": "circle" }, "diameter": { "type": "number", "description": "circle" },
            "points": { "type": "array", "items": { "type": "array", "items": { "type": "number" } }, "description": "polygon/polyline: [[x,y], …]" },
            "closed": { "type": "boolean", "description": "polyline: close the chain (polygon: always)" },
            "from": { "type": "array", "items": { "type": "number" } }, "to": { "type": "array", "items": { "type": "number" } },
            "center": { "type": "array", "items": { "type": "number" }, "description": "arc centre (circle/rect may also use it)" },
            "start": { "type": "array", "items": { "type": "number" } }, "end": { "type": "array", "items": { "type": "number" }, "description": "arc runs counter-clockwise from start to end" }
        }, "required": ["type"] } })
}

fn dim_schema(desc: &str) -> Value { json!({ "description": desc, "type": ["number", "string"] }) }

/// MCP tool annotations, so clients (OpenClaw, Claude …) can skip approval prompts for safe calls.
/// Read-only tools only look at the part. Destructive ones can discard unsaved work or
/// overwrite files; all other edits stay in memory and can be undone.
fn annotations(name: &str) -> Value {
    let read_only = matches!(name, "get_model_info" | "list_features" | "get_feature" | "list_geometry" | "mass_properties" | "regenerate");
    let destructive = matches!(name, "new_part" | "open_model" | "save_model" | "export_model" | "delete_features");
    let title = name.replace('_', " ");
    let title = title[..1].to_uppercase() + &title[1..];
    json!({ "title": title, "readOnlyHint": read_only, "destructiveHint": destructive, "idempotentHint": read_only || matches!(name, "save_model" | "export_model" | "render_png"), "openWorldHint": false })
}

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({ "name": name, "description": desc, "inputSchema": { "type": "object", "properties": props, "required": required }, "annotations": annotations(name) })
}

pub fn tool_list() -> Vec<Value> {
    let feature = json!({ "description": "Feature id or name", "type": ["integer", "string"] });
    let features = json!({ "description": "Feature ids or names", "type": "array", "items": { "type": ["integer", "string"] } });
    let name = json!({ "type": "string", "description": "Feature name (default: next free name such as Profil 1)" });
    vec![
        tool("new_part", "Start a new, empty part from the start-part template (datum planes VORNE/OBEN/RECHTS, BASIS csys, axes A_X/A_Y/A_Z). Unsaved changes to the current part are discarded.",
            json!({ "name": { "type": "string", "description": "Part name, e.g. PLATE (default PRT0001)" } }), &[]),
        tool("open_model", "Open a Freo part (.pfprt or .json), or import a STEP/STL/OBJ file as a new part with an import feature.",
            json!({ "path": { "type": "string", "description": "File path; relative paths resolve against FREO_WORK_DIR or the server's working directory" } }), &["path"]),
        tool("save_model", "Save the part as a Freo part file (JSON, open it in Freo with File ▸ Open). Without a path, saves to the file it was opened from or last saved to.",
            json!({ "path": { "type": "string", "description": "Target path, e.g. ~/Documents/plate.pfprt" } }), &[]),
        tool("get_model_info", "Name, file, units, density, parameters, feature count, volume, bounding box and regeneration errors of the current part.", json!({}), &[]),
        tool("list_features", "The model tree: every feature with id, name, type, state (suppressed, failed) and its main dimensions.", json!({}), &[]),
        tool("get_feature", "The complete definition of one feature as JSON (field names usable with edit_feature).", json!({ "feature": feature }), &["feature"]),
        tool("list_geometry", "Selectable geometry of the regenerated part: planar/curved faces (index, centre, normal, area, local x/y axes), edges (index, end points, length, straight, convex) or datums (planes with their frames, axes, points).",
            json!({ "kind": { "type": "string", "enum": ["faces", "edges", "datums"] }, "limit": { "type": "integer", "description": "Max items (default 200)" } }), &["kind"]),
        tool("add_datum_plane", "Add a datum plane (DTM) offset from a datum plane or planar face.",
            json!({ "reference": plane_schema("Reference plane or face"), "offset": dim_schema("Offset distance in mm along the reference normal (negative = opposite side)"), "name": name }), &["reference", "offset"]),
        tool("add_sketch", "Add a standalone sketch feature (Skizze) that extrude/revolve can reuse via sketch_feature.",
            json!({ "plane": plane_schema("Sketch plane"), "shapes": shapes_schema(), "name": name }), &["plane", "shapes"]),
        tool("add_extrude", "Add an extrude (Profil): a sketched section extruded normal to its plane, adding material or cutting (remove: true). By default material grows along the plane normal; cuts sketched on a face go into the material.",
            json!({ "plane": plane_schema("Sketch plane"), "shapes": shapes_schema(), "sketch_feature": { "type": ["integer", "string"], "description": "Use the section and plane of an existing sketch feature instead of plane + shapes" },
                "depth": dim_schema("Depth in mm (default 20)"), "depth_type": { "type": "string", "enum": ["blind", "symmetric", "through_all", "to_next"], "description": "default blind" },
                "remove": { "type": "boolean", "description": "Cut material (default false)" }, "flip": { "type": "boolean", "description": "Reverse the default direction" },
                "taper": dim_schema("Draft angle in degrees"), "thickness": dim_schema("Make a thin feature with this wall thickness"), "name": name }), &["depth"]),
        tool("add_revolve", "Add a revolve (Drehen): the section is revolved about the first centerline in its shapes.",
            json!({ "plane": plane_schema("Sketch plane"), "shapes": shapes_schema(), "sketch_feature": { "type": ["integer", "string"] }, "angle": dim_schema("Angle in degrees (default 360)"),
                "symmetric": { "type": "boolean" }, "remove": { "type": "boolean" }, "flip": { "type": "boolean" }, "name": name }), &[]),
        tool("add_hole", "Add a hole (Bohrung) drilled into the placement plane or face at local coordinates (x, y), against the plane normal.",
            json!({ "plane": plane_schema("Placement plane, usually a face such as \"+Y\""), "x": dim_schema("Local x (mm)"), "y": dim_schema("Local y (mm)"),
                "diameter": dim_schema("Drill diameter (mm, default 10)"), "depth": dim_schema("Depth (mm, default 20)"), "through_all": { "type": "boolean", "description": "Drill through the whole part" },
                "type": { "type": "string", "enum": ["simple", "counterbore", "countersink", "standard"] }, "cb_diameter": dim_schema("Counterbore diameter"), "cb_depth": dim_schema("Counterbore depth"),
                "cs_diameter": dim_schema("Countersink diameter"), "cs_angle": dim_schema("Countersink angle (°)"), "size": { "type": "string", "description": "Standard hole thread size, e.g. M6" },
                "tapped": { "type": "boolean", "description": "Standard hole: tapped (default true)" }, "flip": { "type": "boolean" }, "name": name }), &["plane"]),
        tool("add_round", "Add a round (Rundung) of constant radius on edges.", json!({ "edges": edges_schema(), "radius": dim_schema("Radius in mm"), "name": name }), &["edges", "radius"]),
        tool("add_chamfer", "Add an edge chamfer (Fase).",
            json!({ "edges": edges_schema(), "distance": dim_schema("D (mm)"), "distance2": dim_schema("D2 for kind D1xD2"), "angle": dim_schema("Angle for kind AnglexD"),
                "kind": { "type": "string", "enum": ["DxD", "D1xD2", "AnglexD"] }, "name": name }), &["edges", "distance"]),
        tool("add_shell", "Add a shell (Schale): hollow the part with the given wall thickness, removing the chosen faces.",
            json!({ "faces": { "type": "array", "items": { "type": ["integer", "string", "object"] }, "description": "Faces to remove: indices or directions like \"+Y\"" }, "thickness": dim_schema("Wall thickness"), "name": name }), &["faces", "thickness"]),
        tool("add_pattern", "Add a pattern (Muster) of features: linear along one or two directions, or around an axis.",
            json!({ "features": features, "kind": { "type": "string", "enum": ["direction", "axis"] },
                "direction": { "type": ["string", "integer"], "description": "X, Y, Z or a datum axis (direction pattern)" }, "count": dim_schema("Instances in direction 1 / around the axis"), "spacing": dim_schema("Spacing (mm) in direction 1"),
                "direction2": { "type": ["string", "integer"] }, "count2": dim_schema("Instances in direction 2"), "spacing2": dim_schema("Spacing in direction 2"),
                "axis": { "type": ["string", "integer"], "description": "Axis pattern: X, Y, Z, a datum axis or a hole" }, "angle": dim_schema("Axis pattern: angle between instances (°)"), "name": name }), &["features", "count"]),
        tool("add_mirror", "Mirror features (or, with no features, all geometry) about a plane.",
            json!({ "plane": plane_schema("Mirror plane"), "features": features, "name": name }), &["plane"]),
        tool("add_feature_json", "Advanced: add any feature from its JSON definition, e.g. {\"DatumPoint\": {\"x\": \"0\", \"y\": \"10\", \"z\": \"0\"}}. Use get_feature on an existing feature to see the format.",
            json!({ "kind": { "type": "object", "description": "Externally tagged FeatureKind JSON" }, "name": name }), &["kind"]),
        tool("edit_feature", "Change a feature: rename it, set fields of its definition (dimensions, flags), replace its sketch, plane, edges or faces. Paths are field names from get_feature, dotted for nesting (\"depth\", \"sets.0.radius\"). The model regenerates; a failing change is rolled back.",
            json!({ "feature": feature, "set": { "type": "object", "description": "Field path → new value, e.g. {\"depth\": 30, \"remove\": true, \"depth_type\": \"ThroughAll\"}" },
                "name": { "type": "string", "description": "New feature name" }, "shapes": shapes_schema(), "plane": plane_schema("New sketch / placement / reference plane"),
                "edges": edges_schema(), "faces": { "type": "array", "items": { "type": ["integer", "string", "object"] } } }), &["feature"]),
        tool("suppress_features", "Suppress features (they stay in the tree but do not regenerate).", json!({ "features": features }), &["features"]),
        tool("resume_features", "Resume suppressed features.", json!({ "features": features }), &["features"]),
        tool("delete_features", "Delete features and every feature that depends on them.", json!({ "features": features }), &["features"]),
        tool("set_parameters", "Create, change or delete (value null) part parameters. Dimensions can use them in expressions.",
            json!({ "parameters": { "type": "object", "description": "NAME → value or expression, e.g. {\"W\": 120, \"T\": \"W/6\"}; null deletes" } }), &["parameters"]),
        tool("set_relations", "Replace the part relations (Beziehungen): one NAME = expression per line.", json!({ "relations": { "type": "string" } }), &["relations"]),
        tool("set_model_properties", "Rename the part, set its material density or colour.",
            json!({ "name": { "type": "string" }, "density_g_cm3": { "type": "number", "description": "e.g. 7.85 steel, 2.70 aluminium, 1.24 PLA" }, "color": { "type": "array", "items": { "type": "number" }, "description": "RGB 0..1" } }), &[]),
        tool("regenerate", "Regenerate the part and report errors, volume and bounding box.", json!({}), &[]),
        tool("undo", "Undo the last change.", json!({}), &[]),
        tool("mass_properties", "Volume (mm³), surface area (mm²), mass (kg), centre of gravity and inertia tensor of the solid.",
            json!({ "density_g_cm3": { "type": "number", "description": "Override the part density" } }), &[]),
        tool("export_model", "Export the solid as STL (binary/ASCII), OBJ, STEP, 3MF, PLY, VRML or glTF.",
            json!({ "path": { "type": "string" }, "format": { "type": "string", "enum": ["stl", "stl_ascii", "obj", "step", "3mf", "ply", "vrml", "gltf"], "description": "Default: from the file extension" } }), &["path"]),
        tool("import_geometry", "Import a STEP, STL or OBJ file into the current part as an import feature.", json!({ "path": { "type": "string" } }), &["path"]),
        tool("render_png", "Render a shaded isometric picture of the part. Returns the image; also writes it when a path is given.",
            json!({ "path": { "type": "string" }, "width": { "type": "integer", "description": "default 800" }, "height": { "type": "integer", "description": "default 600" } }), &[]),
    ]
}

/// Result of a tool: JSON payload and an optional PNG image.
pub struct ToolOutput { pub value: Value, pub png: Option<Vec<u8>> }

impl From<Value> for ToolOutput { fn from(value: Value) -> Self { ToolOutput { value, png: None } } }

impl Session {
    pub fn call(&mut self, name: &str, args: &Map<String, Value>) -> Result<ToolOutput, String> {
        let a = Args(args);
        let out: Value = match name {
            "new_part" => {
                let n = a.str("name").map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty()).unwrap_or_else(|| "PRT0001".into());
                self.replace(ModelData::new_part_template(&n), None);
                json!({ "name": n, "features": self.data.features.len() })
            }
            "open_model" => {
                let p = resolve_path(a.req_str("path")?);
                let lower = p.to_string_lossy().to_lowercase();
                if [".step", ".stp", ".stl", ".obj"].iter().any(|e| lower.ends_with(e)) {
                    let (def, _, note) = crate::app::App::read_neutral(&p)?;
                    let stem = p.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_else(|| "IMPORT".into());
                    let n: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
                    let mut d = ModelData::new_part_template(&n);
                    let kind = FeatureKind::Import(def);
                    d.features.push(Feature { id: d.next_id, name: d.next_name(&kind), suppressed: false, hidden: false, kind });
                    d.next_id += 1;
                    self.replace(d, None);
                    json!({ "opened": p, "imported": note, "model": self.brief() })
                } else {
                    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
                    let d: ModelData = serde_json::from_str(&text).map_err(|e| format!("{} is not a Freo part: {}", p.display(), e))?;
                    self.replace(d, Some(p.clone()));
                    json!({ "opened": p, "name": self.data.name, "features": self.data.features.len(), "model": self.brief() })
                }
            }
            "save_model" => {
                let p = match a.str("path") { Some(s) => resolve_path(s), None => self.path.clone().ok_or("No file yet: give a path.")? };
                if let Some(dir) = p.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
                let text = serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string())?;
                std::fs::write(&p, text).map_err(|e| format!("{}: {}", p.display(), e))?;
                self.path = Some(p.clone());
                self.dirty = false;
                json!({ "saved": p })
            }
            "get_model_info" => {
                let m = crate::app::export::mass_properties(&self.result, self.data.density);
                let mut params = Map::new();
                for p in &self.data.parameters {
                    params.insert(p.name.clone(), json!({ "expr": p.expr, "value": self.result.vars.get(&p.name.to_uppercase()).map(|v| r(*v)), "note": p.note }));
                }
                json!({
                    "name": self.data.name, "file": self.path, "unsaved_changes": self.dirty, "units": "mm",
                    "density_g_cm3": r(self.data.density * 1e6), "features": self.data.features.len(),
                    "parameters": params, "relations": self.data.relations,
                    "solid": !self.result.polys.is_empty(), "volume_mm3": r(m.volume), "mass_kg": r(m.mass),
                    "bbox": bbox(&self.result.topo.bbox), "faces": self.result.topo.faces.len(), "edges": self.result.topo.chains.len(),
                    "errors": self.error_list(),
                })
            }
            "list_features" => {
                let list: Vec<Value> = self.data.features.iter().map(|f| {
                    let ty = serde_json::to_value(&f.kind).ok().and_then(|v| match v { Value::Object(o) => o.keys().next().cloned(), Value::String(s) => Some(s), _ => None }).unwrap_or_default();
                    json!({ "id": f.id, "name": f.name, "type": ty, "type_de": f.kind.type_name(), "suppressed": f.suppressed,
                        "error": self.result.errors.get(&f.id), "rolled_back": self.result.rolled_back.contains(&f.id),
                        "references": f.kind.references(), "summary": self.summary(&f.kind) })
                }).collect();
                json!({ "features": list })
            }
            "get_feature" => {
                let f = self.data.feature(self.feature_id(a.req("feature")?)?).unwrap();
                let mut v = serde_json::to_value(f).map_err(|e| e.to_string())?;
                if let Some(s) = sketch_of(&f.kind) { v["sketch_entities"] = sketch_json(s); }
                v
            }
            "list_geometry" => self.list_geometry(a.req_str("kind")?, a.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize)?,
            "add_datum_plane" => {
                let mut d = DatumPlaneDef::builtin(PlaneKind::Offset);
                d.reference = Some(self.plane_ref(a.req("reference")?)?);
                d.offset = a.req_dim("offset")?;
                self.add_feature(FeatureKind::DatumPlane(d), a.str("name"))?
            }
            "add_sketch" => {
                let kind = FeatureKind::Sketch(SketchDef { plane: Some(self.plane_ref(a.req("plane")?)?), sketch: build_sketch(a.req("shapes")?)? });
                self.add_feature(kind, a.str("name"))?
            }
            "add_extrude" => {
                let FeatureKind::Extrude(mut e) = FeatureKind::new_extrude() else { unreachable!() };
                e.remove = a.bool("remove", false);
                let (plane, sk) = self.section(&a)?;
                e.sketch = sk;
                let mut k = FeatureKind::Extrude(e);
                k.set_sketch_plane(plane);
                let FeatureKind::Extrude(mut e) = k else { unreachable!() };
                if a.bool("flip", false) { e.flip = !e.flip; }
                if let Some(d) = a.dim("depth")? { e.depth = d; }
                e.depth_type = match a.str("depth_type").unwrap_or("blind") {
                    "blind" => DepthType::Blind, "symmetric" => DepthType::Symmetric, "through_all" => DepthType::ThroughAll, "to_next" => DepthType::ToNext,
                    t => return Err(format!("Unknown depth_type {}", t)),
                };
                if let Some(t) = a.dim("taper")? { e.taper = t; }
                if let Some(t) = a.dim("thickness")? { e.thin = true; e.thickness = t; }
                self.add_feature(FeatureKind::Extrude(e), a.str("name"))?
            }
            "add_revolve" => {
                let FeatureKind::Revolve(mut e) = FeatureKind::new_revolve() else { unreachable!() };
                let (plane, sk) = self.section(&a)?;
                if sk.centerline_axis().is_none() { return Err("A revolve needs a centerline shape (the axis of revolution) in its sketch.".into()); }
                e.plane = Some(plane);
                e.sketch = sk;
                e.remove = a.bool("remove", false);
                e.flip = a.bool("flip", false);
                e.symmetric = a.bool("symmetric", false);
                if let Some(d) = a.dim("angle")? { e.angle = d; }
                self.add_feature(FeatureKind::Revolve(e), a.str("name"))?
            }
            "add_hole" => {
                let FeatureKind::Hole(mut h) = FeatureKind::new_hole() else { unreachable!() };
                h.plane = Some(self.plane_ref(a.req("plane")?)?);
                for (k, slot) in [("x", &mut h.x), ("y", &mut h.y), ("diameter", &mut h.diameter), ("depth", &mut h.depth), ("cb_diameter", &mut h.cb_diameter),
                    ("cb_depth", &mut h.cb_depth), ("cs_diameter", &mut h.cs_diameter), ("cs_angle", &mut h.cs_angle)] {
                    if let Some(d) = a.dim(k)? { *slot = d; }
                }
                h.through_all = a.bool("through_all", false);
                h.flip = a.bool("flip", false);
                h.tapped = a.bool("tapped", true);
                h.ty = match a.str("type").unwrap_or("simple") {
                    "simple" => HoleType::Simple, "counterbore" => HoleType::Counterbore, "countersink" => HoleType::Countersink, "standard" => HoleType::Standard,
                    t => return Err(format!("Unknown hole type {}", t)),
                };
                if let Some(s) = a.str("size") { h.size = s.trim().to_string(); }
                self.add_feature(FeatureKind::Hole(h), a.str("name"))?
            }
            "add_round" => {
                let mut set = RoundSet::new(3.0);
                set.edges = self.edge_refs(a.req("edges")?)?;
                set.radius = a.req_dim("radius")?;
                self.add_feature(FeatureKind::Round(RoundDef { sets: vec![set] }), a.str("name"))?
            }
            "add_chamfer" => {
                let FeatureKind::Chamfer(mut c) = FeatureKind::new_chamfer() else { unreachable!() };
                c.edges = self.edge_refs(a.req("edges")?)?;
                c.d1 = a.req_dim("distance")?;
                c.d2 = a.dim("distance2")?.unwrap_or_else(|| c.d1.clone());
                if let Some(d) = a.dim("angle")? { c.angle = d; }
                c.kind = match a.str("kind").unwrap_or("DxD") {
                    "DxD" => ChamferKind::DxD, "D1xD2" => ChamferKind::D1xD2, "AnglexD" => ChamferKind::AnglexD,
                    k => return Err(format!("Unknown chamfer kind {}", k)),
                };
                self.add_feature(FeatureKind::Chamfer(c), a.str("name"))?
            }
            "add_shell" => {
                let faces = self.face_refs(a.req("faces")?)?;
                self.add_feature(FeatureKind::Shell(ShellDef { faces, thickness: a.req_dim("thickness")? }), a.str("name"))?
            }
            "add_pattern" => {
                let FeatureKind::Pattern(mut p) = FeatureKind::new_pattern() else { unreachable!() };
                p.features = self.feature_ids(a.get("features"))?;
                if p.features.is_empty() { return Err("Choose the features to pattern.".into()); }
                let axis_kind = a.str("kind") == Some("axis") || a.get("axis").is_some();
                if axis_kind {
                    p.kind = PatternKind::Axis;
                    p.axis = self.axis_ref(a.get("axis").unwrap_or(&json!("Y")))?;
                    p.axis_count = a.req_dim("count")?;
                    if let Some(d) = a.dim("angle")? { p.axis_angle = d; }
                } else {
                    p.kind = PatternKind::Direction;
                    p.dir1 = self.axis_ref(a.get("direction").unwrap_or(&json!("X")))?;
                    p.count1 = a.req_dim("count")?;
                    if let Some(d) = a.dim("spacing")? { p.spacing1 = d; }
                    if let Some(d2) = a.get("direction2") {
                        p.use_dir2 = true;
                        p.dir2 = self.axis_ref(d2)?;
                        if let Some(d) = a.dim("count2")? { p.count2 = d; }
                        if let Some(d) = a.dim("spacing2")? { p.spacing2 = d; }
                    }
                }
                self.add_feature(FeatureKind::Pattern(p), a.str("name"))?
            }
            "add_mirror" => {
                let m = MirrorDef { plane: Some(self.plane_ref(a.req("plane")?)?), features: self.feature_ids(a.get("features"))? };
                self.add_feature(FeatureKind::Mirror(m), a.str("name"))?
            }
            "add_feature_json" => {
                let kind: FeatureKind = serde_json::from_value(a.req("kind")?.clone()).map_err(|e| format!("Not a valid feature definition: {}", e))?;
                self.add_feature(kind, a.str("name"))?
            }
            "edit_feature" => self.edit_feature(&a)?,
            "suppress_features" | "resume_features" => {
                let ids = self.feature_ids(a.get("features"))?;
                if ids.is_empty() { return Err("No features given.".into()); }
                let s = name == "suppress_features";
                self.commit(|d| { for f in &mut d.features { if ids.contains(&f.id) { f.suppressed = s; } } Ok(()) })?;
                let mut out = json!({ "model": self.brief() });
                out[if s { "suppressed" } else { "resumed" }] = json!(ids);
                out
            }
            "delete_features" => {
                let mut all = self.feature_ids(a.get("features"))?;
                if all.is_empty() { return Err("No features given.".into()); }
                if all.iter().any(|id| [FRONT_ID, TOP_ID, RIGHT_ID, CSYS_ID].contains(id)) { return Err("The default datum planes and BASIS cannot be deleted.".into()); }
                loop {
                    let extra: Vec<u64> = self.data.features.iter().filter(|f| !all.contains(&f.id) && f.kind.references().iter().any(|r| all.contains(r))).map(|f| f.id).collect();
                    if extra.is_empty() { break; }
                    all.extend(extra);
                }
                let names: Vec<String> = all.iter().filter_map(|id| self.data.feature(*id).map(|f| f.name.clone())).collect();
                self.commit(|d| {
                    d.features.retain(|f| !all.contains(&f.id));
                    if let Some(ii) = d.insert_index { d.insert_index = Some(ii.min(d.features.len())); }
                    Ok(())
                })?;
                json!({ "deleted": names, "model": self.brief() })
            }
            "set_parameters" => {
                let map = a.req("parameters")?.as_object().ok_or("parameters must be an object NAME → value")?.clone();
                self.commit(|d| {
                    for (k, v) in &map {
                        let k = k.trim();
                        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || k.chars().next().unwrap().is_ascii_digit() {
                            return Err(format!("Invalid parameter name {:?}", k));
                        }
                        if v.is_null() { d.parameters.retain(|p| !p.name.eq_ignore_ascii_case(k)); continue; }
                        let expr = dim_value(v, k)?.0;
                        match d.parameters.iter_mut().find(|p| p.name.eq_ignore_ascii_case(k)) {
                            Some(p) => p.expr = expr,
                            None => d.parameters.push(Parameter { name: k.to_uppercase(), expr, note: String::new() }),
                        }
                    }
                    Ok(())
                })?;
                let vals: Map<String, Value> = self.data.parameters.iter().map(|p| (p.name.clone(), json!(self.result.vars.get(&p.name.to_uppercase()).map(|v| r(*v))))).collect();
                json!({ "parameters": vals, "model": self.brief() })
            }
            "set_relations" => {
                let rel = a.req_str("relations")?.to_string();
                self.commit(|d| { d.relations = rel; Ok(()) })?;
                json!({ "relations": self.data.relations, "model": self.brief() })
            }
            "set_model_properties" => {
                let name = a.str("name").map(|s| s.trim().to_uppercase());
                let dens = a.f64("density_g_cm3")?;
                let color = a.get("color").map(|c| nums(c, 3, "color")).transpose()?;
                self.commit(|d| {
                    if let Some(n) = name.filter(|n| !n.is_empty()) { d.name = n; }
                    if let Some(x) = dens { if x <= 0.0 { return Err("density must be positive".into()); } d.density = x * 1e-6; }
                    if let Some(c) = color { d.appearance.color = [c[0] as f32, c[1] as f32, c[2] as f32]; }
                    Ok(())
                })?;
                json!({ "name": self.data.name, "density_g_cm3": r(self.data.density * 1e6), "color": self.data.appearance.color })
            }
            "regenerate" => {
                self.result = regen::run(&self.data);
                json!({ "ok": self.result.errors.is_empty() && self.result.param_errors.is_empty(), "time_ms": r(self.result.time * 1000.0), "model": self.brief() })
            }
            "undo" => {
                let prev = self.undo.pop().ok_or("Nothing to undo.")?;
                self.data = prev;
                self.result = regen::run(&self.data);
                self.dirty = true;
                json!({ "undone": true, "features": self.data.features.len(), "model": self.brief() })
            }
            "mass_properties" => {
                let dens = a.f64("density_g_cm3")?.map(|x| x * 1e-6).unwrap_or(self.data.density);
                let m = crate::app::export::mass_properties(&self.result, dens);
                json!({ "volume_mm3": r(m.volume), "area_mm2": r(m.area), "density_g_cm3": r(dens * 1e6), "mass_kg": (m.mass * 1e6).round() / 1e6,
                    "center_of_gravity": v3(m.cog), "inertia_kg_mm2": { "xx": r(m.inertia[0]), "yy": r(m.inertia[1]), "zz": r(m.inertia[2]), "xy": r(m.inertia[3]), "yz": r(m.inertia[4]), "zx": r(m.inertia[5]) },
                    "bbox": bbox(&m.bbox) })
            }
            "export_model" => {
                let p = resolve_path(a.req_str("path")?);
                let fmt = export_format(a.str("format"), &p)?;
                if self.result.polys.is_empty() { return Err("The part has no solid geometry to export.".into()); }
                if let Some(dir) = p.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
                crate::app::export::write(&self.result, &self.data, fmt, &p)?;
                let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                json!({ "exported": p, "format": fmt.name(), "bytes": bytes, "triangles": crate::app::export::triangles(&self.result).len() })
            }
            "import_geometry" => {
                let p = resolve_path(a.req_str("path")?);
                let (def, _, note) = crate::app::App::read_neutral(&p)?;
                let mut out = self.add_feature(FeatureKind::Import(def), a.str("name"))?;
                out["imported"] = json!(note);
                out
            }
            "render_png" => {
                let w = a.get("width").and_then(Value::as_u64).unwrap_or(800).clamp(16, 2048) as usize;
                let h = a.get("height").and_then(Value::as_u64).unwrap_or(600).clamp(16, 2048) as usize;
                let img = crate::softrender::render(&self.result, w, h, self.data.appearance.color);
                let png = crate::softrender::encode_png(&img);
                let mut v = json!({ "width": w, "height": h, "bytes": png.len() });
                if let Some(s) = a.str("path") {
                    let p = resolve_path(s);
                    if let Some(dir) = p.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
                    std::fs::write(&p, &png).map_err(|e| format!("{}: {}", p.display(), e))?;
                    v["written"] = json!(p);
                }
                return Ok(ToolOutput { value: v, png: Some(png) });
            }
            _ => return Err(format!("Unknown tool: {}", name)),
        };
        Ok(out.into())
    }

    /// Sketch plane and section from `plane` + `shapes` or from `sketch_feature`.
    fn section(&self, a: &Args) -> Result<(PlaneRef, Sketch), String> {
        if let Some(sf) = a.get("sketch_feature") {
            let id = self.feature_id(sf)?;
            return match &self.data.feature(id).unwrap().kind {
                FeatureKind::Sketch(s) => Ok((s.plane.ok_or("The sketch has no plane.")?, s.sketch.clone())),
                _ => Err("sketch_feature must name a sketch feature.".into()),
            };
        }
        let plane = self.plane_ref(a.get("plane").ok_or("Give plane and shapes, or sketch_feature.")?)?;
        Ok((plane, build_sketch(a.get("shapes").ok_or("Give the section as shapes.")?)?))
    }

    fn face_refs(&self, v: &Value) -> Result<Vec<FaceRef>, String> {
        match v {
            Value::Array(a) => a.iter().map(|x| self.face_ref(x)).collect(),
            x => Ok(vec![self.face_ref(x)?]),
        }
    }

    fn list_geometry(&self, kind: &str, limit: usize) -> Result<Value, String> {
        let t = &self.result.topo;
        Ok(match kind {
            "faces" => {
                let faces: Vec<Value> = (0..t.faces.len()).take(limit).map(|i| {
                    let f = &t.faces[i];
                    let mut v = json!({ "index": i, "planar": f.planar, "center": v3(f.center), "normal": v3(f.normal), "area": r(f.area) });
                    if let Some(fr) = t.face_frame(i) { v["local_x"] = v3(fr.x); v["local_y"] = v3(fr.y); }
                    if let Some(id) = self.result.tag_owner.get(&t.face_tag(i)) { v["feature"] = json!(id); }
                    v
                }).collect();
                json!({ "count": t.faces.len(), "faces": faces })
            }
            "edges" => {
                let edges: Vec<Value> = t.chains.iter().enumerate().take(limit).map(|(i, c)| json!({
                    "index": i, "start": v3(c.points[0]), "end": v3(*c.points.last().unwrap()), "mid": v3(c.mid()), "length": r(c.length()),
                    "straight": c.is_straight(), "closed": c.closed, "convex": c.convex, "faces": [c.face_a, c.face_b],
                })).collect();
                json!({ "count": t.chains.len(), "edges": edges })
            }
            "datums" => {
                let planes: Vec<Value> = self.result.planes.iter().map(|p| json!({ "id": p.id, "name": p.name, "origin": v3(p.frame.origin), "normal": v3(p.frame.z()), "local_x": v3(p.frame.x), "local_y": v3(p.frame.y) })).collect();
                let axes: Vec<Value> = self.result.axes.iter().map(|x| json!({ "id": x.id, "name": x.name, "feature": x.feature, "point": v3(x.point), "direction": v3(x.dir) })).collect();
                let points: Vec<Value> = self.result.points.iter().map(|p| json!({ "id": p.id, "name": p.name, "point": v3(p.p) })).collect();
                json!({ "planes": planes, "axes": axes, "points": points })
            }
            k => return Err(format!("Unknown geometry kind {}; use faces, edges or datums.", k)),
        })
    }

    fn edit_feature(&mut self, a: &Args) -> Result<Value, String> {
        let id = self.feature_id(a.req("feature")?)?;
        let f = self.data.feature(id).unwrap().clone();
        let mut kind = f.kind.clone();
        if let Some(shapes) = a.get("shapes") {
            let sk = build_sketch(shapes)?;
            match sketch_of_mut(&mut kind) { Some(s) => *s = sk, None => return Err(format!("{} has no sketch.", f.name)) }
        }
        if let Some(p) = a.get("plane") {
            let p = self.plane_ref(p)?;
            match &mut kind {
                FeatureKind::Hole(h) => h.plane = Some(p),
                FeatureKind::DatumPlane(d) if !matches!(d.kind, PlaneKind::Front | PlaneKind::Top | PlaneKind::Right) => d.reference = Some(p),
                FeatureKind::Mirror(m) => m.plane = Some(p),
                k if k.sketch_plane().is_some() || k.has_sketch() => k.set_sketch_plane(p),
                _ => return Err(format!("{} has no plane to change.", f.name)),
            }
        }
        if let Some(e) = a.get("edges") {
            let edges = self.edge_refs(e)?;
            match &mut kind {
                FeatureKind::Round(rd) => rd.sets[0].edges = edges,
                FeatureKind::Chamfer(c) => c.edges = edges,
                _ => return Err(format!("{} has no edges.", f.name)),
            }
        }
        if let Some(fs) = a.get("faces") {
            let faces = self.face_refs(fs)?;
            match &mut kind {
                FeatureKind::Shell(s) => s.faces = faces,
                FeatureKind::Draft(d) => d.faces = faces,
                _ => return Err(format!("{} has no faces.", f.name)),
            }
        }
        if let Some(set) = a.get("set") {
            let set = set.as_object().ok_or("set must be an object of field path → value")?;
            let mut v = serde_json::to_value(&kind).map_err(|e| e.to_string())?;
            let inner = match &mut v { Value::Object(o) if o.len() == 1 => o.values_mut().next().unwrap(), _ => return Err(format!("{} has no editable fields.", f.name)) };
            for (path, val) in set {
                let mut cur: &mut Value = &mut *inner;
                for seg in path.split('.') {
                    cur = match cur {
                        Value::Object(o) => {
                            if !o.contains_key(seg) {
                                return Err(format!("{} has no field {:?} (fields: {}).", f.name, seg, o.keys().cloned().collect::<Vec<_>>().join(", ")));
                            }
                            o.get_mut(seg).unwrap()
                        }
                        Value::Array(arr) => { let i: usize = seg.parse().map_err(|_| format!("{:?} must be an array index", seg))?; arr.get_mut(i).ok_or(format!("index {} out of range", i))? }
                        _ => return Err(format!("{} cannot be indexed with {:?}", path, seg)),
                    };
                }
                *cur = match (&*cur, val) {
                    // dimensions are stored as expression strings
                    (Value::String(_), Value::Number(n)) => json!(Dim::v(n.as_f64().unwrap_or(0.0)).0),
                    _ => val.clone(),
                };
            }
            kind = serde_json::from_value(v).map_err(|e| format!("Invalid value: {}", e))?;
        }
        let new_name = a.str("name").map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(n) = &new_name {
            if self.data.features.iter().any(|g| g.id != id && g.name.eq_ignore_ascii_case(n)) { return Err(format!("A feature named {} already exists.", n)); }
        }
        self.commit(|d| {
            let g = d.features.iter_mut().find(|g| g.id == id).unwrap();
            g.kind = kind;
            if let Some(n) = new_name { g.name = n; }
            Ok(())
        })?;
        let g = self.data.feature(id).unwrap();
        Ok(json!({ "id": id, "name": g.name, "summary": self.summary(&g.kind), "model": self.brief() }))
    }
}

fn sketch_of(k: &FeatureKind) -> Option<&Sketch> {
    match k {
        FeatureKind::Sketch(d) => Some(&d.sketch), FeatureKind::Extrude(d) => Some(&d.sketch), FeatureKind::Revolve(d) => Some(&d.sketch),
        FeatureKind::Rib(d) => Some(&d.sketch), _ => None,
    }
}

fn sketch_of_mut(k: &mut FeatureKind) -> Option<&mut Sketch> {
    match k {
        FeatureKind::Sketch(d) => Some(&mut d.sketch), FeatureKind::Extrude(d) => Some(&mut d.sketch), FeatureKind::Revolve(d) => Some(&mut d.sketch),
        FeatureKind::Rib(d) => Some(&mut d.sketch), _ => None,
    }
}

// ---------------------------------------------------------------- JSON-RPC

pub struct Server { pub session: Session }

impl Default for Server {
    fn default() -> Self { Server::new() }
}

fn rpc_error(id: Value, code: i64, msg: &str) -> Value { json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }) }
fn rpc_ok(id: Value, result: Value) -> Value { json!({ "jsonrpc": "2.0", "id": id, "result": result }) }

impl Server {
    pub fn new() -> Server { Server { session: Session::new() } }

    /// Handles one line of input; returns the line to send back, if any.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(rpc_error(Value::Null, -32700, &format!("Parse error: {}", e)).to_string()),
        };
        match msg {
            Value::Array(batch) => {
                let out: Vec<Value> = batch.into_iter().filter_map(|m| self.handle(m)).collect();
                if out.is_empty() { None } else { Some(Value::Array(out).to_string()) }
            }
            m => self.handle(m).map(|v| v.to_string()),
        }
    }

    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        let Some(method) = msg.get("method").and_then(Value::as_str).map(str::to_string) else {
            // a response to a server request (we send none) or garbage
            return msg.get("id").filter(|_| msg.get("result").is_none() && msg.get("error").is_none()).map(|id| rpc_error(id.clone(), -32600, "Invalid request"));
        };
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let id = id?; // notifications (initialized, cancelled, …) need no answer
        Some(match method.as_str() {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
                rpc_ok(id, json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false }, "resources": { "listChanged": false, "subscribe": false } },
                    "serverInfo": { "name": "freo", "title": "Freo parametric CAD", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => rpc_ok(id, json!({})),
            "tools/list" => rpc_ok(id, json!({ "tools": tool_list() })),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else { return Some(rpc_error(id, -32602, "tools/call needs a tool name")) };
                if !tool_list().iter().any(|t| t["name"] == name) { return Some(rpc_error(id, -32602, &format!("Unknown tool: {}", name))); }
                let args = params.get("arguments").and_then(Value::as_object).cloned().unwrap_or_default();
                rpc_ok(id, self.call_tool(name, &args))
            }
            "resources/list" => rpc_ok(id, json!({ "resources": [
                { "uri": "freo://model/current", "name": "current-model", "title": "Current part (Freo JSON)", "description": "The complete part definition as saved in .pfprt files", "mimeType": "application/json" },
                { "uri": "freo://model/features", "name": "model-tree", "title": "Model tree", "description": "Features with ids, types, states and main dimensions", "mimeType": "application/json" },
            ] })),
            "resources/templates/list" => rpc_ok(id, json!({ "resourceTemplates": [] })),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                let text = match uri {
                    "freo://model/current" => serde_json::to_string_pretty(&self.session.data).unwrap_or_default(),
                    "freo://model/features" => match self.session.call("list_features", &Map::new()) { Ok(o) => serde_json::to_string_pretty(&o.value).unwrap_or_default(), Err(e) => e },
                    _ => return Some(rpc_error(id, -32002, &format!("Resource not found: {}", uri))),
                };
                rpc_ok(id, json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": text }] }))
            }
            "prompts/list" => rpc_ok(id, json!({ "prompts": [] })),
            "logging/setLevel" => rpc_ok(id, json!({})),
            m => rpc_error(id, -32601, &format!("Method not found: {}", m)),
        })
    }

    /// Runs a tool; tool failures (and panics in the kernel) come back as `isError` results.
    fn call_tool(&mut self, name: &str, args: &Map<String, Value>) -> Value {
        let saved = self.session.data.clone();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.session.call(name, args)));
        let res = match res {
            Ok(r) => r,
            Err(p) => {
                self.session.data = saved;
                self.session.result = regen::run(&self.session.data);
                let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "internal error".into());
                Err(format!("Internal error, change rolled back: {}", msg))
            }
        };
        match res {
            Ok(out) => {
                let text = out.value.to_string();
                let mut content = vec![json!({ "type": "text", "text": text })];
                if let Some(png) = out.png { content.push(json!({ "type": "image", "data": base64(&png), "mimeType": "image/png" })); }
                let mut res = json!({ "content": content, "isError": false });
                if out.value.is_object() { res["structuredContent"] = out.value; }
                res
            }
            Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
        }
    }
}

/// Serves MCP over stdin/stdout until stdin closes.
pub fn serve<R: BufRead, W: Write>(input: R, mut output: W) -> std::io::Result<()> {
    let mut server = Server::new();
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        if let Some(out) = server.handle_line(&line) {
            writeln!(output, "{}", out)?;
            output.flush()?;
        }
    }
    Ok(())
}

pub fn serve_stdio() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    serve(stdin.lock(), std::io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn sketch_shapes_close_loops() {
        let s = build_sketch(&json!([{ "type": "polygon", "points": [[0, 0], [10, 0], [10, 5]] }, { "type": "circle", "diameter": 2, "cx": 6, "cy": 1 }])).unwrap();
        assert!(s.open_ends().is_empty());
        assert_eq!(s.entities.len(), 4);
        let s = build_sketch(&json!([{ "type": "line", "from": [0, 0], "to": [10, 0] }, { "type": "arc", "center": [10, 5], "start": [10, 0], "end": [10, 10] }, { "type": "line", "from": [10, 10], "to": [0, 10] }, { "type": "line", "from": [0, 10], "to": [0, 0] }])).unwrap();
        assert!(s.open_ends().is_empty(), "{:?}", s.open_ends());
    }
}
