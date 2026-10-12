//! Konwersja obrazków z OpenDecka (data URL) na JPG dla klawiszy MasterHUB.

use base64::Engine;
use image::{DynamicImage, ImageBuffer, Rgb, codecs::jpeg::JpegEncoder, imageops::FilterType};

/// Zwraca kwadratowy JPG `size`×`size` gotowy do wysłania; `None` daje czarny obraz.
pub fn to_jpeg(data_url: Option<&str>, size: u32) -> Result<Vec<u8>, String> {
    let rgb = match data_url {
        None => ImageBuffer::from_pixel(size, size, Rgb([0, 0, 0])),
        Some(url) => {
            let (header, data) = url.split_once(',').ok_or("niepoprawny data URL")?;
            if !header.ends_with(";base64") {
                return Err(format!("nieobsługiwany format obrazka: {header}"));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .map_err(|e| e.to_string())?;
            let img = image::load_from_memory(&bytes).map_err(|e| format!("{header}: {e}"))?;
            flatten(img.resize_exact(size, size, FilterType::Triangle))
        }
    };
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Skaluje obraz do kwadratu `size`×`size`: `cover` przycina do wypełnienia, inaczej mieści
/// całość na czarnym tle. Zwraca JPG.
pub fn fit_jpeg(img: &DynamicImage, size: u32, cover: bool, quality: u8) -> Result<Vec<u8>, String> {
    let scaled = if cover {
        img.resize_to_fill(size, size, FilterType::Triangle)
    } else {
        img.resize(size, size, FilterType::Triangle)
    };
    let mut canvas = ImageBuffer::from_pixel(size, size, Rgb([0, 0, 0]));
    let flat = flatten(scaled);
    let (x, y) = ((size - flat.width()) / 2, (size - flat.height()) / 2);
    image::imageops::overlay(&mut canvas, &flat, x as i64, y as i64);
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&canvas)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Nakłada przezroczystość na czarne tło.
fn flatten(img: DynamicImage) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let rgba = img.to_rgba8();
    ImageBuffer::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y).0;
        let a = p[3] as u16;
        Rgb([0, 1, 2].map(|c| (p[c] as u16 * a / 255) as u8))
    })
}
