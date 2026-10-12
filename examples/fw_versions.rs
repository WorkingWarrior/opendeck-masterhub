//! Odczyt wersji firmware'u modułów – te same raporty i rozmiar bufora (32 B) co oficjalne SDK.
fn main() {
    let api = hidapi::HidApi::new().unwrap();
    for d in api.device_list().filter(|d| d.vendor_id() == 0x2516) {
        let (name, report) = match d.product_id() {
            0x01f6 => ("Baza", 0x2e),
            0x0200 => ("Fader", 0x2e),
            0x0202 => ("3xKnob", 0x2e),
            0x0204 => ("2xRoller", 0x2e),
            0x01fe => ("15xKey", 0x38),
            0x0206 => ("Pokretlo", 0x48),
            _ => continue,
        };
        let dev = d.open_device(&api).unwrap();
        let mut raw = [0u8; 0x20];
        raw[0] = report;
        match dev.get_feature_report(&mut raw) {
            // Format: id, długość, 4 bajty nieznane, wersja tekstem.
            Ok(_) => {
                let len = (raw[1] as usize).min(30);
                let text: String = raw[2..2 + len].iter().skip(4).take_while(|b| **b != 0).map(|b| *b as char).collect();
                println!("{name:10} {text}");
            }
            Err(e) => println!("{name:10} błąd: {e}"),
        }
    }
}
