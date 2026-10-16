//! Rysowanie obrazów generowanych przez plugin: wskaźnik poziomu (nakładka na pokrętle) i zegar.
//!
//! Tekst rysujemy czcionką systemową (Windows: Segoe UI, Linux: DejaVu/Noto/Liberation,
//! macOS: Arial), żeby nie dołączać czcionek do pluginu.

use ab_glyph::{Font, FontArc, FontVec, PxScale, ScaleFont};
use image::{ImageBuffer, Rgb, RgbImage, imageops::FilterType};
use imageproc::drawing::draw_text_mut;
use std::{collections::{BTreeSet, HashMap}, sync::{LazyLock, Mutex}};

const BG: Rgb<u8> = Rgb([16, 16, 22]);
pub const TRACK: Rgb<u8> = Rgb([48, 48, 60]);
const ACCENT: Rgb<u8> = Rgb([124, 58, 237]);
const MUTED: Rgb<u8> = Rgb([200, 40, 40]);
const TEXT: Rgb<u8> = Rgb([240, 240, 245]);
const DIM_TEXT: Rgb<u8> = Rgb([160, 160, 175]);
/// Jaśniejszy odcień dla drobnego tekstu (data), żeby był czytelny na małym ekranie.
const SOFT_TEXT: Rgb<u8> = Rgb([205, 205, 220]);

/// Pierwsza czcionka z listy ścieżek, która istnieje i da się wczytać.
fn load_font(paths: &[String]) -> Option<FontArc> {
    let font = paths.iter().find_map(|p| FontArc::try_from_vec(std::fs::read(p).ok()?).ok());
    if font.is_none() {
        log::warn!("Nie znaleziono czcionki (szukano: {paths:?}) – tekst nie będzie rysowany");
    }
    font
}

#[cfg(windows)]
fn font_paths(bold: bool) -> Vec<String> {
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let files: &[&str] = if bold { &["segoeuib.ttf", "arialbd.ttf"] } else { &["segoeui.ttf", "arial.ttf"] };
    files.iter().map(|f| format!(r"{dir}\Fonts\{f}")).collect()
}

#[cfg(target_os = "linux")]
fn font_paths(bold: bool) -> Vec<String> {
    let style = if bold { "Bold" } else { "Regular" };
    let dejavu = if bold { "DejaVuSans-Bold" } else { "DejaVuSans" };
    [
        format!("/usr/share/fonts/truetype/dejavu/{dejavu}.ttf"),
        format!("/usr/share/fonts/TTF/{dejavu}.ttf"),
        format!("/usr/share/fonts/dejavu-sans-fonts/{dejavu}.ttf"),
        format!("/usr/share/fonts/truetype/noto/NotoSans-{style}.ttf"),
        format!("/usr/share/fonts/noto/NotoSans-{style}.ttf"),
        format!("/usr/share/fonts/google-noto/NotoSans-{style}.ttf"),
        format!("/usr/share/fonts/truetype/liberation/LiberationSans-{style}.ttf"),
        format!("/usr/share/fonts/liberation-sans/LiberationSans-{style}.ttf"),
    ]
    .into()
}

#[cfg(target_os = "macos")]
fn font_paths(bold: bool) -> Vec<String> {
    let file = if bold { "Arial Bold.ttf" } else { "Arial.ttf" };
    vec![
        format!("/System/Library/Fonts/Supplemental/{file}"),
        format!("/Library/Fonts/{file}"),
    ]
}

struct TextFont {
    primary: FontArc,
    bold: bool,
}

static BOLD: LazyLock<Option<TextFont>> = LazyLock::new(|| load_font(&font_paths(true)).map(|primary| TextFont { primary, bold: true }));
static REGULAR: LazyLock<Option<TextFont>> = LazyLock::new(|| load_font(&font_paths(false)).map(|primary| TextFont { primary, bold: false }));

// Baza zawiera metadane, a nie kopie wszystkich plików fontów. Inicjalizacja i odczyty
// plików odbywają się w istniejącym wątku renderowania / spawn_blocking panelu ustawień.
static FONT_DB: LazyLock<fontdb::Database> = LazyLock::new(|| {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    #[cfg(target_os = "linux")]
    if std::path::Path::new("/.flatpak-info").exists() {
        db.load_fonts_dir("/run/host/fonts");
        db.load_fonts_dir("/run/host/user-fonts");
    }
    db
});
static FONT_CACHE: LazyLock<Mutex<HashMap<fontdb::ID, Option<FontArc>>>> = LazyLock::new(Default::default);
static FONT_FAMILIES: LazyLock<Vec<String>> = LazyLock::new(|| {
    FONT_DB.faces().flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
        .collect::<BTreeSet<_>>().into_iter().collect()
});

