//! Test ekranu pokrętła: rysuje literę "F" (widać obrót/odbicie) i wysyła na moduł 1xDial.

#[path = "../src/hub.rs"]
#[allow(dead_code)]
mod hub;

use image::{ImageBuffer, Rgb, codecs::jpeg::JpegEncoder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = hidapi::HidApi::new()?;
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == hub::VID && hub::Kind::from_pid(d.product_id()) == Some(hub::Kind::Dial))
        .ok_or("nie znaleziono pokrętła")?;
    let dev = info.open_device(&api)?;

    // Tło: gradient czerwony (poziomo) / niebieski (pionowo); białe "F" w środku.
    let img = ImageBuffer::from_fn(240, 240, |x, y| {
        let f = (80..100).contains(&x) && (60..180).contains(&y)
            || (80..160).contains(&x) && (60..80).contains(&y)
            || (80..140).contains(&x) && (110..130).contains(&y);
        if f { Rgb([255, 255, 255]) } else { Rgb([x as u8, 40, y as u8]) }
    });
    let mut jpeg = vec![];
    JpegEncoder::new_with_quality(&mut jpeg, 95).encode_image(&img)?;

    hub::set_sw_mode(&dev, hub::Kind::Dial, true)?;
    hub::upload_dial_image(&dev, &jpeg)?;
    println!("Wysłano {} bajtów", jpeg.len());
    Ok(())
}
