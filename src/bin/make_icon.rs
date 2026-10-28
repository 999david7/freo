//! Renders the app icon: `make_icon <size> <out.png>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let size: usize = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let out = a.get(2).cloned().unwrap_or_else(|| "icon.png".into());
    let rgba = freo::icon::render_icon(size);
    std::fs::write(out, freo::softrender::encode_png_rgba(size, size, &rgba)).unwrap();
}
