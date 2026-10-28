//! 2D import for the sketcher (Importieren): DXF, IGES and Adobe Illustrator files, read into
//! lines, circles, arcs, splines, ellipses and points in sketch coordinates (mm).

use crate::kernel::math::*;

pub enum Curve2 { Line(V2, V2), Circle(V2, f64), Arc(V2, f64, f64, f64), Spline(Vec<V2>), Ellipse(V2, V2, f64), Point(V2) }

/// Minimal DXF reader for the ENTITIES section (2D, group codes 10/20, 11/21, 40-42, 70, 90).
pub fn parse_dxf(text: &str) -> Result<Vec<Curve2>, String> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).collect();
    if lines.len() < 2 { return Err("Keine DXF-Datei.".into()); }
    let pairs: Vec<(i32, &str)> = lines.chunks(2).filter(|c| c.len() == 2).filter_map(|c| c[0].parse::<i32>().ok().map(|k| (k, c[1]))).collect();
    let mut out = vec![];
    let mut i = 0;
    let f = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    // entity records: group 0 starts a record
    while i < pairs.len() {
        if pairs[i].0 != 0 { i += 1; continue; }
        let kind = pairs[i].1.to_uppercase();
        let mut j = i + 1;
        while j < pairs.len() && pairs[j].0 != 0 { j += 1; }
        let rec = &pairs[i + 1..j];
        let get = |code: i32| rec.iter().find(|p| p.0 == code).map(|p| f(p.1)).unwrap_or(0.0);
        match kind.as_str() {
            "LINE" => out.push(Curve2::Line(V2::new(get(10), get(20)), V2::new(get(11), get(21)))),
            "CIRCLE" => out.push(Curve2::Circle(V2::new(get(10), get(20)), get(40))),
            "ARC" => out.push(Curve2::Arc(V2::new(get(10), get(20)), get(40), get(50).to_radians(), get(51).to_radians())),
            "POINT" => out.push(Curve2::Point(V2::new(get(10), get(20)))),
            "ELLIPSE" => {
                let (c, a, ratio) = (V2::new(get(10), get(20)), V2::new(get(11), get(21)), get(40));
                out.push(Curve2::Ellipse(c, a, a.length() * ratio));
            }
            "SPLINE" => {
                let fit: Vec<V2> = rec.iter().filter(|p| p.0 == 11).zip(rec.iter().filter(|p| p.0 == 21)).map(|(x, y)| V2::new(f(x.1), f(y.1))).collect();
                let ctl: Vec<V2> = rec.iter().filter(|p| p.0 == 10).zip(rec.iter().filter(|p| p.0 == 20)).map(|(x, y)| V2::new(f(x.1), f(y.1))).collect();
                out.push(Curve2::Spline(if fit.len() >= 2 { fit } else { ctl }));
            }
            "LWPOLYLINE" => {
                let closed = (get(70) as i64) & 1 == 1;
                let mut verts: Vec<(V2, f64)> = vec![];
                for p in rec {
                    match p.0 { 10 => verts.push((V2::new(f(p.1), 0.0), 0.0)), 20 => if let Some(v) = verts.last_mut() { v.0.y = f(p.1) }, 42 => if let Some(v) = verts.last_mut() { v.1 = f(p.1) }, _ => {} }
                }
                poly_segments(&verts, closed, &mut out);
            }
            "POLYLINE" => {
                // vertices follow as VERTEX records until SEQEND
                let closed = (get(70) as i64) & 1 == 1;
                let mut verts: Vec<(V2, f64)> = vec![];
                let mut k = j;
                while k < pairs.len() && !(pairs[k].0 == 0 && pairs[k].1.eq_ignore_ascii_case("SEQEND")) {
                    if pairs[k].0 == 0 && pairs[k].1.eq_ignore_ascii_case("VERTEX") {
                        let mut m = k + 1;
                        let (mut x, mut y, mut b) = (0.0, 0.0, 0.0);
                        while m < pairs.len() && pairs[m].0 != 0 { match pairs[m].0 { 10 => x = f(pairs[m].1), 20 => y = f(pairs[m].1), 42 => b = f(pairs[m].1), _ => {} } m += 1; }
                        verts.push((V2::new(x, y), b));
                        k = m;
                    } else { k += 1; }
                }
                poly_segments(&verts, closed, &mut out);
                j = k;
            }
            _ => {}
        }
        i = j.max(i + 1);
    }
    Ok(out)
}

