//! Developer check: imports every STEP file in a folder and reports mesh quality.
//! cargo run --release --example stepcheck -- <folder>
use freo::kernel::math::*;
use freo::model::step_import::*;
use std::collections::HashMap;

fn main() {
    let dir = std::env::args().nth(1).expect("folder");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    files.sort();
    for p in files {
        let t0 = std::time::Instant::now();
        let text = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).to_string();
        match import(&text) {
            Err(e) => println!("{:40} ERROR {}", p.file_name().unwrap().to_string_lossy(), e),
            Ok(r) => {
                let mut vol = 0.0;
                let mut ntri = 0;
                let mut edges: HashMap<((i64, i64, i64), (i64, i64, i64)), i32> = HashMap::new();
                let key = |v: V3| ((v.x * 1e4).round() as i64, (v.y * 1e4).round() as i64, (v.z * 1e4).round() as i64);
                let mut bb = BBox::default();
                for f in &r.faces {
                    for t in &f.tris {
                        let (a, b, c) = (f.verts[t[0] as usize].0, f.verts[t[1] as usize].0, f.verts[t[2] as usize].0);
                        vol += a.dot(b.cross(c)) / 6.0;
                        ntri += 1;
                        for (x, y) in [(a, b), (b, c), (c, a)] { bb.add(x); let (kx, ky) = (key(x), key(y)); *edges.entry(if kx < ky { (kx, ky) } else { (ky, kx) }).or_default() += if kx < ky { 1 } else { -1 }; }
                    }
                }
                let open = edges.values().filter(|v| **v != 0).count();
                if std::env::var("FACES").is_ok() {
                    // per face: boundary edges not matched by any other face
                    for (fi, f) in r.faces.iter().enumerate() {
                        let mut bad = 0;
                        for t in &f.tris { for k in 0..3 { let (x, y) = (key(f.verts[t[k] as usize].0), key(f.verts[t[(k + 1) % 3] as usize].0)); let kk = if x < y { (x, y) } else { (y, x) }; if edges.get(&kk).map(|v| *v != 0).unwrap_or(false) { bad += 1; } } }
                        if bad > 0 || f.tris.len() > 2000 { println!("   face {:3} {:8} tris {:6} open {:5} projerr {:.4}", fi, f.kind, f.tris.len(), bad, f.proj_err); }
                    }
                }
                println!("{:40} {:>5} faces {:>6} tris skipped {:>3} solids {:>3} unit {:4} vol {:>14.3} open-edges {:>5} size {:?} {:.0}ms", p.file_name().unwrap().to_string_lossy(), r.faces.len(), ntri, r.skipped, r.solids, r.unit, vol, open, bb.size(), t0.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }
}
