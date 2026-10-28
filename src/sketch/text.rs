//! Outlines of sketch text: glyph contours of the bundled TrueType fonts, flattened to polylines.

use crate::kernel::math::*;
use ab_glyph::{Font, FontRef, OutlineCurve, Point};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Fonts that ship with Freo (always available).
pub const FONTS: [&str; 2] = ["Ubuntu Light", "Hack"];

fn font(i: u8) -> FontRef<'static> {
    let data = if i == 1 { epaint_default_fonts::HACK_REGULAR } else { epaint_default_fonts::UBUNTU_LIGHT };
    FontRef::try_from_slice(data).expect("bundled font")
}

/// TrueType / OpenType fonts installed on this Mac, by name (like Creo's font list).
pub fn system_fonts() -> &'static Vec<(String, PathBuf)> {
    static LIST: OnceLock<Vec<(String, PathBuf)>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut dirs = vec![PathBuf::from("/System/Library/Fonts"), PathBuf::from("/System/Library/Fonts/Supplemental"), PathBuf::from("/Library/Fonts")];
        if let Some(h) = std::env::var_os("HOME") { dirs.push(PathBuf::from(h).join("Library/Fonts")); }
        let mut out: Vec<(String, PathBuf)> = vec![];
        for d in dirs {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                if !matches!(ext.as_str(), "ttf" | "otf" | "ttc") { continue; }
                let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if !out.iter().any(|o| o.0 == name) { out.push((name, p)); }
            }
        }
        out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        out
    })
}

/// A system font by name; loaded once and kept for the session.
fn family_font(name: &str) -> Option<FontRef<'static>> {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static [u8]>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut c = cache.lock().ok()?;
    let data: &'static [u8] = match c.get(name) {
        Some(d) => d,
        None => {
            let path = &system_fonts().iter().find(|f| f.0 == name)?.1;
            let bytes: &'static [u8] = Box::leak(std::fs::read(path).ok()?.into_boxed_slice());
            c.insert(name.to_string(), bytes);
            bytes
        }
    };
    FontRef::try_from_slice_and_index(data, 0).ok()
}

/// Like [`outline`], with an installed font by name (falls back to the bundled font).
pub fn outline_family(text: &str, family: &str, font_idx: u8, aspect: f64, slant_deg: f64, a: V2, b: V2) -> Vec<Vec<V2>> {
    let f = if family.is_empty() { None } else { family_font(family) }.unwrap_or_else(|| font(font_idx));
    outline_with(&f, text, aspect, slant_deg, a, b)
}

/// Closed contours of `text`. `a` is the start point, `b` the height point: |b - a| is the
/// character (cap) height and a→b points "up" for the text, like Creo's text construction line.
pub fn outline(text: &str, font_idx: u8, aspect: f64, slant_deg: f64, a: V2, b: V2) -> Vec<Vec<V2>> {
    outline_with(&font(font_idx), text, aspect, slant_deg, a, b)
}

fn outline_with(f: &FontRef<'static>, text: &str, aspect: f64, slant_deg: f64, a: V2, b: V2) -> Vec<Vec<V2>> {
    let h = (b - a).length();
    if h < 1e-9 || text.is_empty() { return vec![]; }
    let cap = f.glyph_id('H');
    let cap_h = f.outline(cap).map(|o| (o.bounds.max.y - o.bounds.min.y).abs() as f64).filter(|v| *v > 1.0).unwrap_or(f.units_per_em().unwrap_or(1000.0) as f64 * 0.7);
    let s = h / cap_h;
    let v = (b - a) / h;
    let u = V2::new(v.y, -v.x);
    let shear = slant_deg.to_radians().tan();
    let aspect = if aspect > 1e-6 { aspect } else { 1.0 };
    let map = |p: Point, pen: f64| -> V2 {
        let (x, y) = (p.x as f64, p.y as f64);
        let xl = (pen + x * aspect + y * shear) * s;
        a + u * xl + v * (y * s)
    };
    let mut out: Vec<Vec<V2>> = vec![];
    let mut pen = 0.0f64;
    let mut prev = None;
    for ch in text.chars() {
        let id = f.glyph_id(ch);
        if let Some(p) = prev { pen += f.kern_unscaled(p, id) as f64 * aspect; }
        if let Some(o) = f.outline(id) {
            let mut cur: Vec<V2> = vec![];
            let mut last: Option<Point> = None;
            for c in &o.curves {
                let (start, pts): (Point, Vec<Point>) = match *c {
                    OutlineCurve::Line(p0, p1) => (p0, vec![p1]),
                    OutlineCurve::Quad(p0, p1, p2) => (p0, (1..=6).map(|i| { let t = i as f32 / 6.0; let m = 1.0 - t; Point { x: m * m * p0.x + 2.0 * m * t * p1.x + t * t * p2.x, y: m * m * p0.y + 2.0 * m * t * p1.y + t * t * p2.y } }).collect()),
                    OutlineCurve::Cubic(p0, p1, p2, p3) => (p0, (1..=8).map(|i| { let t = i as f32 / 8.0; let m = 1.0 - t; Point { x: m * m * m * p0.x + 3.0 * m * m * t * p1.x + 3.0 * m * t * t * p2.x + t * t * t * p3.x, y: m * m * m * p0.y + 3.0 * m * m * t * p1.y + 3.0 * m * t * t * p2.y + t * t * t * p3.y } }).collect()),
                };
                let joined = last.is_some_and(|l| (l.x - start.x).abs() < 1e-3 && (l.y - start.y).abs() < 1e-3);
                if !joined {
                    if cur.len() > 2 { out.push(std::mem::take(&mut cur)); } else { cur.clear(); }
                    cur.push(map(start, pen));
                }
                for p in &pts { cur.push(map(*p, pen)); }
                last = pts.last().copied();
            }
            if cur.len() > 2 { out.push(cur); }
        }
        pen += f.h_advance_unscaled(id) as f64 * aspect;
        prev = Some(id);
    }
    // drop duplicated closing points
    for l in &mut out { if l.len() > 2 && (l[0] - l[l.len() - 1]).length() < 1e-9 * h { l.pop(); } }
    out
}
