//! Test animacji: generuje GIF (obracający się klin, 24 klatki) i odtwarza go 6 s na pokrętle,
//! mierząc rzeczywistą liczbę klatek na sekundę.

#[path = "../src/hub.rs"]
#[allow(dead_code)]
mod hub;

use image::{
    AnimationDecoder, Delay, DynamicImage, Frame, ImageBuffer, Rgba,
    codecs::{gif::{GifDecoder, GifEncoder, Repeat}, jpeg::JpegEncoder},
};
use std::{io::Cursor, time::{Duration, Instant}};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Generujemy GIF w pamięci.
    let mut gif = vec![];
    {
        let mut enc = GifEncoder::new(&mut gif);
        enc.set_repeat(Repeat::Infinite)?;
        for i in 0..24 {
            let angle = i as f32 / 24.0 * std::f32::consts::TAU;
            let img = ImageBuffer::from_fn(120, 120, |x, y| {
                let (dx, dy) = (x as f32 - 60.0, y as f32 - 60.0);
                let a = dy.atan2(dx);
                let diff = ((a - angle).rem_euclid(std::f32::consts::TAU)) < 0.8;
                if diff && dx * dx + dy * dy < 3600.0 { Rgba([255, 120 + i * 5, 0, 255]) } else { Rgba([20, 20, 60, 255]) }
            });
            enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(40, 1)))?;
        }
    }
    println!("GIF: {} B", gif.len());

    // 2. Dekodujemy i kodujemy klatki tak jak plugin.
    let frames: Vec<Vec<u8>> = GifDecoder::new(Cursor::new(gif))?
        .into_frames()
        .map(|f| {
            let img = DynamicImage::ImageRgba8(f.unwrap().into_buffer()).resize_to_fill(240, 240, image::imageops::FilterType::Triangle);
            let mut out = vec![];
            JpegEncoder::new_with_quality(&mut out, 85).encode_image(&img.to_rgb8()).unwrap();
            out
        })
        .collect();
    println!("Klatek: {}, średnio {} B", frames.len(), frames.iter().map(Vec::len).sum::<usize>() / frames.len());

    // 3. Odtwarzamy na pokrętle tak szybko, jak pozwala minimalne opóźnienie 66 ms.
    let api = hidapi::HidApi::new()?;
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == hub::VID && hub::Kind::from_pid(d.product_id()) == Some(hub::Kind::Dial))
        .ok_or("nie znaleziono pokrętła")?;
    let dev = info.open_device(&api)?;
    let start = Instant::now();
    let mut shown = 0;
    let mut upload_time = Duration::ZERO;
    while start.elapsed() < Duration::from_secs(6) {
        for f in &frames {
            let t = Instant::now();
            hub::upload_dial_image(&dev, f)?;
            upload_time += t.elapsed();
            shown += 1;
            std::thread::sleep(Duration::from_millis(66).saturating_sub(t.elapsed()));
        }
    }
    println!(
        "Wyświetlono {shown} klatek w {:.1} s ({:.1} kl./s), średni czas wysyłki {:.1} ms",
        start.elapsed().as_secs_f32(),
        shown as f32 / start.elapsed().as_secs_f32(),
        upload_time.as_secs_f32() * 1000.0 / shown as f32
    );
    Ok(())
}
