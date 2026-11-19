//! Single-line stroke font for silkscreen text (like KiCad's stroke font): every glyph is a
//! few polylines on a 4 × 6 grid (cap height 6), drawn with a round pen of 15 % of the
//! text height. Lower case is drawn as small capitals.

use super::geom::{p, rot, P};

fn glyph(c: char) -> &'static str {
    match c {
        'A' => "0,0 0,4 2,6 4,4 4,0;0,3 4,3",
        'B' => "0,0 0,6 3,6 4,5 4,4 3,3 0,3;3,3 4,2 4,1 3,0 0,0",
        'C' => "4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1",
        'D' => "0,0 0,6 3,6 4,5 4,1 3,0 0,0",
        'E' => "4,6 0,6 0,0 4,0;0,3 3,3",
        'F' => "4,6 0,6 0,0;0,3 3,3",
        'G' => "4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1 4,3 2,3",
        'H' => "0,0 0,6;4,0 4,6;0,3 4,3",
        'I' => "1,6 3,6;2,6 2,0;1,0 3,0",
        'J' => "4,6 4,1 3,0 1,0 0,1",
        'K' => "0,0 0,6;4,6 0,2;1,3 4,0",
        'L' => "0,6 0,0 4,0",
        'M' => "0,0 0,6 2,3 4,6 4,0",
        'N' => "0,0 0,6 4,0 4,6",
        'O' => "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0",
        'P' => "0,0 0,6 3,6 4,5 4,4 3,3 0,3",
        'Q' => "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0;2,2 4,0",
        'R' => "0,0 0,6 3,6 4,5 4,4 3,3 0,3;2,3 4,0",
        'S' => "4,5 3,6 1,6 0,5 0,4 1,3 3,3 4,2 4,1 3,0 1,0 0,1",
        'T' => "0,6 4,6;2,6 2,0",
        'U' => "0,6 0,1 1,0 3,0 4,1 4,6",
        'V' => "0,6 2,0 4,6",
        'W' => "0,6 1,0 2,3 3,0 4,6",
        'X' => "0,0 4,6;0,6 4,0",
        'Y' => "0,6 2,3 4,6;2,3 2,0",
        'Z' => "0,6 4,6 0,0 4,0",
        'Ä' => "0,0 0,4 2,6 4,4 4,0;0,3 4,3;1,7 1,7.4;3,7 3,7.4",
        'Ö' => "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0;1,7 1,7.4;3,7 3,7.4",
        'Ü' => "0,6 0,1 1,0 3,0 4,1 4,6;1,7 1,7.4;3,7 3,7.4",
        'ß' => "0,0 0,5 1,6 3,6 4,5 3,4 4,3 4,1 3,0 2,0",
        '0' => "1,0 0,1 0,5 1,6 3,6 4,5 4,1 3,0 1,0;0,1 4,5",
        '1' => "1,5 2,6 2,0;1,0 3,0",
        '2' => "0,5 1,6 3,6 4,5 4,4 0,0 4,0",
        '3' => "0,5 1,6 3,6 4,5 4,4 3,3 1,3;3,3 4,2 4,1 3,0 1,0 0,1",
        '4' => "3,0 3,6 0,2 4,2",
        '5' => "4,6 0,6 0,3 3,3 4,2 4,1 3,0 1,0 0,1",
        '6' => "4,5 3,6 1,6 0,5 0,1 1,0 3,0 4,1 4,2 3,3 0,3",
        '7' => "0,6 4,6 1,0",
        '8' => "1,3 0,4 0,5 1,6 3,6 4,5 4,4 3,3 1,3 0,2 0,1 1,0 3,0 4,1 4,2 3,3",
        '9' => "0,1 1,0 3,0 4,1 4,5 3,6 1,6 0,5 0,4 1,3 4,3",
        '-' => "1,3 3,3",
        '+' => "0,3 4,3;2,1 2,5",
        '.' => "1.8,0 2.2,0",
        ',' => "2,0.5 1.5,-1",
        ':' => "1.8,1 2.2,1;1.8,4 2.2,4",
        ';' => "1.8,4 2.2,4;2,0.5 1.5,-1",
        '/' => "0,0 4,6",
        '\\' => "0,6 4,0",
        '_' => "0,-0.5 4,-0.5",
        '(' => "3,6 1,4 1,2 3,0",
        ')' => "1,6 3,4 3,2 1,0",
        '[' => "3,6 1,6 1,0 3,0",
        ']' => "1,6 3,6 3,0 1,0",
        '=' => "0,2 4,2;0,4 4,4",
        '*' => "2,1 2,5;0,2 4,4;0,4 4,2",
        '#' => "1,0 1,6;3,0 3,6;0,2 4,2;0,4 4,4",
        '?' => "0,5 1,6 3,6 4,5 4,4 2,3 2,2;1.8,0 2.2,0",
        '!' => "2,6 2,2;1.8,0 2.2,0",
        '%' => "0,0 4,6;0,5 1,6;3,0 4,1",
        '°' => "1,5 2,6 3,5 2,4 1,5",
        '<' => "4,5 0,3 4,1",
        '>' => "0,5 4,3 0,1",
        '\'' => "2,6 2,4.5",
        '"' => "1,6 1,4.5;3,6 3,4.5",
        '~' => "0,3 1,4 3,2 4,3",
        'µ' => "0,-1 0,4;0,1 1,0 3,0 4,1;4,4 4,0",
        'Ω' => "0,0 1,0 0,2 0,5 1,6 3,6 4,5 4,2 3,0 4,0",
        ' ' => "",
        _ => "0,5 1,6 3,6 4,5 4,4 2,3 2,2;1.8,0 2.2,0",
    }
}