pub fn font_families() -> Vec<String> { FONT_FAMILIES.clone() }

fn system_font(family: &str, bold: bool) -> Option<TextFont> {
    if family.is_empty() || family == "auto" { return None }
    let id = FONT_DB.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight: if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
        ..Default::default()
    })?;
    let mut cache = FONT_CACHE.lock().unwrap();
    let primary = if let Some(font) = cache.get(&id) { font.clone() } else {
        let font = FONT_DB.with_face_data(id, |data, index| {
            FontVec::try_from_vec_and_index(data.to_vec(), index).ok().map(FontArc::from)
        }).flatten();
        if cache.len() >= 8 { cache.clear(); }
        cache.insert(id, font.clone());
        font
    }?;
    Some(TextFont { primary, bold })
}

fn cjk_font(bold: bool) -> Option<FontArc> {
    let font = ["Noto Sans CJK JP", "Noto Sans JP", "Yu Gothic", "Meiryo", "Hiragino Sans", "Hiragino Kaku Gothic ProN", "MS Gothic", "IPA Gothic"]
        .into_iter().find_map(|family| system_font(family, bold)).map(|f| f.primary);
    if font.is_none() {
        log::warn!("Brak czcionki japońskiej; zainstaluj Noto Sans CJK JP i uruchom plugin ponownie");
    }
    font
}

static CJK_BOLD: LazyLock<Option<FontArc>> = LazyLock::new(|| cjk_font(true));
static CJK_REGULAR: LazyLock<Option<FontArc>> = LazyLock::new(|| cjk_font(false));

impl TextFont {
    fn glyph_font(&self, c: char) -> &FontArc {
        if self.primary.glyph_id(c).0 != 0 { return &self.primary }
        let fallback = if self.bold { &CJK_BOLD } else { &CJK_REGULAR };
        if let Some(font) = fallback.as_ref().filter(|font| font.glyph_id(c).0 != 0) { return font }
        let default = if self.bold { &BOLD } else { &REGULAR };
        if let Some(font) = default.as_ref().filter(|font| font.primary.glyph_id(c).0 != 0) { return &font.primary }
        &self.primary
    }

    fn scale(&self, font: &FontArc, size: f32) -> PxScale {
        // Ten sam rozmiar em w obu fontach: CJK nie może stać się miniaturowe
        // tylko dlatego, że ma inne metryki ascender/descender niż DejaVu.
        let em = size * self.primary.units_per_em().unwrap_or(1.0) / self.primary.height_unscaled();
        PxScale::from(em * font.height_unscaled() / font.units_per_em().unwrap_or(1.0))
    }
}

fn text_width(font: &TextFont, size: f32, text: &str) -> f32 {
    text.chars().map(|c| {
        let face = font.glyph_font(c);
        let scaled = face.as_scaled(font.scale(face, size));
        scaled.h_advance(scaled.glyph_id(c))
    }).sum()
}

/// Tekst wyśrodkowany poziomo; `cy` to środek wysokości tekstu. Zbyt szeroki tekst jest zmniejszany.
fn centered(img: &mut RgbImage, font: &TextFont, size: f32, max_width: f32, cy: f32, color: Rgb<u8>, text: &str) {
    let mut size = size;
    while size > 8.0 && text_width(font, size, text) > max_width {
        size -= 1.0;
    }
    let w = text_width(font, size, text);
    let scaled = font.primary.as_scaled(PxScale::from(size));
    let h = scaled.ascent() - scaled.descent();
    let x = (img.width() as f32 - w) / 2.0;
    let y = cy - h / 2.0;
    if text.chars().all(|c| font.primary.glyph_id(c).0 != 0) {
        draw_text_mut(img, color, x.round() as i32, y.round() as i32, PxScale::from(size), &font.primary, text);
    } else {
        let baseline = y + scaled.ascent();
        let mut x = x;
        for c in text.chars() {
            let face = font.glyph_font(c);
            let scale = font.scale(face, size);
            let scaled = face.as_scaled(scale);
            let mut buf = [0; 4];
            draw_text_mut(img, color, x.round() as i32, (baseline - scaled.ascent()).round() as i32, scale, face, c.encode_utf8(&mut buf));
            x += scaled.h_advance(scaled.glyph_id(c));
        }
    }
}

pub fn blend(a: Rgb<u8>, b: Rgb<u8>, t: f32) -> Rgb<u8> {
    let t = t.clamp(0.0, 1.0);
    Rgb([0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8))
}

