//! Publishing drawings from the sheet primitives: vector PDF (one page per sheet), SVG and
//! DXF (AutoCAD R12).

use super::drawing::{text_em, text_origin, Dash, Prim, Rgb};
use crate::kernel::math::*;

/// Helvetica advance widths (1/1000 em) for ASCII 32..=126.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556,
    278, 278, 584, 584, 584, 556, 1015,
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611,
    278, 278, 278, 469, 556, 333,
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500,
    334, 260, 334, 584,
];

/// Advance width of a character in Helvetica (1/1000 em).
pub fn char_width(c: char) -> f64 {
    let u = c as u32;
    if (32..=126).contains(&u) { return HELVETICA[(u - 32) as usize] as f64; }
    match c { '°' => 400.0, '⌀' | 'Ø' => 778.0, '±' => 584.0, '×' => 584.0, 'µ' => 556.0, 'Ä' => 667.0, 'Ö' => 778.0, 'Ü' => 722.0, 'ß' => 611.0, _ => 556.0 }
}

fn win_ansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '⌀' => 0xD8,
        '\u{A0}'..='\u{FF}' => c as u8, // Latin-1 matches WinAnsi here (°, ±, ×, µ, Ä, Ö, Ü, ä, ö, ü, ß…)
        _ => b'?',
    }
}

fn pdf_string(s: &str) -> String {
    let mut out = String::from("(");
    for c in s.chars() {
        let b = win_ansi(c);
        match b {
            b'(' | b')' | b'\\' => { out.push('\\'); out.push(b as char); }
            32..=126 => out.push(b as char),
            _ => out.push_str(&format!("\\{:03o}", b)),
        }
    }
    out.push(')');
    out
}

fn n(v: f64) -> String { let s = format!("{:.3}", v); let t = s.trim_end_matches('0').trim_end_matches('.'); if t == "-0" || t.is_empty() { "0".into() } else { t.to_string() } }

fn rgb(c: Rgb) -> (f64, f64, f64) { (c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0) }

/// Content stream of one sheet (coordinates in points, origin bottom-left).
fn pdf_page(prims: &[Prim]) -> String {
    let k = 72.0 / 25.4;
    let mut s = String::from("q 1 J 1 j\n");
    let dash = |d: Dash| -> String {
        let p = d.pattern();
        if p.is_empty() { "[] 0 d".into() } else { format!("[{}] 0 d", p.iter().map(|v| n(v * k)).collect::<Vec<_>>().join(" ")) }
    };
    for p in prims {
        match p {
            Prim::Line { pts, w, c, dash: d } => {
                if pts.len() < 2 { continue; }
                let (r, g, b) = rgb(*c);
                s.push_str(&format!("{} {} {} RG {} w {}\n", n(r), n(g), n(b), n((w * k).max(0.2)), dash(*d)));
                s.push_str(&format!("{} {} m", n(pts[0].x * k), n(pts[0].y * k)));
                for q in &pts[1..] { s.push_str(&format!(" {} {} l", n(q.x * k), n(q.y * k))); }
                s.push_str(" S\n");
            }
            Prim::Fill { pts, c } => {
                if pts.len() < 3 { continue; }
                let (r, g, b) = rgb(*c);
                s.push_str(&format!("{} {} {} rg {} {} m", n(r), n(g), n(b), n(pts[0].x * k), n(pts[0].y * k)));
                for q in &pts[1..] { s.push_str(&format!(" {} {} l", n(q.x * k), n(q.y * k))); }
                s.push_str(" f\n");
            }
            Prim::Circle { c, r, w, col, dash: d, fill } => {
                let (cr, cg, cb) = rgb(*col);
                let (cx, cy, rr) = (c.x * k, c.y * k, r * k);
                let m = 0.552_284_75 * rr;
                let path = format!(
                    "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c",
                    n(cx + rr), n(cy),
                    n(cx + rr), n(cy + m), n(cx + m), n(cy + rr), n(cx), n(cy + rr),
                    n(cx - m), n(cy + rr), n(cx - rr), n(cy + m), n(cx - rr), n(cy),
                    n(cx - rr), n(cy - m), n(cx - m), n(cy - rr), n(cx), n(cy - rr),
                    n(cx + m), n(cy - rr), n(cx + rr), n(cy - m), n(cx + rr), n(cy),
                );
                if *fill { s.push_str(&format!("{} {} {} rg {} f\n", n(cr), n(cg), n(cb), path)); }
                else { s.push_str(&format!("{} {} {} RG {} w {} {} S\n", n(cr), n(cg), n(cb), n((w * k).max(0.2)), dash(*d), path)); }
            }
            Prim::Text { pos, text, h, angle, halign, valign, c } => {
                if text.is_empty() { continue; }
                let o = text_origin(*pos, text, *h, *angle, *halign, *valign);
                let (cr, cg, cb) = rgb(*c);
                let (sn, cs) = angle.sin_cos();
                s.push_str(&format!("BT /F1 {} Tf {} {} {} rg {} {} {} {} {} {} Tm {} Tj ET\n", n(text_em(*h) * k), n(cr), n(cg), n(cb), n(cs), n(sn), n(-sn), n(cs), n(o.x * k), n(o.y * k), pdf_string(text)));
            }
        }
    }
    s.push_str("Q\n");
    s
}