/// Strokes of `text`, `h` mm high, starting at the baseline point `at` (board coordinates,
/// y down), rotated by `angle` degrees and mirrored for the bottom side.
pub fn strokes(text: &str, at: P, h: f64, angle: f64, mirror: bool) -> Vec<Vec<P>> {
    let k = h / 6.0;
    let mut out = vec![];
    let mut x = 0.0;
    for ch in text.chars() {
        let (g, scale) = if ch.is_lowercase() { (ch.to_uppercase().next().unwrap_or(ch), 0.75) } else { (ch, 1.0) };
        let g = if ch == 'ß' { 'ß' } else { g };
        for stroke in glyph(g).split(';').filter(|s| !s.trim().is_empty()) {
            let pts: Vec<P> = stroke
                .split_whitespace()
                .filter_map(|xy| {
                    let (a, b) = xy.split_once(',')?;
                    Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?))
                })
                .map(|(gx, gy)| {
                    let lx = (x + gx * scale) * k;
                    let ly = -(gy * scale) * k;
                    let q = p(if mirror { -lx } else { lx }, ly);
                    at + rot(q, angle)
                })
                .collect();
            if !pts.is_empty() {
                out.push(pts);
            }
        }
        x += 5.0 * scale;
    }
    out
}

/// Width of `text` at height `h`.
pub fn width(text: &str, h: f64) -> f64 {
    let k = h / 6.0;
    let w: f64 = text.chars().map(|c| if c.is_lowercase() { 5.0 * 0.75 } else { 5.0 }).sum();
    (w - 1.0).max(0.0) * k
}

/// Pen width for text of height `h` (KiCad's default ratio).
pub fn pen(h: f64) -> f64 {
    (h * 0.15).max(0.1)
}

/// Strokes of `text` centred on `c`.
pub fn centered(text: &str, c: P, h: f64, angle: f64, mirror: bool) -> Vec<Vec<P>> {
    let w = width(text, h);
    let off = rot(p(if mirror { w / 2.0 } else { -w / 2.0 }, h / 2.0), angle);
    strokes(text, c + off, h, angle, mirror)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_has_the_right_size() {
        let s = centered("R10", p(0.0, 0.0), 1.0, 0.0, false);
        let (lo, hi) = super::super::geom::bbox(s.iter().flatten().copied()).unwrap();
        assert!((hi.y - lo.y - 1.0).abs() < 1e-9);
        assert!((hi.x - lo.x - width("R10", 1.0)).abs() < 0.2);
        assert!((lo.x + hi.x).abs() < 0.2 && (lo.y + hi.y).abs() < 1e-9);
    }
}