/// Okrągły wskaźnik poziomu 0–1 z dużą wartością pośrodku i podpisem pod nią.
pub fn gauge(size: u32, level: f32, muted: bool, value_text: &str, label: &str) -> RgbImage {
    let (color, track) = if muted { (MUTED, blend(TRACK, MUTED, 0.35)) } else { (ACCENT, TRACK) };
    ring(size, if muted { 0.0 } else { level }, color, track, None, value_text, label)
}

/// Pierścień wypełniony w `level` (0–1, od góry zgodnie z ruchem wskazówek zegara), duża wartość
/// pośrodku, opcjonalny mały napis nad nią i podpis pod nią.
pub fn ring(size: u32, level: f32, color: Rgb<u8>, track: Rgb<u8>, top: Option<&str>, value_text: &str, label: &str) -> RgbImage {
    let mut img = ImageBuffer::from_pixel(size, size, BG);
    let c = size as f32 / 2.0;
    let (outer, width) = (c - size as f32 * 0.05, size as f32 * 0.08);
    let inner = outer - width;
    let level = level.clamp(0.0, 1.0);
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let r = (dx * dx + dy * dy).sqrt();
            // Wygładzone krawędzie pierścienia.
            let coverage = (r - inner + 0.5).clamp(0.0, 1.0) * (outer - r + 0.5).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let angle = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
            let ring = if angle <= level && level > 0.0 { color } else { track };
            img.put_pixel(x, y, blend(BG, ring, coverage));
        }
    }
    if let Some(font) = BOLD.as_ref() {
        centered(&mut img, font, size as f32 * 0.27, inner * 1.6, c - size as f32 * 0.03, TEXT, value_text);
    }
    if let Some(font) = REGULAR.as_ref() {
        centered(&mut img, font, size as f32 * 0.09, inner * 1.3, c + size as f32 * 0.19, DIM_TEXT, label);
    }
    if let (Some(top), Some(font)) = (top, BOLD.as_ref()) {
        centered(&mut img, font, size as f32 * 0.085, inner * 1.2, c - size as f32 * 0.24, SOFT_TEXT, top);
    }
    img
}

/// Zegar: godzina dużą czcionką, opcjonalnie data pod spodem.
pub fn clock(size: u32, time: &str, date: Option<&str>) -> RgbImage {
    let mut img = ImageBuffer::from_pixel(size, size, BG);
    let c = size as f32 / 2.0;
    // Na okrągłym ekranie pokrętła tekst musi zmieścić się w kole – stąd ~70% szerokości.
    let max_width = size as f32 * 0.72;
    let time_y = if date.is_some() { c - size as f32 * 0.06 } else { c };
    if let Some(font) = BOLD.as_ref() {
        centered(&mut img, font, size as f32 * 0.30, max_width, time_y, TEXT, time);
    }
    // Data pogrubioną czcionką i jaśniej – cienka szara była na ekranie nieczytelna.
    if let (Some(date), Some(font)) = (date, BOLD.as_ref()) {
        centered(&mut img, font, size as f32 * 0.115, max_width, c + size as f32 * 0.21, SOFT_TEXT, date);
    }
    img
}

pub struct NowPlayingView<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub paused: bool,
    pub progress: Option<f32>,
    pub art: Option<&'a RgbImage>,
    pub show_progress: bool,
    pub show_artist: bool,
}

fn fitted(text: &str, font: &TextFont, size: f32, width: f32) -> String {
    if text_width(font, size, text) <= width { return text.into() }
    let mut s = String::new();
    for ch in text.chars() {
        let candidate = format!("{s}{ch}…");
        if text_width(font, size, &candidate) > width { break }
        s.push(ch);
    }
    format!("{s}…")
}

