//! Test diod LED modułów M0: baza czerwona, fader zielony, gałki niebieskie (tryb statyczny).
//! Nic nie jest zapisywane w pamięci modułu (save_fw = 0).

use hidapi::{HidApi, HidDevice};

fn send(dev: &HidDevice, bytes: &[u8]) {
    let mut raw = [0u8; 0x40];
    raw[..bytes.len()].copy_from_slice(bytes);
    match dev.write(&raw) {
        Ok(n) => println!("  {:02x?} -> {n} B", bytes),
        Err(e) => println!("  {:02x?} -> błąd: {e}", bytes),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;
    for info in api.device_list().filter(|d| d.vendor_id() == 0x2516) {
        let (name, module, rgb) = match info.product_id() {
            0x01f6 => ("baza", 0u8, [255u8, 0, 0]),
            0x0200 => ("fader", 1, [0, 255, 0]),
            0x0202 => ("gałki", 2, [0, 0, 255]),
            _ => continue,
        };
        let index = if module == 0 { 0 } else { 0xff };
        println!("{name} (moduł {module})");
        let dev = info.open_device(&api)?;
        // Tryb statyczny (0x2c), kolor (0x2d), jasność 100% (0x2e)
        send(&dev, &[0x02, 0x2c, module, index, 0x01, 0x00]);
        send(&dev, &[0x02, 0x2d, module, index, rgb[0], rgb[1], rgb[2], 0x00]);
        send(&dev, &[0x02, 0x2e, module, index, 100]);
    }
    Ok(())
}
