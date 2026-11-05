//! Drives the freo-mcp binary over stdin/stdout like an MCP client (Claude Desktop, Claude Code …).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start() -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_freo-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start freo-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Client { child, stdin, stdout, next_id: 1 }
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{}", msg).unwrap();
        self.stdin.flush().unwrap();
    }

    fn send_raw(&mut self, line: &str) -> Value {
        writeln!(self.stdin, "{}", line).unwrap();
        self.stdin.flush().unwrap();
        self.read()
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        assert!(self.stdout.read_line(&mut line).unwrap() > 0, "server closed stdout");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("stdout must carry JSON-RPC only: {} in {:?}", e, line))
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let resp = self.read();
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], id, "responses come back in order: {}", resp);
        resp
    }

    /// Raw tools/call result (content, isError, structuredContent).
    fn call_raw(&mut self, tool: &str, args: Value) -> Value {
        let resp = self.request("tools/call", json!({ "name": tool, "arguments": args }));
        assert!(resp.get("error").is_none(), "{}: {}", tool, resp);
        resp["result"].clone()
    }

    /// A tool call that must succeed; returns its JSON payload.
    fn call(&mut self, tool: &str, args: Value) -> Value {
        let res = self.call_raw(tool, args);
        assert_eq!(res["isError"], false, "{} failed: {}", tool, res["content"][0]["text"]);
        let text: Value = serde_json::from_str(res["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, res["structuredContent"], "text and structured content agree");
        text
    }

    /// A tool call that must fail; returns its message.
    fn call_err(&mut self, tool: &str, args: Value) -> String {
        let res = self.call_raw(tool, args);
        assert_eq!(res["isError"], true, "{} should fail: {}", tool, res);
        res["content"][0]["text"].as_str().unwrap().to_string()
    }

    fn initialize(&mut self) -> Value {
        let r = self.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "freo-test", "version": "1" } }));
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        r
    }
}

impl Drop for Client {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}

fn vol(v: &Value) -> f64 { v["model"]["volume_mm3"].as_f64().unwrap() }