/// Polyline vertices with bulges (tan of a quarter of the arc angle) to lines and arcs.
fn poly_segments(v: &[(V2, f64)], closed: bool, out: &mut Vec<Curve2>) {
    let n = v.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let (p, b) = v[i];
        let q = v[(i + 1) % n].0;
        if b.abs() < 1e-12 { out.push(Curve2::Line(p, q)); continue; }
        let d = (q - p).length();
        if d < 1e-12 { continue; }
        let u = (q - p) / d;
        let c = (p + q) * 0.5 + perp_left(u) * (d * 0.5 * (1.0 - b * b) / (2.0 * b));
        let r = (p - c).length();
        let ang = |x: V2| (x.y - c.y).atan2(x.x - c.x);
        let (a0, a1) = if b > 0.0 { (ang(p), ang(q)) } else { (ang(q), ang(p)) };
        out.push(Curve2::Arc(c, r, a0, a1));
    }
}

// ---------------------------------------------------------------- IGES

/// IGES (fixed 80-column format): lines (110), circular arcs (100), conic arcs (104), copious
/// data (106), parametric splines (112), rational B-splines (126), points (116) and composite
/// curves (102), with transformation matrices (124); projected onto XY, converted to mm.
pub fn parse_iges(text: &str) -> Result<Vec<Curve2>, String> {
    let lines: Vec<&str> = text.lines().filter(|l| l.len() >= 73).collect();
    if lines.is_empty() { return Err("Keine IGES-Datei.".into()); }
    let sec = |l: &str| l.as_bytes()[72] as char;
    let global: String = lines.iter().filter(|l| sec(l) == 'G').map(|l| &l[..72]).collect();
    let (pd, rd) = {
        let g = global.as_bytes();
        let mut pd = ',';
        let mut rd = ';';
        if g.len() > 2 && g[0] == b'1' && g[1] == b'H' { pd = g[2] as char; }
        if let Some(p) = global.find(&format!("{}1H", pd)) { if let Some(c) = global[p + 3..].chars().next() { rd = c; } }
        (pd, rd)
    };
    let gfields: Vec<String> = split_params(&global, pd, rd);
    let unit = match gfields.get(13).and_then(|v| v.trim().parse::<i32>().ok()) { Some(1) => 25.4, Some(2) => 1.0, Some(3) => 25.4 / 12.0 * 144.0, Some(4) => 1609344.0, Some(5) => 1000.0, Some(6) => 1e6, Some(8) => 0.0254, Some(9) => 1e-3, Some(10) => 10.0, Some(11) => 0.0000254, _ => 1.0 };
    let unit = if gfields.get(13).and_then(|v| v.trim().parse::<i32>().ok()) == Some(3) { 304.8 } else { unit };
    // directory entries: two lines each
    let dlines: Vec<&str> = lines.iter().copied().filter(|l| sec(l) == 'D').collect();
    struct De { ty: i32, ptr: usize, xf: usize, sub: i32, form: i32 }
    let field = |l: &str, i: usize| l.get(i * 8..i * 8 + 8).unwrap_or("").trim().parse::<i64>().unwrap_or(0);
    let mut des: Vec<De> = vec![];
    for pair in dlines.chunks(2) {
        if pair.len() < 2 { break; }
        let status = pair[0].get(64..72).unwrap_or("").trim();
        let st = format!("{:0>8}", status);
        des.push(De { ty: field(pair[0], 0) as i32, ptr: field(pair[0], 1) as usize, xf: field(pair[0], 6).max(0) as usize, sub: st[2..4].parse().unwrap_or(0), form: field(pair[1], 4) as i32 });
    }
    // parameter data by directory pointer (odd sequence number of the DE)
    let mut pmap: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    for l in lines.iter().filter(|l| sec(l) == 'P') {
        let de: usize = l.get(64..72).unwrap_or("").trim().parse().unwrap_or(0);
        pmap.entry(de).or_default().push_str(&l[..64.min(l.len())]);
    }
    let params = |de_index: usize| -> Vec<f64> {
        let key = de_index * 2 + 1;
        pmap.get(&key).map(|t| split_params(t, pd, rd).iter().skip(1).map(|v| v.trim().replace('D', "E").parse::<f64>().unwrap_or(0.0)).collect()).unwrap_or_default()
    };
    let xform = |de_index: usize| -> Option<[f64; 12]> {
        let x = des.get(de_index)?.xf;
        if x == 0 { return None; }
        let p = params((x - 1) / 2);
        if p.len() < 12 { return None; }
        let mut m = [0.0; 12];
        m.copy_from_slice(&p[..12]);
        Some(m)
    };
    let mut out = vec![];
    fn emit(de: usize, des_ty: &dyn Fn(usize) -> (i32, i32), params: &dyn Fn(usize) -> Vec<f64>, xform: &dyn Fn(usize) -> Option<[f64; 12]>, unit: f64, out: &mut Vec<Curve2>, depth: usize) {
        if depth > 8 { return; }
        let (ty, form) = des_ty(de);
        let p = params(de);
        let m = xform(de);
        let tr = |x: f64, y: f64, z: f64| -> V2 {
            let (x, y) = match m { Some(m) => (m[0] * x + m[1] * y + m[2] * z + m[3], m[4] * x + m[5] * y + m[6] * z + m[7]), None => (x, y) };
            V2::new(x * unit, y * unit)
        };
        let g = |i: usize| p.get(i).copied().unwrap_or(0.0);
        match ty {
            110 => out.push(Curve2::Line(tr(g(0), g(1), g(2)), tr(g(3), g(4), g(5)))),
            116 => out.push(Curve2::Point(tr(g(0), g(1), g(2)))),
            100 => {
                let z = g(0);
                let (c, s, e) = (tr(g(1), g(2), z), tr(g(3), g(4), z), tr(g(5), g(6), z));
                let r = (s - c).length();
                if (s - e).length() < 1e-9 * r.max(1.0) { out.push(Curve2::Circle(c, r)); }
                else { out.push(Curve2::Arc(c, r, (s.y - c.y).atan2(s.x - c.x), (e.y - c.y).atan2(e.x - c.x))); }
            }
            104 => {
                // centred ellipse A x² + C y² + F = 0 (form 1); other conics sampled through their points
                let (a, c, f, z) = (g(0), g(2), g(5), g(6));
                let (s, e) = ((g(7), g(8)), (g(9), g(10)));
                if a.abs() > 1e-15 && c.abs() > 1e-15 && -f / a > 0.0 && -f / c > 0.0 {
                    let (ra, rb) = ((-f / a).sqrt(), (-f / c).sqrt());
                    let t0 = (s.1 / rb).atan2(s.0 / ra);
                    let mut t1 = (e.1 / rb).atan2(e.0 / ra);
                    if (s.0 - e.0).abs() < 1e-12 && (s.1 - e.1).abs() < 1e-12 && m.is_none() && ra >= rb {
                        out.push(Curve2::Ellipse(tr(0.0, 0.0, z), tr(ra, 0.0, z) - tr(0.0, 0.0, z), rb * unit));
                    } else {
                        if t1 <= t0 { t1 += std::f64::consts::TAU; }
                        let pts = (0..=24).map(|i| { let t = t0 + (t1 - t0) * i as f64 / 24.0; tr(ra * t.cos(), rb * t.sin(), z) }).collect();
                        out.push(Curve2::Spline(pts));
                    }
                } else {
                    out.push(Curve2::Line(tr(s.0, s.1, z), tr(e.0, e.1, z)));
                }
            }
            106 => {
                let ip = g(0) as i32;
                let n = g(1) as usize;
                let mut pts = vec![];
                for k in 0..n {
                    let q = match ip { 1 => tr(g(3 + 2 * k), g(4 + 2 * k), g(2)), 2 => tr(g(2 + 3 * k), g(3 + 3 * k), g(4 + 3 * k)), _ => tr(g(2 + 6 * k), g(3 + 6 * k), g(4 + 6 * k)) };
                    pts.push(q);
                }
                if matches!(form, 11 | 12 | 63) || form == 0 {
                    for w in pts.windows(2) { out.push(Curve2::Line(w[0], w[1])); }
                    if form == 63 && pts.len() > 2 { out.push(Curve2::Line(*pts.last().unwrap(), pts[0])); }
                } else { for q in pts { out.push(Curve2::Point(q)); } }
            }
            112 => {
                let n = g(3) as usize;
                let ts: Vec<f64> = (0..=n).map(|i| g(4 + i)).collect();
                let base = 5 + n;
                let mut pts = vec![];
                for seg in 0..n {
                    let c = |i: usize| g(base + seg * 12 + i);
                    for k in 0..8 {
                        let u = (ts[seg + 1] - ts[seg]) * k as f64 / 8.0;
                        let ev = |o: usize| c(o) + c(o + 1) * u + c(o + 2) * u * u + c(o + 3) * u * u * u;
                        pts.push(tr(ev(0), ev(4), ev(8)));
                    }
                }
                let c = |i: usize| g(base + (n - 1) * 12 + i);
                let u = ts[n] - ts[n - 1];
                let ev = |o: usize| c(o) + c(o + 1) * u + c(o + 2) * u * u + c(o + 3) * u * u * u;
                pts.push(tr(ev(0), ev(4), ev(8)));
                out.push(Curve2::Spline(pts));
            }
            126 => {
                let (k, deg) = (g(0) as usize, g(1) as usize);
                let nk = k + deg + 2;
                let knots: Vec<f64> = (0..nk).map(|i| g(6 + i)).collect();
                let wbase = 6 + nk;
                let w: Vec<f64> = (0..=k).map(|i| g(wbase + i)).collect();
                let cbase = wbase + k + 1;
                let ctl: Vec<(f64, f64, f64)> = (0..=k).map(|i| (g(cbase + 3 * i), g(cbase + 3 * i + 1), g(cbase + 3 * i + 2))).collect();
                let (v0, v1) = (g(cbase + 3 * (k + 1)), g(cbase + 3 * (k + 1) + 1));
                if deg == 1 {
                    for wn in ctl.windows(2) { out.push(Curve2::Line(tr(wn[0].0, wn[0].1, wn[0].2), tr(wn[1].0, wn[1].1, wn[1].2))); }
                } else {
                    let samples = 12 * (k + 1).max(2);
                    let pts = (0..=samples).map(|i| {
                        let t = v0 + (v1 - v0) * i as f64 / samples as f64;
                        let (x, y, z) = nurbs_point(&knots, &w, &ctl, deg, t);
                        tr(x, y, z)
                    }).collect();
                    out.push(Curve2::Spline(pts));
                }
            }
            102 => {
                let n = g(0) as usize;
                for i in 0..n {
                    let ptr = g(1 + i) as usize;
                    if ptr > 0 { emit((ptr - 1) / 2, des_ty, params, xform, unit, out, depth + 1); }
                }
            }
            _ => {}
        }
    }
    let des_ty = |i: usize| des.get(i).map(|d| (d.ty, d.form)).unwrap_or((0, 0));
    for (i, d) in des.iter().enumerate() {
        // only independent entities; members of composite curves come through 102
        if d.sub != 0 || d.ptr == 0 { continue; }
        emit(i, &des_ty, &params, &xform, unit, &mut out, 0);
    }
    Ok(out)
}

