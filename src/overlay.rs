//! Chwilowa nakładka na ekranie pokrętła (np. poziom głośności podczas kręcenia).
//!
//! Na czas nakładki obrazy z OpenDecka i klatki GIF-a dla pokrętła nie są wysyłane; po jej końcu
//! wraca ostatni obraz, który przysłał OpenDeck.

use image::RgbImage;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::{MODULES, hub, hub::Kind, images, with_writer};

const SHOW_FOR: Duration = Duration::from_millis(1500);

#[derive(Default)]
struct State {
    /// Najnowszy obraz nakładki czekający na wysłanie (pośrednie są pomijane).
    pending: Option<RgbImage>,
    until: Option<Instant>,
    running: bool,
}

static STATE: LazyLock<Mutex<HashMap<String, State>>> = LazyLock::new(Default::default);
/// Ostatni obraz klawisza pokrętła od OpenDecka (data URL; `None` = czarny).
static LAST_IMAGE: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(Default::default);
/// Obraz rysowany przez plugin w pełnej rozdzielczości (np. zegar) – ma pierwszeństwo przed
/// obrazem od OpenDecka, który jest przeskalowany do 144×144.
static DIRECT: LazyLock<Mutex<HashMap<String, RgbImage>>> = LazyLock::new(Default::default);

fn upload(device: &str, img: &RgbImage) {
    let jpeg = crate::render::jpeg(img);
    with_writer(device, |dev| hub::upload_dial_image(dev, &jpeg));
}

/// Ustawia obraz pokrętła rysowany przez plugin i od razu go wysyła (chyba że trwa nakładka).
pub fn set_direct(device: &str, img: RgbImage) {
    DIRECT.lock().unwrap().insert(device.to_string(), img.clone());
    if !active(device) {
        upload(device, &img);
    }
}

/// Usuwa obraz rysowany przez plugin; zwraca, czy był ustawiony.
pub fn clear_direct(device: &str) -> bool {
    DIRECT.lock().unwrap().remove(device).is_some()
}

pub fn has_direct(device: &str) -> bool {
    DIRECT.lock().unwrap().contains_key(device)
}

/// Ponownie wysyła obraz pluginu (np. po ponownym podłączeniu pokrętła).
pub fn refresh_direct(device: &str) {
    let img = DIRECT.lock().unwrap().get(device).cloned();
    if let Some(img) = img {
        upload(device, &img);
    }
}

/// Zapamiętuje obraz od OpenDecka, żeby przywrócić go po nakładce.
pub fn remember(device: &str, image: Option<String>) {
    LAST_IMAGE.lock().unwrap().insert(device.to_string(), image);
}

/// Czy na pokrętle jest teraz nakładka (wtedy inne obrazy czekają).
pub fn active(device: &str) -> bool {
    STATE.lock().unwrap().get(device).is_some_and(|s| s.running)
}

/// Pokazuje obraz na ekranach wszystkich podłączonych pokręteł przez ~1,5 s.
pub fn show(img: RgbImage) {
    let dials: Vec<String> =
        MODULES.lock().unwrap().iter().filter(|(_, m)| m.kind == Kind::Dial).map(|(id, _)| id.clone()).collect();
    for device in dials {
        let start = {
            let mut states = STATE.lock().unwrap();
            let s = states.entry(device.clone()).or_default();
            s.pending = Some(img.clone());
            s.until = Some(Instant::now() + SHOW_FOR);
            !std::mem::replace(&mut s.running, true)
        };
        if start {
            thread::spawn(move || worker(device));
        }
    }
}

fn worker(device: String) {
    loop {
        let (next, expired) = {
            let mut states = STATE.lock().unwrap();
            let s = states.entry(device.clone()).or_default();
            (s.pending.take(), s.until.is_none_or(|u| Instant::now() >= u))
        };
        if let Some(img) = next {
            upload(&device, &img);
            continue;
        }
        if !expired {
            thread::sleep(Duration::from_millis(30));
            continue;
        }
        // Koniec nakładki: przywracamy obraz pluginu (np. zegar) albo ten od OpenDecka.
        let direct = DIRECT.lock().unwrap().get(&device).cloned();
        if let Some(img) = direct {
            upload(&device, &img);
        } else {
            let last = LAST_IMAGE.lock().unwrap().get(&device).cloned().flatten();
            if let Ok(jpeg) = images::to_jpeg(last.as_deref(), hub::DIAL_IMAGE_SIZE) {
                with_writer(&device, |dev| hub::upload_dial_image(dev, &jpeg));
            }
        }
        let mut states = STATE.lock().unwrap();
        let s = states.entry(device.clone()).or_default();
        // W trakcie przywracania mogła przyjść nowa nakładka – wtedy działamy dalej.
        if s.pending.is_none() {
            s.running = false;
            return;
        }
    }
}