fn out_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("freo_mcp_test_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn handshake_tools_and_protocol_errors() {
    let mut c = Client::start();
    let init = c.initialize();
    let r = &init["result"];
    assert_eq!(r["protocolVersion"], "2025-06-18");
    assert_eq!(r["serverInfo"]["name"], "freo");
    assert!(r["capabilities"]["tools"].is_object());
    assert!(r["instructions"].as_str().unwrap().contains("millimetres"));

    // older clients get their own protocol revision back
    let old = c.request("initialize", json!({ "protocolVersion": "2024-11-05", "capabilities": {} }));
    assert_eq!(old["result"]["protocolVersion"], "2024-11-05");

    let tools = c.request("tools/list", json!({}));
    let tools = tools["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["new_part", "open_model", "save_model", "list_features", "add_datum_plane", "add_sketch", "add_extrude", "add_revolve", "add_hole", "add_round",
        "add_chamfer", "edit_feature", "suppress_features", "resume_features", "delete_features", "regenerate", "mass_properties", "get_model_info", "export_model", "render_png"] {
        assert!(names.contains(&n), "missing tool {}", n);
    }
    for t in tools {
        assert!(t["description"].as_str().is_some_and(|d| !d.is_empty()));
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        assert!(t["annotations"]["readOnlyHint"].is_boolean() && t["annotations"]["destructiveHint"].is_boolean(), "{}", t["name"]);
    }
    let ann = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap()["annotations"].clone();
    assert_eq!(ann("list_features")["readOnlyHint"], true);
    assert_eq!(ann("add_hole")["readOnlyHint"], false);
    assert_eq!(ann("add_hole")["destructiveHint"], false);
    assert_eq!(ann("delete_features")["destructiveHint"], true);
    assert_eq!(ann("export_model")["destructiveHint"], true);

    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
    assert_eq!(c.request("no/such/method", json!({}))["error"]["code"], -32601);
    assert_eq!(c.request("tools/call", json!({ "name": "no_such_tool", "arguments": {} }))["error"]["code"], -32602);
    assert_eq!(c.send_raw("{ not json")["error"]["code"], -32700);

    let res = c.request("resources/list", json!({}));
    assert!(res["result"]["resources"].as_array().unwrap().iter().any(|r| r["uri"] == "freo://model/current"));
    let model = c.request("resources/read", json!({ "uri": "freo://model/current" }));
    let data: Value = serde_json::from_str(model["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(data["name"], "PRT0001");
}

#[test]
fn plate_with_hole_regenerate_and_export_stl() {
    let mut c = Client::start();
    c.initialize();
    let dir = out_dir();

    c.call("new_part", json!({ "name": "plate" }));
    c.call("set_parameters", json!({ "parameters": { "T": 10 } }));
    let ext = c.call("add_extrude", json!({ "plane": "TOP", "shapes": [{ "type": "rect", "width": 100, "height": 60 }], "depth": "T", "name": "PLATTE" }));
    assert_eq!(ext["name"], "PLATTE");
    assert!((vol(&ext) - 60000.0).abs() < 1e-3, "{}", ext);
    assert_eq!(ext["model"]["bbox"]["size"], json!([100.0, 10.0, 60.0]));

    // hole through the top face, 20 mm right of centre
    let hole = c.call("add_hole", json!({ "plane": "+Y", "x": 20, "y": 0, "diameter": 10, "through_all": true }));
    let removed = 60000.0 - vol(&hole);
    let cyl = std::f64::consts::PI * 25.0 * 10.0;
    assert!((removed - cyl).abs() / cyl < 0.01, "hole removes {} ≈ {}", removed, cyl);
    let hole_id = hole["id"].as_u64().unwrap();

    let faces = c.call("list_geometry", json!({ "kind": "faces" }));
    assert!(faces["faces"].as_array().unwrap().iter().any(|f| !f["planar"].as_bool().unwrap()), "the hole adds a cylindrical face");
    let edges = c.call("list_geometry", json!({ "kind": "edges" }));
    assert!(edges["count"].as_u64().unwrap() >= 14);

    // rounds on the four vertical corner edges, chamfer on the top outline
    let rnd = c.call("add_round", json!({ "edges": { "direction": "Y" }, "radius": 5 }));
    let corner = 4.0 * 10.0 * (25.0 - std::f64::consts::PI * 25.0 / 4.0);
    assert!(((vol(&hole) - vol(&rnd)) - corner).abs() < 10.0, "round removes {}", vol(&hole) - vol(&rnd));
    c.call("add_chamfer", json!({ "edges": { "face_dir": "+Y", "closed": false }, "distance": 1 }));

    // parameter-driven depth: doubling T doubles the plate
    let before = c.call("get_model_info", json!({}));
    let p = c.call("set_parameters", json!({ "parameters": { "T": 20 } }));
    assert_eq!(p["parameters"]["T"], 20.0);
    assert!(vol(&p) > 1.9 * before["volume_mm3"].as_f64().unwrap());
    let edited = c.call("edit_feature", json!({ "feature": "PLATTE", "set": { "depth": 15 } }));
    assert_eq!(edited["summary"]["depth"], "15");
    assert_eq!(edited["model"]["bbox"]["size"][1], 15.0);

    // a change that breaks regeneration is rolled back
    let err = c.call_err("edit_feature", json!({ "feature": "PLATTE", "set": { "depth": "NO_SUCH_PARAM * 2" } }));
    assert!(err.contains("rolled back"), "{}", err);
    let msg = c.call_err("add_hole", json!({ "plane": "DTM99", "diameter": 5 }));
    assert!(msg.contains("DTM99"), "{}", msg);

    let regen = c.call("regenerate", json!({}));
    assert_eq!(regen["ok"], true, "{}", regen);
    assert_eq!(regen["model"]["errors"], json!([]));
    assert_eq!(regen["model"]["bbox"]["size"][1], 15.0, "the failed edit left the model unchanged");

    let tree = c.call("list_features", json!({}));
    let names: Vec<&str> = tree["features"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(&names[names.len() - 4..], ["PLATTE", "Bohrung 1", "Rundung 1", "Fase 1"]);

    // suppress / resume / delete / undo the hole
    let full = vol(&regen);
    let s = c.call("suppress_features", json!({ "features": [hole_id] }));
    assert!(vol(&s) > full + 1000.0);
    let r = c.call("resume_features", json!({ "features": ["Bohrung 1"] }));
    assert!((vol(&r) - full).abs() < 1e-6);
    let d = c.call("delete_features", json!({ "features": [hole_id] }));
    assert_eq!(d["deleted"], json!(["Bohrung 1"]));
    let u = c.call("undo", json!({}));
    assert!((vol(&u) - full).abs() < 1e-6);

    let m = c.call("mass_properties", json!({ "density_g_cm3": 2.7 }));
    assert!((m["volume_mm3"].as_f64().unwrap() - full).abs() < 1e-3);
    assert!((m["mass_kg"].as_f64().unwrap() - full * 2.7e-6).abs() < 1e-5);
    assert!(m["center_of_gravity"][1].as_f64().unwrap() > 7.0 && m["center_of_gravity"][1].as_f64().unwrap() < 8.0);

    // export STL: binary size is 84 + 50 bytes per triangle
    let stl = dir.join("plate.stl");
    let e = c.call("export_model", json!({ "path": stl }));
    let bytes = std::fs::read(&stl).unwrap();
    let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    assert_eq!(n as u64, e["triangles"].as_u64().unwrap());
    assert_eq!(bytes.len(), 84 + n * 50);
    let step = dir.join("plate.step");
    c.call("export_model", json!({ "path": step }));
    assert!(std::fs::read_to_string(&step).unwrap().starts_with("ISO-10303-21;"));
    c.call("export_model", json!({ "path": dir.join("plate.bin"), "format": "3mf" }));
    assert_eq!(&std::fs::read(dir.join("plate.bin")).unwrap()[..4], b"PK\x03\x04");

    // save, reopen, same part
    let file = dir.join("plate.pfprt");
    c.call("save_model", json!({ "path": file }));
    c.call("new_part", json!({}));
    let o = c.call("open_model", json!({ "path": file }));
    assert_eq!(o["name"], "PLATE");
    assert!((vol(&o) - full).abs() < 1e-6);

    // the PNG comes back as image content
    let res = c.call_raw("render_png", json!({ "width": 320, "height": 240, "path": dir.join("plate.png") }));
    assert_eq!(res["content"][1]["type"], "image");
    assert_eq!(res["content"][1]["mimeType"], "image/png");
    assert!(res["content"][1]["data"].as_str().unwrap().starts_with("iVBORw0KGgo"), "PNG signature");
    assert_eq!(&std::fs::read(dir.join("plate.png")).unwrap()[..4], b"\x89PNG");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn datum_plane_sketch_revolve_pattern_and_shell() {
    let mut c = Client::start();
    c.initialize();
    c.call("new_part", json!({ "name": "flange" }));
    // a revolved disc: profile on FRONT, revolved about the vertical centerline
    let rev = c.call("add_revolve", json!({ "plane": "FRONT", "shapes": [
        { "type": "centerline", "from": [0, -10], "to": [0, 10] },
        { "type": "rect", "width": 40, "height": 10, "cx": 20, "cy": 5 }
    ] }));
    let disc = std::f64::consts::PI * 40.0 * 40.0 * 10.0;
    assert!((vol(&rev) - disc).abs() / disc < 0.01, "disc {}", vol(&rev));
    assert!(c.call_err("add_revolve", json!({ "plane": "FRONT", "shapes": [{ "type": "circle", "radius": 5, "cx": 20 }] })).contains("centerline"));

    // a boss sketched on a datum plane 10 mm above TOP, reusing a sketch feature
    let dtm = c.call("add_datum_plane", json!({ "reference": "TOP", "offset": 10 }));
    assert_eq!(dtm["name"], "DTM1");
    c.call("add_sketch", json!({ "plane": "DTM1", "shapes": [{ "type": "circle", "diameter": 20 }], "name": "BOSS_SK" }));
    let boss = c.call("add_extrude", json!({ "sketch_feature": "BOSS_SK", "depth": 15 }));
    assert_eq!(boss["model"]["bbox"]["max"][1], 25.0);

    // bolt hole on the flange face, patterned 6× around Y
    // "+Y" would pick the boss top (the outermost face), so pick the flange face by index
    let faces = c.call("list_geometry", json!({ "kind": "faces" }));
    let flange_top = faces["faces"].as_array().unwrap().iter()
        .find(|f| f["planar"] == true && f["normal"] == json!([0.0, 1.0, 0.0]) && f["center"][1] == 10.0 && f["area"].as_f64().unwrap() > 3000.0)
        .map(|f| f["index"].as_u64().unwrap());
    let h = c.call("add_hole", json!({ "plane": { "face": flange_top.expect("flange top face") }, "x": 30, "y": 0, "diameter": 6, "through_all": true }));
    let one = std::f64::consts::PI * 9.0 * 10.0;
    let before = vol(&h);
    let pat = c.call("add_pattern", json!({ "features": [h["id"]], "axis": "Y", "count": 6, "angle": 60 }));
    assert!(((before - vol(&pat)) - 5.0 * one).abs() / (5.0 * one) < 0.05, "pattern removes {}", before - vol(&pat));

    // edit the sketch of the boss through edit_feature shapes
    let e = c.call("edit_feature", json!({ "feature": boss["id"], "shapes": [{ "type": "rect", "width": 20, "height": 20 }] }));
    assert!(e["model"]["errors"].as_array().unwrap().is_empty());
    let f = c.call("get_feature", json!({ "feature": boss["id"] }));
    assert_eq!(f["sketch_entities"].as_array().unwrap().len(), 4);

    // advanced: any feature from JSON
    let p = c.call("add_feature_json", json!({ "kind": { "DatumPoint": { "x": "0", "y": "40", "z": "0" } } }));
    assert_eq!(p["type"], "Bezugspunkt");
    let datums = c.call("list_geometry", json!({ "kind": "datums" }));
    assert!(datums["points"].as_array().unwrap().iter().any(|q| q["point"] == json!([0.0, 40.0, 0.0])));

    // shell an open box
    c.call("new_part", json!({ "name": "box" }));
    c.call("add_extrude", json!({ "plane": "TOP", "shapes": [{ "type": "rect", "width": 40, "height": 40 }], "depth": 20 }));
    let s = c.call("add_shell", json!({ "faces": ["+Y"], "thickness": 2 }));
    let expect = 40.0 * 40.0 * 20.0 - 36.0 * 36.0 * 18.0;
    assert!((vol(&s) - expect).abs() < 1.0, "shell {} ≈ {}", vol(&s), expect);
}