/// Splits IGES free-format parameters (Hollerith strings like 3HABC are kept whole).
fn split_params(t: &str, pd: char, rd: char) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == pd || c == rd {
            out.push(cur.trim().to_string());
            cur.clear();
            if c == rd { break; }
            i += 1;
            continue;
        }
        if c == 'H' && !cur.trim().is_empty() && cur.trim().chars().all(|d| d.is_ascii_digit()) {
            let n: usize = cur.trim().parse().unwrap_or(0);
            let s: String = chars[i + 1..(i + 1 + n).min(chars.len())].iter().collect();
            cur = s;
            i += 1 + n;
            continue;
        }
        cur.push(c);
        i += 1;
    }
    if !cur.trim().is_empty() { out.push(cur.trim().to_string()); }
    out
}

/// Point of a rational B-spline (de Boor on homogeneous coordinates).
fn nurbs_point(knots: &[f64], w: &[f64], ctl: &[(f64, f64, f64)], p: usize, t: f64) -> (f64, f64, f64) {
    let n = ctl.len() - 1;
    let mut k = p;
    while k < n && t >= knots[k + 1] { k += 1; }
    let mut d: Vec<[f64; 4]> = (0..=p).map(|j| { let i = j + k - p; let wi = w.get(i).copied().unwrap_or(1.0); [ctl[i].0 * wi, ctl[i].1 * wi, ctl[i].2 * wi, wi] }).collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + k - p;
            let den = knots[i + p + 1 - r] - knots[i];
            let a = if den.abs() < 1e-15 { 0.0 } else { (t - knots[i]) / den };
            for c in 0..4 { d[j][c] = (1.0 - a) * d[j - 1][c] + a * d[j][c]; }
        }
    }
    let h = d[p];
    let wq = if h[3].abs() < 1e-15 { 1.0 } else { h[3] };
    (h[0] / wq, h[1] / wq, h[2] / wq)
}

