//! Lista interfejsów HID modułów MasterHUB.
fn main() {
    let api = hidapi::HidApi::new().unwrap();
    for d in api.device_list().filter(|d| d.vendor_id() == 0x2516) {
        println!("{:04x} if={} page={:04x} usage={:04x} sn={:?}\n     {}", d.product_id(), d.interface_number(), d.usage_page(), d.usage(), d.serial_number(), d.path().to_string_lossy());
    }
}
