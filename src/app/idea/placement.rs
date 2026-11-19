//! Placement geometry in logical page coordinates, shared by the preview and commit.
use super::*;
use crate::sketch::palette::PaletteItem;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct PlacedImage {
    pub png: Arc<String>,
    pub min: [f32; 2],
    pub size: [f32; 2],
}

pub(super) fn import_image(path: &std::path::Path) -> Result<(Arc<String>, egui::ColorImage), String> {
    let reader = image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    use image::ImageDecoder;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > 2048 {
        image = image.thumbnail(2048, 2048);
    }
    let rgba = image.to_rgba8();
    let pixels = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
    let mut bytes = std::io::Cursor::new(Vec::new());
    rgba.write_to(&mut bytes, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok((Arc::new(STANDARD.encode(bytes.into_inner())), pixels))
}

pub(super) fn pixels(png: &str) -> Result<egui::ColorImage, String> {
    let bytes = STANDARD.decode(png).map_err(|e| e.to_string())?;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    reader.limits(limits);
    let rgba = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

pub(super) enum Placement {
    Circle,
    Palette(usize, usize),
    Image(Arc<String>, egui::TextureHandle),
}

pub(super) fn fit(center: egui::Vec2, aspect: egui::Vec2, extent: f32) -> egui::Rect {
    let size = aspect * extent.min(PAGE[0] / aspect.x).min(PAGE[1] / aspect.y);
    let page = vec2(PAGE[0], PAGE[1]);
    let size = size.min(page);
    let min = (center - size * 0.5).clamp(egui::Vec2::ZERO, page - size);
    egui::Rect::from_min_size(min.to_pos2(), size)
}

pub(super) fn palette_paths(item: &PaletteItem, at: egui::Vec2, extent: f32) -> Vec<Vec<[f32; 2]>> {
    let (lo, hi) = item.bounds(false);
    let span = (hi - lo).as_vec2();
    let bounds = fit(at, vec2(span.x, span.y).max(vec2(0.001, 0.001)) / span.max_element().max(0.001), extent);
    let k = bounds.size().max_elem() as f64 / item.extent();
    let center = (hi + lo) * 0.5;
    item.sketch
        .entities
        .iter()
        .filter(|e| !e.centerline && !e.construction)
        .map(|e| {
            item.sketch
                .sample(e, 48)
                .into_iter()
                .map(|p| {
                    let q = (p - center) * k;
                    [(bounds.center().x + q.x as f32).clamp(0., PAGE[0]), (bounds.center().y - q.y as f32).clamp(0., PAGE[1])]
                })
                .collect()
        })
        .collect()
}

pub(super) fn circle(at: egui::Vec2, radius: f32) -> Vec<[f32; 2]> {
    let radius = radius.min(at.x).min(at.y).min(PAGE[0] - at.x).min(PAGE[1] - at.y);
    (0..=128)
        .map(|i| {
            let angle = (i % 128) as f32 * std::f32::consts::TAU / 128.;
            [at.x + radius * angle.cos(), at.y + radius * angle.sin()]
        })
        .collect()
}