// ---------------------------------------------------------------- Adobe Illustrator

/// Adobe Illustrator: PDF-based files (content streams, Flate-compressed or plain) and older
/// PostScript-based files. Paths (m, l, c, v, y, h, re and the AI forms L, C, V, Y) become lines
/// and cubic segments; points are converted from pt to mm.
pub fn parse_ai(data: &[u8]) -> Result<Vec<Curve2>, String> {
    let mut out = vec![];
    let head = String::from_utf8_lossy(&data[..data.len().min(1024)]).to_string();
    if head.contains("%PDF") {
        let mut native = vec![];
        for (dict, body) in pdf_streams(data) {
            if dict.contains("/Image") || dict.contains("/Metadata") || dict.contains("/Length1") || dict.contains("/FontFile") || dict.contains("/DCTDecode") { continue; }
            let decoded = if dict.contains("/FlateDecode") { match miniz_oxide::inflate::decompress_to_vec_zlib(body) { Ok(d) => d, Err(_) => continue } } else { body.to_vec() };
            let txt = String::from_utf8_lossy(&decoded).to_string();
            if txt.contains("%%AI") || txt.contains("%AI") { native.push(txt); continue; }
            ps_paths(&txt, false, &mut out);
        }
        // only AI's own private data present (no page content): read that
        if out.is_empty() { for t in native { ps_paths(&t, true, &mut out); } }
    } else if head.starts_with("%!PS") || head.contains("Adobe Illustrator") {
        let txt = String::from_utf8_lossy(data).to_string();
        let body = txt.find("%%EndSetup").map(|i| &txt[i..]).unwrap_or(&txt);
        ps_paths(body, true, &mut out);
    } else {
        return Err("Keine Adobe-Illustrator-Datei.".into());
    }
    if out.is_empty() { return Err("Die Datei enthält keine Pfade.".into()); }
    Ok(out)
}

