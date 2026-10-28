//! Every export format writes a file that other tools can read back.

use freo::app::export::{write, Format};
use freo::kernel::math::*;
use freo::model::features::*;
use freo::model::regen;
use freo::sketch::sketch::Sketch;

fn block() -> (ModelData, regen::RegenResult) {
    let mut m = ModelData::new_part("BLOCK");
    let FeatureKind::Extrude(mut d) = FeatureKind::new_extrude() else { unreachable!() };
    d.plane = Some(PlaneRef::Datum(TOP_ID));
    d.sketch = Sketch::rect(40.0, 30.0, 0.0, 0.0);
    d.depth = Dim::v(10.0);
    m.features.push(Feature { id: 10, name: "Profil 1".into(), suppressed: false, hidden: false, kind: FeatureKind::Extrude(d) });
    let r = regen::run(&m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    (m, r)
}

fn out(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("freo_export_formats");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn b64decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c { b'A'..=b'Z' => c - b'A', b'a'..=b'z' => c - b'a' + 26, b'0'..=b'9' => c - b'0' + 52, b'+' => 62, _ => 63 } as u32;
    let mut out = vec![];
    for q in s.as_bytes().chunks(4) {
        let n = q.iter().take_while(|&&c| c != b'=').fold(0u32, |a, &c| a << 6 | val(c)) << (6 * q.iter().filter(|&&c| c == b'=').count());
        let pad = q.iter().filter(|&&c| c == b'=').count();
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    out
}

#[test]
fn all_formats_round_trip() {
    let (m, r) = block();
    let ntri = freo::app::export::triangles(&r).len();
    assert!(ntri >= 12);
    for f in Format::ALL {
        let p = out(&format!("block_{:?}.{}", f, f.ext()));
        write(&r, &m, f, &p).unwrap_or_else(|e| panic!("{:?}: {}", f, e));
        let bytes = std::fs::read(&p).unwrap();
        assert!(!bytes.is_empty(), "{:?} empty", f);
        match f {
            Format::Ply => {
                let head = String::from_utf8_lossy(&bytes[..bytes.windows(10).position(|w| w == b"end_header").unwrap() + 11]).to_string();
                let faces: usize = head.lines().find(|l| l.starts_with("element face")).unwrap().split(' ').nth(2).unwrap().parse().unwrap();
                let verts: usize = head.lines().find(|l| l.starts_with("element vertex")).unwrap().split(' ').nth(2).unwrap().parse().unwrap();
                assert_eq!(faces, ntri);
                assert_eq!(bytes.len(), head.len() + verts * 12 + faces * 13, "binary body size");
            }
            Format::Vrml => {
                let t = String::from_utf8(bytes).unwrap();
                assert!(t.starts_with("#VRML V2.0 utf8"));
                assert_eq!(t.matches(", -1,").count(), ntri);
            }
            Format::Gltf => {
                let j: serde_json::Value = serde_json::from_str(std::str::from_utf8(&bytes).unwrap()).unwrap();
                assert_eq!(j["asset"]["version"], "2.0");
                let uri = j["buffers"][0]["uri"].as_str().unwrap();
                let data = b64decode(uri.split_once(',').unwrap().1);
                assert_eq!(data.len() as u64, j["buffers"][0]["byteLength"].as_u64().unwrap());
                assert_eq!(j["accessors"][0]["count"].as_u64().unwrap() as usize, ntri * 3);
                // positions in metres: the 40 mm block spans 0.04
                let (x0, x1) = (j["accessors"][0]["min"][0].as_f64().unwrap(), j["accessors"][0]["max"][0].as_f64().unwrap());
                assert!(((x1 - x0) - 0.04).abs() < 1e-6, "{} {}", x0, x1);
            }
            Format::ThreeMf => {
                assert_eq!(&bytes[..4], b"PK\x03\x04");
                // macOS ships unzip: the package must test clean and contain the model part
                if let Ok(o) = std::process::Command::new("unzip").arg("-t").arg(&p).output() {
                    let txt = String::from_utf8_lossy(&o.stdout);
                    assert!(o.status.success() && txt.contains("No errors"), "{}", txt);
                    let model = std::process::Command::new("unzip").arg("-p").arg(&p).arg("3D/3dmodel.model").output().unwrap();
                    let x = String::from_utf8_lossy(&model.stdout);
                    assert!(x.contains("unit=\"millimeter\""));
                    assert_eq!(x.matches("<triangle ").count(), ntri);
                }
            }
            Format::Obj => assert!(String::from_utf8(bytes).unwrap().lines().any(|l| l.starts_with("f "))),
            Format::StlBinary => assert_eq!(bytes.len(), 84 + ntri * 50),
            Format::StlAscii => assert_eq!(String::from_utf8(bytes).unwrap().matches("facet normal").count(), ntri),
            Format::Step => assert!(String::from_utf8(bytes).unwrap().starts_with("ISO-10303-21;")),
        }
    }
    let _ = V3::ZERO;
}
