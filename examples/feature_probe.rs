//! Próbny odczyt numeru seryjnego (feature report) z każdego modułu – tylko odczyt.
fn main() {
    let api = hidapi::HidApi::new().unwrap();
    for d in api.device_list().filter(|d| d.vendor_id() == 0x2516) {
        let id = match d.product_id() { 0x01fe => 0x3a, 0x0206 => 0x4a, _ => 0x30 };
        let dev = d.open_device(&api).unwrap();
        for size in [0x20usize] {
            let mut raw = vec![0u8; size];
            raw[0] = id;
            let r = dev.get_feature_report(&mut raw);
            println!("{:04x} report {id:#04x} buf {size:#04x}: {}", d.product_id(), match r {
                Ok(n) => format!("OK {n} B: {}", String::from_utf8_lossy(&raw[2..2 + (raw[1] as usize).min(size - 2)])),
                Err(e) => format!("błąd {e}"),
            });
        }
    }
}
