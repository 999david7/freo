//! The app icon. `assets/freo-icon.png` (1024 px, transparent, macOS icon grid) is the one
//! source; every size (window icon, .icns, .ico, Linux PNGs) is scaled from it.

const MASTER: &[u8] = include_bytes!("../assets/freo-icon.png");

/// The icon as `size`×`size` RGBA pixels.
pub fn render_icon(size: usize) -> Vec<u8> {
    let img = image::load_from_memory_with_format(MASTER, image::ImageFormat::Png).expect("embedded icon is a valid PNG").to_rgba8();
    let size = size.max(1) as u32;
    if img.width() == size && img.height() == size {
        return img.into_raw();
    }
    image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3).into_raw()
}
