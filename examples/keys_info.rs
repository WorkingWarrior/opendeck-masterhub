//! Odczyt (tylko get_feature_report) raportów informacyjnych modułu 15xKey.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = hidapi::HidApi::new()?;
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == 0x2516 && d.product_id() == 0x01fe)
        .ok_or("nie znaleziono 15xKey")?;
    let dev = info.open_device(&api)?;
    for (id, name) in [(0x3au8, "Serial"), (0x37, "FW 37"), (0x39, "FW 39"), (0x38, "FW infos"), (0x3b, "Device infos"), (0x3c, "Backlight"), (0x3d, "Animation infos"), (0x3e, "Config infos")] {
        let mut raw = [0u8; 0x20];
        raw[0] = id;
        match dev.get_feature_report(&mut raw) {
            Ok(n) => {
                let len = raw[1] as usize;
                let data = &raw[2..(2 + len).min(n)];
                println!("{id:#04x} {name:16} len={len:3}  {:02x?}", data);
                println!("{:22}ascii: {}", "", String::from_utf8_lossy(data).replace(|c: char| !c.is_ascii_graphic(), "."));
            }
            Err(e) => println!("{id:#04x} {name:16} błąd: {e}"),
        }
    }
    Ok(())
}
