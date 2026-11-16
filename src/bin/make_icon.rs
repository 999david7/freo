//! Renders the app icon: `make_icon <size> <out.png>`, or `make_icon ico <out.ico>` for a
//! Windows icon holding 16 to 256 px images.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let out = a.get(2).cloned().unwrap_or_else(|| "icon.png".into());
    if a.get(1).map(String::as_str) == Some("ico") {
        std::fs::write(out, ico(&[16, 24, 32, 48, 64, 128, 256])).unwrap();
        return;
    }
    let size: usize = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let rgba = freo::icon::render_icon(size);
    std::fs::write(out, freo::softrender::encode_png_rgba(size, size, &rgba)).unwrap();
}

/// An .ico file whose images are stored as PNG (supported since Windows Vista).
fn ico(sizes: &[usize]) -> Vec<u8> {
    let pngs: Vec<Vec<u8>> = sizes.iter().map(|&s| freo::softrender::encode_png_rgba(s, s, &freo::icon::render_icon(s))).collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (s, png) in sizes.iter().zip(&pngs) {
        // width and height 0 mean 256
        let d = if *s >= 256 { 0 } else { *s as u8 };
        out.extend([d, d, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((png.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += png.len();
    }
    for png in &pngs {
        out.extend(png);
    }
    out
}