/// (dictionary text, raw stream bytes) of every PDF stream object.
fn pdf_streams(data: &[u8]) -> Vec<(String, &[u8])> {
    let mut out = vec![];
    let find = |hay: &[u8], needle: &[u8], from: usize| -> Option<usize> { hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from) };
    let mut pos = 0;
    while let Some(s) = find(data, b"stream", pos) {
        if s >= 3 && &data[s - 3..s] == b"end" { pos = s + 6; continue; }
        let mut start = s + 6;
        if data.get(start) == Some(&b'\r') { start += 1; }
        if data.get(start) == Some(&b'\n') { start += 1; }
        let Some(e) = find(data, b"endstream", start) else { break };
        let dict_start = data[..s].windows(2).rposition(|w| w == b"<<").unwrap_or(s.saturating_sub(400));
        let dict = String::from_utf8_lossy(&data[dict_start..s]).to_string();
        let mut end = e;
        while end > start && (data[end - 1] == b'\n' || data[end - 1] == b'\r') { end -= 1; }
        out.push((dict, &data[start..end]));
        pos = e + 9;
    }
    out
}

/// Path operators of PDF content / AI PostScript. `ai` also accepts Illustrator's uppercase forms.
fn ps_paths(txt: &str, ai: bool, out: &mut Vec<Curve2>) {
    const PT: f64 = 25.4 / 72.0;
    let mut stack: Vec<f64> = vec![];
    let mut ctm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut saved: Vec<[f64; 6]> = vec![];
    let mut cur = V2::ZERO;
    let mut start = V2::ZERO;
    let mut segs: Vec<Curve2> = vec![];
    let tf = |m: &[f64; 6], x: f64, y: f64| V2::new((m[0] * x + m[2] * y + m[4]) * PT, (m[1] * x + m[3] * y + m[5]) * PT);
    let bez = |a: V2, b: V2, c: V2, d: V2| -> Curve2 {
        Curve2::Spline((0..=6).map(|i| { let t = i as f64 / 6.0; let u = 1.0 - t; a * (u * u * u) + b * (3.0 * u * u * t) + c * (3.0 * u * t * t) + d * (t * t * t) }).collect())
    };
    let mut in_text = false;
    for tok in txt.split(|c: char| c.is_whitespace() || c == '[' || c == ']') {
        if tok.is_empty() { continue; }
        if let Ok(v) = tok.parse::<f64>() { stack.push(v); continue; }
        if tok == "BT" { in_text = true; } else if tok == "ET" { in_text = false; stack.clear(); continue; }
        if in_text { stack.clear(); continue; }
        let n = stack.len();
        let arg = |i: usize| stack[n - i];
        match tok {
            "q" => saved.push(ctm),
            "Q" => { if let Some(m) = saved.pop() { ctm = m; } }
            "cm" if n >= 6 => {
                let m = [arg(6), arg(5), arg(4), arg(3), arg(2), arg(1)];
                ctm = [m[0] * ctm[0] + m[1] * ctm[2], m[0] * ctm[1] + m[1] * ctm[3], m[2] * ctm[0] + m[3] * ctm[2], m[2] * ctm[1] + m[3] * ctm[3], m[4] * ctm[0] + m[5] * ctm[2] + ctm[4], m[4] * ctm[1] + m[5] * ctm[3] + ctm[5]];
            }
            "m" if n >= 2 => { cur = tf(&ctm, arg(2), arg(1)); start = cur; }
            "l" | "L" if n >= 2 && (tok == "l" || ai) => { let p = tf(&ctm, arg(2), arg(1)); if (p - cur).length() > 1e-9 { segs.push(Curve2::Line(cur, p)); } cur = p; }
            "c" | "C" if n >= 6 && (tok == "c" || ai) => { let (b, c, d) = (tf(&ctm, arg(6), arg(5)), tf(&ctm, arg(4), arg(3)), tf(&ctm, arg(2), arg(1))); segs.push(bez(cur, b, c, d)); cur = d; }
            "v" | "V" if n >= 4 && (tok == "v" || ai) => { let (c, d) = (tf(&ctm, arg(4), arg(3)), tf(&ctm, arg(2), arg(1))); segs.push(bez(cur, cur, c, d)); cur = d; }
            "y" | "Y" if n >= 4 && (tok == "y" || ai) => { let (b, d) = (tf(&ctm, arg(4), arg(3)), tf(&ctm, arg(2), arg(1))); segs.push(bez(cur, b, d, d)); cur = d; }
            "re" if n >= 4 => {
                let (x, y, w, h) = (arg(4), arg(3), arg(2), arg(1));
                let p = [tf(&ctm, x, y), tf(&ctm, x + w, y), tf(&ctm, x + w, y + h), tf(&ctm, x, y + h)];
                for i in 0..4 { segs.push(Curve2::Line(p[i], p[(i + 1) % 4])); }
                cur = p[0]; start = p[0];
            }
            "h" => { if (cur - start).length() > 1e-9 { segs.push(Curve2::Line(cur, start)); } cur = start; }
            // painting: fills and s/b close the path, S strokes it open, n discards it (clip paths)
            "s" | "b" | "b*" | "B" | "B*" | "f" | "F" | "f*" => { if (cur - start).length() > 1e-9 { segs.push(Curve2::Line(cur, start)); } cur = start; out.append(&mut segs); }
            "S" => out.append(&mut segs),
            "n" | "N" => segs.clear(),
            _ => {}
        }
        if !matches!(tok, "q" | "Q") { stack.clear(); }
    }
    out.append(&mut segs);
}