/// A PDF document with one page per sheet: (width mm, height mm, primitives).
pub fn pdf(pages: &[(f64, f64, Vec<Prim>)], title: &str) -> Vec<u8> {
    let k = 72.0 / 25.4;
    let np = pages.len();
    // objects: 1 catalog, 2 pages, 3 font, 4 info, then page/content pairs
    let mut objs: Vec<String> = vec![];
    let kids: Vec<String> = (0..np).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
    objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), np));
    objs.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into());
    objs.push(format!("<< /Title {} /Producer (Freo) /Creator (Freo drawing) >>", pdf_string(title)));
    for (i, (w, h, prims)) in pages.iter().enumerate() {
        let content = pdf_page(prims);
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", n(w * k), n(h * k), 6 + 2 * i));
        objs.push(format!("<< /Length {} >>\nstream\n{}endstream", content.len(), content));
    }
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![];
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, o).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in &offsets { out.extend_from_slice(format!("{:010} 00000 n \n", o).as_bytes()); }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{}\n%%EOF\n", objs.len() + 1, xref).as_bytes());
    out
}

fn xml(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }

fn svg_colour(c: Rgb) -> String { format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]) }

/// One sheet as SVG (millimetre units).
pub fn svg(w: f64, h: f64, prims: &[Prim]) -> String {
    let mut s = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\">\n<rect width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n", n(w), n(h), n(w), n(h), n(w), n(h));
    let y = |v: f64| h - v;
    let pts = |p: &[V2]| p.iter().map(|q| format!("{},{}", n(q.x), n(y(q.y)))).collect::<Vec<_>>().join(" ");
    let dash = |d: Dash| { let p = d.pattern(); if p.is_empty() { String::new() } else { format!(" stroke-dasharray=\"{}\"", p.iter().map(|v| n(*v)).collect::<Vec<_>>().join(" ")) } };
    for p in prims {
        match p {
            Prim::Line { pts: l, w, c, dash: d } => s.push_str(&format!("<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"{}/>\n", pts(l), svg_colour(*c), n(w.max(0.1)), dash(*d))),
            Prim::Fill { pts: l, c } => s.push_str(&format!("<polygon points=\"{}\" fill=\"{}\"/>\n", pts(l), svg_colour(*c))),
            Prim::Circle { c, r, w, col, dash: d, fill } => {
                if *fill { s.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>\n", n(c.x), n(y(c.y)), n(*r), svg_colour(*col))); }
                else { s.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{}/>\n", n(c.x), n(y(c.y)), n(*r), svg_colour(*col), n(w.max(0.1)), dash(*d))); }
            }
            Prim::Text { pos, text, h: th, angle, halign, valign, c } => {
                if text.is_empty() { continue; }
                let o = text_origin(*pos, text, *th, *angle, *halign, *valign);
                s.push_str(&format!("<text x=\"{}\" y=\"{}\" font-family=\"Helvetica, Arial, sans-serif\" font-size=\"{}\" fill=\"{}\" transform=\"rotate({} {} {})\">{}</text>\n",
                    n(o.x), n(y(o.y)), n(text_em(*th)), svg_colour(*c), n(-angle.to_degrees()), n(o.x), n(y(o.y)), xml(text)));
            }
        }
    }
    s.push_str("</svg>\n");
    s
}

fn dxf_text(s: &str) -> String {
    let s = s.replace('⌀', "%%c").replace('Ø', "%%c").replace('°', "%%d").replace('±', "%%p");
    // R12 text is 8-bit; write other non-ASCII characters (umlauts, ß) as \U+XXXX escapes.
    s.chars().map(|c| if c.is_ascii() { c.to_string() } else { format!("\\U+{:04X}", c as u32) }).collect()
}

/// One sheet as an AutoCAD R12 DXF with layers per line type.
pub fn dxf(prims: &[Prim]) -> String {
    let mut s = String::new();
    let mut g = |code: i32, v: &str| { s.push_str(&format!("{}\n{}\n", code, v)); };
    g(0, "SECTION"); g(2, "HEADER"); g(9, "$ACADVER"); g(1, "AC1009"); g(9, "$INSUNITS"); g(70, "4"); g(0, "ENDSEC");
    g(0, "SECTION"); g(2, "TABLES");
    g(0, "TABLE"); g(2, "LTYPE"); g(70, "4");
    for (name, desc, pat) in [("CONTINUOUS", "Solid line", vec![]), ("HIDDEN", "Hidden __ __", vec![3.0, -1.5]), ("CENTER", "Center ____ _", vec![10.0, -1.5, 1.5, -1.5]), ("PHANTOM", "Phantom ____ _ _", vec![12.0, -1.5, 2.0, -1.5, 2.0, -1.5])] {
        g(0, "LTYPE"); g(2, name); g(70, "0"); g(3, desc); g(72, "65"); g(73, &pat.len().to_string());
        g(40, &n(pat.iter().map(|v: &f64| v.abs()).sum::<f64>()));
        for v in &pat { g(49, &n(*v)); }
    }
    g(0, "ENDTAB");
    g(0, "TABLE"); g(2, "LAYER"); g(70, "6");
    for (name, colour, lt) in [("VISIBLE", "7", "CONTINUOUS"), ("THIN", "7", "CONTINUOUS"), ("HIDDEN", "8", "HIDDEN"), ("CENTER", "1", "CENTER"), ("PHANTOM", "1", "PHANTOM"), ("TEXT", "7", "CONTINUOUS")] {
        g(0, "LAYER"); g(2, name); g(70, "0"); g(62, colour); g(6, lt);
    }
    g(0, "ENDTAB");
    g(0, "ENDSEC");
    g(0, "SECTION"); g(2, "ENTITIES");
    let layer = |d: Dash, w: f64| match d { Dash::Solid => if w >= 0.4 { "VISIBLE" } else { "THIN" }, Dash::Hidden => "HIDDEN", Dash::Center => "CENTER", Dash::Phantom => "PHANTOM" };
    for p in prims {
        match p {
            Prim::Line { pts, w, dash, .. } => {
                for wd in pts.windows(2) {
                    g(0, "LINE"); g(8, layer(*dash, *w));
                    g(10, &n(wd[0].x)); g(20, &n(wd[0].y)); g(30, "0");
                    g(11, &n(wd[1].x)); g(21, &n(wd[1].y)); g(31, "0");
                }
            }
            Prim::Fill { pts, .. } => {
                if pts.len() < 3 { continue; }
                let q = [pts[0], pts[1], pts[2], *pts.get(3).unwrap_or(&pts[2])];
                g(0, "SOLID"); g(8, "THIN");
                for (i, v) in q.iter().enumerate() { g(10 + i as i32, &n(v.x)); g(20 + i as i32, &n(v.y)); g(30 + i as i32, "0"); }
            }
            Prim::Circle { c, r, w, dash, .. } => {
                g(0, "CIRCLE"); g(8, layer(*dash, *w)); g(10, &n(c.x)); g(20, &n(c.y)); g(30, "0"); g(40, &n(*r));
            }
            Prim::Text { pos, text, h, angle, halign, valign, .. } => {
                if text.is_empty() { continue; }
                let o = text_origin(*pos, text, *h, *angle, *halign, *valign);
                g(0, "TEXT"); g(8, "TEXT"); g(10, &n(o.x)); g(20, &n(o.y)); g(30, "0"); g(40, &n(*h)); g(1, &dxf_text(text)); g(50, &n(angle.to_degrees()));
            }
        }
    }
    g(0, "ENDSEC");
    g(0, "EOF");
    s
}
