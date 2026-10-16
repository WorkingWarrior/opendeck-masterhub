//! Chwilowa nakładka na ekranie pokrętła (np. poziom głośności podczas kręcenia).
//!
//! Na czas nakładki obrazy z OpenDecka i klatki GIF-a dla pokrętła nie są wysyłane; po jej końcu
//! wraca ostatni obraz, który przysłał OpenDeck.

use image::RgbImage;
use std::{
    collections::HashMap,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
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
static LAST_IMAGE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(Default::default);
/// Obraz rysowany przez plugin w pełnej rozdzielczości (np. zegar) – ma pierwszeństwo przed
/// obrazem od OpenDecka, który jest przeskalowany do 144×144.
struct DirectImage {
    owner: String,
    image: RgbImage,
    sent: bool,
}
static DIRECT: LazyLock<Mutex<HashMap<String, DirectImage>>> = LazyLock::new(Default::default);
static OWNERS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

fn upload(device: &str, img: &RgbImage) -> bool {
    let jpeg = crate::render::jpeg(img);
    with_writer(device, |dev| hub::upload_dial_image(dev, &jpeg))
}

/// Zmiana akcji na jednym ekranie unieważnia wszystkie spóźnione klatki poprzedniej.
pub fn claim_direct(device: &str, owner: &str) {
    // GIF chroni własną klatkę osobną blokadą. Zatrzymujemy go przed zajęciem
    // ekranu przez widget; stop sam używa blokady przejścia modułu.
    crate::gif::stop(device, 0, false);
    crate::gif::stop(device, 0, true);
    crate::gif::with_transition(device, || {
        OWNERS.lock().unwrap().insert(device.into(), owner.into());
        DIRECT.lock().unwrap().remove(device);
    });
}

/// Właściciel i zapis HID zmieniają się pod tą samą blokadą co zwykłe obrazy i GIF.
pub fn set_direct_owned(device: &str, owner: &str, img: RgbImage) {
    set_direct_inner(device, owner, img, || true);
}

pub fn set_direct_owned_if(device: &str, owner: &str, img: RgbImage, alive: &AtomicBool) {
    set_direct_inner(device, owner, img, || alive.load(Ordering::Acquire));
}

fn set_direct_inner(device: &str, owner: &str, img: RgbImage, alive: impl FnOnce() -> bool) {
    crate::gif::with_transition(device, || {
        if !alive() {
            return;
        }
        if OWNERS
            .lock()
            .unwrap()
            .get(device)
            .is_none_or(|current| current != owner)
        {
            return;
        }
        let mut direct = DIRECT.lock().unwrap();
        let overlay_active = active(device);
        if direct
            .get(device)
            .is_some_and(|d| d.owner == owner && d.image == img && (d.sent || overlay_active))
        {
            return;
        }
        direct.insert(
            device.to_string(),
            DirectImage {
                owner: owner.to_string(),
                image: img.clone(),
                sent: false,
            },
        );
        drop(direct);
        if !overlay_active {
            let sent = upload(device, &img);
            if let Some(direct) = DIRECT.lock().unwrap().get_mut(device) {
                direct.sent = sent;
            }
        }
    });
}

pub fn clear_direct_owned(device: &str, owner: &str) -> bool {
    crate::gif::with_transition(device, || {
        let mut owners = OWNERS.lock().unwrap();
        if owners.get(device).is_none_or(|current| current != owner) {
            return false;
        }
        owners.remove(device);
        DIRECT.lock().unwrap().remove(device);
        true
    })
}

pub fn has_direct(device: &str) -> bool {
    OWNERS.lock().unwrap().contains_key(device)
}

/// Ponownie wysyła obraz pluginu (np. po ponownym podłączeniu pokrętła).
pub fn refresh_direct(device: &str) {
    crate::gif::with_transition(device, || {
        let img = DIRECT.lock().unwrap().get(device).map(|d| d.image.clone());
        if let Some(img) = img.filter(|_| !active(device)) {
            let sent = upload(device, &img);
            if let Some(direct) = DIRECT.lock().unwrap().get_mut(device) {
                direct.sent = sent;
            }
        }
    });
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
    let dials: Vec<String> = MODULES
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, m)| m.kind == Kind::Dial)
        .map(|(id, _)| id.clone())
        .collect();
    for device in dials {
        let start = crate::gif::with_transition(&device, || {
            let mut states = STATE.lock().unwrap();
            let s = states.entry(device.clone()).or_default();
            s.pending = Some(img.clone());
            s.until = Some(Instant::now() + SHOW_FOR);
            !std::mem::replace(&mut s.running, true)
        });
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
            (
                s.pending.take(),
                s.until.is_none_or(|u| Instant::now() >= u),
            )
        };
        if let Some(img) = next {
            crate::gif::with_transition(&device, || upload(&device, &img));
            continue;
        }
        if !expired {
            thread::sleep(Duration::from_millis(30));
            continue;
        }
        // Koniec nakładki: przywracamy obraz pluginu (np. zegar) albo ten od OpenDecka.
        let finished = crate::gif::with_transition(&device, || {
            let mut states = STATE.lock().unwrap();
            let s = states.entry(device.clone()).or_default();
            // Nowa nakładka mogła przyjść od sprawdzenia terminu. Zmiana running
            // i przywrócenie obrazu muszą być atomowe względem set_direct/show.
            if s.pending.is_some() || s.until.is_some_and(|until| Instant::now() < until) {
                return false;
            }
            s.running = false;
            drop(states);
            let direct = DIRECT.lock().unwrap().get(&device).map(|d| d.image.clone());
            if let Some(img) = direct {
                let sent = upload(&device, &img);
                if let Some(direct) = DIRECT.lock().unwrap().get_mut(&device) {
                    direct.sent = sent;
                }
            } else if !crate::gif::is_playing(&device, 0) {
                let last = LAST_IMAGE.lock().unwrap().get(&device).cloned().flatten();
                if let Ok(jpeg) = images::to_jpeg(last.as_deref(), hub::DIAL_IMAGE_SIZE) {
                    with_writer(&device, |dev| hub::upload_dial_image(dev, &jpeg));
                }
            }
            true
        });
        if finished {
            return;
        }
    }
}
