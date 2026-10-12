//! Test komendy "obraz na cały ekran" (0x35) modułu 15xKey.
//! Użycie: screen_test <szerokość> <wysokość>
//! Wzór: siatka kafelków 72 px w różnych kolorach z białą ramką, czerwona ramka całości
//! i biała przekątna z lewego górnego rogu – pozwala ocenić skalę, przesunięcie i obrót.

#[path = "../src/hub.rs"]
#[allow(dead_code)]
mod hub;

use image::{ImageBuffer, Rgb, codecs::jpeg::JpegEncoder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<u32> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let (w, h) = (args.first().copied().unwrap_or(360), args.get(1).copied().unwrap_or(216));
    let palette: [[u8; 3]; 15] = [
        [230, 60, 60], [240, 150, 40], [230, 220, 50], [80, 200, 80], [50, 180, 220],
        [70, 90, 230], [160, 80, 220], [230, 80, 170], [140, 140, 140], [120, 70, 40],
        [0, 120, 100], [200, 200, 255], [255, 200, 200], [40, 40, 120], [120, 160, 0],
    ];
    let img = ImageBuffer::from_fn(w, h, |x, y| {
        if x < 3 || y < 3 || x >= w - 3 || y >= h - 3 {
            return Rgb([255, 0, 0]);
        }
        if (x as i64 * h as i64 - y as i64 * w as i64).abs() < (w.max(h) as i64 * 2) {
            return Rgb([255, 255, 255]);
        }
        let (tw, th) = (w / 5, h / 3);
        let (cx, cy) = (x / tw, y / th);
        if x % tw < 2 || y % th < 2 {
            return Rgb([255, 255, 255]);
        }
        Rgb(palette[((cy * 5 + cx) as usize) % palette.len()])
    });
    let mut jpeg = vec![];
    JpegEncoder::new_with_quality(&mut jpeg, 92).encode_image(&img)?;

    let api = hidapi::HidApi::new()?;
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == hub::VID && d.product_id() == 0x01fe)
        .ok_or("nie znaleziono 15xKey")?;
    let dev = info.open_device(&api)?;

    // Ten sam format co obraz na klawiszu, komenda 0x35 (Display_Picture_On_Screen_CM_M4315).
    const REPORT_LEN: usize = 0x140;
    const CHUNK_MAX: usize = REPORT_LEN - 8;
    for (i, chunk) in jpeg.chunks(CHUNK_MAX).enumerate() {
        let mut report = [0u8; REPORT_LEN];
        report[0] = 0x02;
        report[1] = 0x35;
        report[2..4].copy_from_slice(&(i as u16).to_le_bytes());
        report[4..6].copy_from_slice(&(chunk.len() as u16).to_le_bytes());
        report[7] = ((i + 1) * CHUNK_MAX >= jpeg.len()) as u8;
        report[8..8 + chunk.len()].copy_from_slice(chunk);
        dev.write(&report)?;
    }
    println!("Wysłano {w}x{h}, {} B", jpeg.len());
    Ok(())
}
