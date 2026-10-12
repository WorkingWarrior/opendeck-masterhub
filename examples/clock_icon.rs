//! Ikona akcji "Zegar".
#[path = "../src/render.rs"]
#[allow(dead_code)]
mod render;

fn main() {
    let dir = std::env::args().nth(1).expect("katalog");
    render::clock(144, "12:00", None).save(format!("{dir}/clock@2x.png")).unwrap();
    render::clock(72, "12:00", None).save(format!("{dir}/clock.png")).unwrap();
}
