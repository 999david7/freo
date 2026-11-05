//! Headless software renderer (z-buffered, shaded faces + model edges) writing PNG files.
//! Used for thumbnails and kernel regression images.

use crate::kernel::math::*;
use crate::model::regen::RegenResult;

pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<[u8; 3]>,
}

/// Isometric-ish view direction (camera at +X +Y +Z).
pub fn render(r: &RegenResult, w: usize, h: usize, color: [f32; 3]) -> Image {
    let topo = &r.topo;
    let mut img = Image { w, h, rgb: vec![[0; 3]; w * h] };
    for y in 0..h {
        let t = y as f32 / h as f32;
        let c = [(0.98 - 0.18 * t) * 255.0, (0.99 - 0.14 * t) * 255.0, 1.0 * 255.0 - 30.0 * t];
        for x in 0..w { img.rgb[y * w + x] = [c[0] as u8, c[1] as u8, c[2] as u8]; }
    }
    let view = safe_norm(V3::new(1.0, 0.8, 1.2), V3::Z);
    let right = safe_norm(V3::Y.cross(view), V3::X);
    let up = view.cross(right);
    let b = if topo.bbox.is_empty() { let mut b = crate::kernel::math::BBox::default(); b.add(V3::splat(-50.0)); b.add(V3::splat(50.0)); b } else { topo.bbox };
    let c = b.center();
    let scale = 0.8 * (w.min(h) as f64) / b.diagonal().max(1e-6);
    let proj = |p: V3| -> (f64, f64, f64) {
        let d = p - c;
        (w as f64 / 2.0 + d.dot(right) * scale, h as f64 / 2.0 - d.dot(up) * scale, d.dot(view))
    };
    let mut z = vec![f64::NEG_INFINITY; w * h];
    let light = safe_norm(view + up * 0.6 + right * 0.3, view);
    for (pi, poly) in topo.polys.iter().enumerate() {
        let pts: Vec<(f64, f64, f64)> = poly.iter().map(|&i| proj(topo.positions[i])).collect();
        let n = topo.planes[pi].n;
        let shade = (0.35 + 0.65 * n.dot(light).max(0.0) + 0.25 * n.dot(view).max(0.0).powi(16)) as f32;
        let col = [(color[0] * shade).min(1.0), (color[1] * shade).min(1.0), (color[2] * shade).min(1.0)];
        for k in 1..pts.len() - 1 { tri(&mut img, &mut z, pts[0], pts[k], pts[k + 1], col); }
    }
    let span = b.diagonal() * 1e-3;
    for &(a, bb) in &topo.draw_edges {
        let (pa, pb) = (proj(topo.positions[a]), proj(topo.positions[bb]));
        line(&mut img, &z, pa, pb, span, [20, 20, 30]);
    }
    img
}

fn tri(img: &mut Image, z: &mut [f64], a: (f64, f64, f64), b: (f64, f64, f64), c: (f64, f64, f64), col: [f32; 3]) {
    let minx = a.0.min(b.0).min(c.0).floor().max(0.0) as usize;
    let maxx = (a.0.max(b.0).max(c.0).ceil() as usize).min(img.w - 1);
    let miny = a.1.min(b.1).min(c.1).floor().max(0.0) as usize;
    let maxy = (a.1.max(b.1).max(c.1).ceil() as usize).min(img.h - 1);
    let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    if area.abs() < 1e-12 { return; }
    let rgb = [(col[0] * 255.0) as u8, (col[1] * 255.0) as u8, (col[2] * 255.0) as u8];
    for y in miny..=maxy {
        for x in minx..=maxx {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) / area;
            let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 { continue; }
            let d = w0 * a.2 + w1 * b.2 + w2 * c.2;
            let i = y * img.w + x;
            if d > z[i] { z[i] = d; img.rgb[i] = rgb; }
        }
    }
}

fn line(img: &mut Image, z: &[f64], a: (f64, f64, f64), b: (f64, f64, f64), bias: f64, col: [u8; 3]) {
    let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil() as usize).max(1);
    for s in 0..=steps {
        let t = s as f64 / steps as f64;
        let (x, y, d) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
        if x < 0.0 || y < 0.0 { continue; }
        let (xi, yi) = (x as usize, y as usize);
        if xi >= img.w || yi >= img.h { continue; }
        let i = yi * img.w + xi;
        if d + bias * 3.0 >= z[i] { img.rgb[i] = col; }
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 { c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 }; }
    }
    !c
}

/// Encode as PNG (zlib-compressed with miniz_oxide).
pub fn encode_png(img: &Image) -> Vec<u8> {
    let rgb: Vec<u8> = img.rgb.iter().flatten().copied().collect();
    encode_png_raw(img.w, img.h, &rgb, 3)
}

/// PNG of 8-bit RGBA pixels.
pub fn encode_png_rgba(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> { encode_png_raw(w, h, rgba, 4) }

fn encode_png_raw(w: usize, h: usize, px: &[u8], channels: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity(h * (w * channels + 1));
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&px[y * w * channels..(y + 1) * w * channels]);
    }
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |ty: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = vec![];
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, if channels == 4 { 6 } else { 2 }, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}