/// Kompozycja korzysta z tego samego bufora RGB co zegar i nakładka. Wszystkie napisy
/// pokrętła mieszczą się wewnątrz koła. Okładka wypełnia Dial, a dolny gradient
/// zapewnia kontrast napisów; na 15xKey zostaje tylko czytelna miniatura.
pub fn now_playing(size: u32, view: Option<NowPlayingView<'_>>, no_player: &str, accent: Rgb<u8>, font_family: &str) -> RgbImage {
    let mut img = ImageBuffer::from_pixel(size, size, BG);
    let dial = size >= 200;
    let (art_size, art_y) = if dial { (106, 32) } else { (58, 7) };
    let art_x = (size - art_size) / 2;
    if let Some(art) = view.as_ref().and_then(|v| v.art) {
        if dial {
            // Centralne przycięcie typu cover wypełnia koło bez rozciągania okładki.
            let side = art.width().min(art.height());
            let square = image::imageops::crop_imm(art, (art.width() - side) / 2, (art.height() - side) / 2, side, side);
            img = if art.dimensions() == (size, size) {
                art.clone()
            } else {
                image::imageops::resize(&*square, size, size, FilterType::Lanczos3)
            };
            // Jasna okładka pozostaje jasna u góry; tylko obszar napisów dostaje cień.
            for y in 0..size {
                let shade = ((y as f32 / size as f32 - 0.44) / 0.22).clamp(0.0, 0.78);
                if shade > 0.0 {
                    for x in 0..size {
                        let pixel = *img.get_pixel(x, y);
                        img.put_pixel(x, y, blend(pixel, Rgb([0, 0, 0]), shade));
                    }
                }
            }
        } else {
            let scaled = image::imageops::resize(art, art_size, art_size, FilterType::Lanczos3);
            image::imageops::replace(&mut img, &scaled, art_x as i64, art_y as i64);
        }
    } else {
        let c = size as i32 / 2;
        let stem_x = c + if dial { 17 } else { 9 };
        let stem_top = if dial { 61 } else { 19 };
        let stem_bottom = if dial { 104 } else { 44 };
        let thickness = if dial { 6 } else { 4 };
        for y in stem_top..stem_bottom { for x in stem_x..stem_x + thickness { img.put_pixel(x as u32, y as u32, accent); } }
        for y in stem_bottom - thickness..stem_bottom + thickness { for x in stem_x - thickness * 2..stem_x + thickness {
            if ((x - (stem_x - thickness)) * (x - (stem_x - thickness)) + (y - stem_bottom) * (y - stem_bottom)) < thickness * thickness * 2 {
                img.put_pixel(x as u32, y as u32, accent);
            }
        } }
    }
    if let Some(v) = view.as_ref() {
        if v.paused {
            let c = size as i32 / 2;
            let y = if dial { 198 } else { 25 };
            let h = if dial { 14 } else { 12 };
            let w = if dial { 5 } else { 3 };
            for yy in y..y + h { for xx in [c - w - 3, c + 3] { for dx in 0..w { img.put_pixel((xx + dx) as u32, yy as u32, TEXT); } } }
        }
    }
    if dial {
        let custom_regular = system_font(font_family, false);
        let custom_bold = system_font(font_family, true);
        let regular = custom_regular.as_ref().or(REGULAR.as_ref());
        let bold = custom_bold.as_ref().or(BOLD.as_ref());
        if let Some(v) = view.as_ref() {
            if let Some(font) = regular {
                let title = if v.title.is_empty() { "—" } else { v.title };
                let width = if v.show_artist { 166.0 } else { 176.0 };
                let title = fitted(title, font, 18.0, width);
                centered(&mut img, font, 18.0, width, if v.show_artist { 178.0 } else { 166.0 }, TEXT, &title);
            }
            if v.show_artist {
                if let Some(font) = bold {
                    let artist = if v.artist.is_empty() { v.album } else { v.artist };
                    let artist = fitted(artist, font, 19.0, 184.0);
                    centered(&mut img, font, 19.0, 184.0, 151.0, accent, &artist);
                }
            }
            if v.show_progress {
                if let Some(progress) = v.progress {
                    let c = size as f32 / 2.0;
                    for y in 0..size { for x in 0..size {
                        let dx = x as f32 + 0.5 - c;
                        let dy = y as f32 + 0.5 - c;
                        let r = (dx * dx + dy * dy).sqrt();
                        let coverage = (r - 108.0 + 0.5).clamp(0.0, 1.0) * (113.0 - r + 0.5).clamp(0.0, 1.0);
                        if coverage > 0.0 {
                            let angle = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
                            let color = if progress > 0.0 && angle <= progress.clamp(0.0, 1.0) { accent } else { TRACK };
                            let pixel = *img.get_pixel(x, y);
                            img.put_pixel(x, y, blend(pixel, color, coverage));
                        }
                    } }
                }
            }
        } else if let Some(font) = bold {
            centered(&mut img, font, 17.0, 180.0, 168.0, SOFT_TEXT, no_player);
        }
    }
    img
}

/// PNG jako data URL (dla obrazów ustawianych przez akcje w OpenDecku).
pub fn png_data_url(img: &RgbImage) -> String {
    use base64::Engine;
    let mut buf = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut buf, image::ImageFormat::Png);
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
}

/// JPG gotowy do wysłania na moduł.
pub fn jpeg(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95).encode_image(img);
    out
}
