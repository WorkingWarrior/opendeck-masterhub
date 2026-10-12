//! Podgląd obrazów generowanych przez plugin (zapis do PNG).
#[path = "../src/render.rs"]
#[allow(dead_code)]
mod render;

fn main() {
    let out = std::env::args().nth(1).expect("katalog");
    render::gauge(240, 0.45, false, "45%", "Spotify").save(format!("{out}/gauge.png")).unwrap();
    render::gauge(240, 0.8, true, "Wycisz.", "System").save(format!("{out}/gauge-muted.png")).unwrap();
    render::clock(240, "20:47", Some("czw, 9 paź")).save(format!("{out}/clock.png")).unwrap();
}
