//! Jedno połączenie MPRIS dla wszystkich widocznych instancji akcji.
//! Sygnały aktualizują migawkę, a lokalny zegar interpoluje sam postęp.

use std::{collections::{BTreeMap, HashMap}, io::Read, sync::{Arc, LazyLock, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use serde::Deserialize;
use image::{RgbImage, ImageReader};

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub player: String,
    #[serde(rename = "rotateMode")]
    pub rotate_mode: String,
    #[serde(rename = "seekStep")]
    pub seek_step: u32,
    #[serde(rename = "showArt")]
    pub show_art: bool,
    #[serde(rename = "showProgress")]
    pub show_progress: bool,
    #[serde(rename = "showArtist")]
    pub show_artist: bool,
    #[serde(rename = "accentColor")]
    pub accent_color: String,
    #[serde(rename = "fontFamily")]
    pub font_family: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { player: "auto".into(), rotate_mode: "seek".into(), seek_step: 5, show_art: true, show_progress: true, show_artist: true, accent_color: "#a0dc30".into(), font_family: "auto".into() }
    }
}

impl Settings {
    fn accent(&self) -> image::Rgb<u8> {
        let hex = self.accent_color.strip_prefix('#').unwrap_or(&self.accent_color);
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            if let Ok(rgb) = u32::from_str_radix(hex, 16) {
                return image::Rgb([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
            }
        }
        image::Rgb([160, 220, 48])
    }
}

#[derive(Clone)]
struct Instance {
    device: String,
    controller: String,
    settings: Settings,
    alive: Arc<AtomicBool>,
}

#[derive(Clone, Default)]
struct Player {
    title: String,
    artist: String,
    album: String,
    art_url: String,
    length: Option<i64>,
    position: i64,
    sampled: Option<Instant>,
    status: String,
    can_control: bool,
    can_play: bool,
    can_pause: bool,
    can_seek: bool,
    volume: Option<f64>,
    rate: f64,
}

impl Player {
    fn position_now(&self) -> i64 {
        let elapsed = if self.status == "Playing" {
            self.sampled.map_or(0, |t| (t.elapsed().as_micros() as f64 * self.rate.clamp(0.0, 16.0)).min(i64::MAX as f64) as i64)
        } else { 0 };
        self.position.saturating_add(elapsed).clamp(0, self.length.unwrap_or(i64::MAX).max(0))
    }
}

enum Command { Wake, Press(String), Rotate(String, i16), Shutdown }

static INSTANCES: LazyLock<Mutex<HashMap<String, Instance>>> = LazyLock::new(Default::default);
static ENGINE: LazyLock<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Command>>>> = LazyLock::new(Default::default);
static NAMES: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Default::default);
static LAST_KEYS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
static LAST_RENDER: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(Default::default);

pub fn player_list_json() -> serde_json::Value {
    serde_json::json!({"players": *NAMES.lock().unwrap()})
}

/// Wywoływane poza wątkiem async: pierwsze otwarcie panelu skanuje fonty systemowe.
pub fn inspector_options_json() -> serde_json::Value {
    let mut options = player_list_json();
    options["fonts"] = serde_json::json!(crate::render::font_families());
    options
}

pub fn register(context: &str, device: &str, controller: &str, settings: &serde_json::Value) {
    let settings: Settings = serde_json::from_value(settings.clone()).unwrap_or_default();
    let mut instances = INSTANCES.lock().unwrap();
    if controller == "Keypad" && crate::MODULES.lock().unwrap().get(device).is_some_and(|m| m.kind == crate::hub::Kind::Dial) {
        let previous: Vec<String> = instances.iter().filter(|(c, i)| c.as_str() != context && i.device == device && i.controller == "Keypad").map(|(c, _)| c.clone()).collect();
        for old in previous {
            if let Some(i) = instances.remove(&old) { i.alive.store(false, Ordering::Release); }
            LAST_KEYS.lock().unwrap().remove(&old);
        }
    }
    let alive = instances.get(context).map(|i| i.alive.clone()).unwrap_or_else(|| Arc::new(AtomicBool::new(true)));
    instances.insert(context.into(), Instance { device: device.into(), controller: controller.into(), settings, alive });
    drop(instances);
    let mut engine = ENGINE.lock().unwrap();
    let tx = engine.get_or_insert_with(|| {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(monitor(rx));
        tx
    });
    let _ = tx.send(Command::Wake);
}

pub fn update(context: &str, settings: &serde_json::Value) {
    let instance = INSTANCES.lock().unwrap().get(context).cloned();
    if let Some(instance) = instance {
        register(context, &instance.device, &instance.controller, settings);
    }
}

/// Zwraca urządzenie, którego własny obraz trzeba przywrócić przez OpenDeck.
pub fn unregister(context: &str) -> Option<String> {
    let instance = INSTANCES.lock().unwrap().remove(context)?;
    instance.alive.store(false, Ordering::Release);
    LAST_KEYS.lock().unwrap().remove(context);
    let restore = (instance.controller == "Keypad" && crate::overlay::clear_direct_owned(&instance.device, context)).then_some(instance.device);
    if INSTANCES.lock().unwrap().is_empty() {
        if let Some(tx) = ENGINE.lock().unwrap().take() { let _ = tx.send(Command::Shutdown); }
        NAMES.lock().unwrap().clear();
        *LAST_RENDER.lock().unwrap() = None;
    }
    restore
}

pub fn press(context: &str) { if let Some(tx) = ENGINE.lock().unwrap().as_ref() { let _ = tx.send(Command::Press(context.into())); } }
pub fn rotate(context: &str, ticks: i16) { if let Some(tx) = ENGINE.lock().unwrap().as_ref() { let _ = tx.send(Command::Rotate(context.into(), ticks)); } }

pub fn refresh_device(device: &str) {
    let contexts: Vec<String> = INSTANCES.lock().unwrap().iter().filter(|(_, i)| i.device == device).map(|(c, _)| c.clone()).collect();
    let mut keys = LAST_KEYS.lock().unwrap();
    for context in contexts { keys.remove(&context); }
    if let Some(tx) = ENGINE.lock().unwrap().as_ref() { let _ = tx.send(Command::Wake); }
}

#[cfg(not(target_os = "linux"))]
async fn monitor(mut rx: tokio::sync::mpsc::UnboundedReceiver<Command>) {
    log::info!("Now Playing: MPRIS jest dostępny tylko na Linuksie");
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::Shutdown => break,
            Command::Wake => render_all(&BTreeMap::new()).await,
            _ => {},
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use futures_util::StreamExt;
    use zbus::{Connection, MatchRule, MessageStream, message::Type, zvariant::OwnedValue};

    const PREFIX: &str = "org.mpris.MediaPlayer2.";
    const PATH: &str = "/org/mpris/MediaPlayer2";
    const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
    type Props = HashMap<String, OwnedValue>;

    fn string(map: &Props, key: &str) -> String { map.get(key).and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default() }
    fn boolean(map: &Props, key: &str) -> bool { map.get(key).and_then(|v| bool::try_from(v.clone()).ok()).unwrap_or(false) }
    fn number(map: &Props, key: &str) -> Option<i64> {
        map.get(key).and_then(|v| {
            // MPRIS wymaga i64, ale Spotify przekazuje mpris:length jako u64.
            // Konwersja sprawdzana nie pozwala zawinąć dużej wartości do liczby ujemnej.
            i64::try_from(v).ok().or_else(|| u64::try_from(v).ok().and_then(|n| i64::try_from(n).ok()))
        })
    }
    fn float(map: &Props, key: &str) -> Option<f64> { map.get(key).and_then(|v| f64::try_from(v.clone()).ok()) }

    async fn fetch(conn: &Connection, name: &str) -> Option<Player> {
        let proxy = zbus::Proxy::new(conn, name, PATH, "org.freedesktop.DBus.Properties").await.ok()?;
        let props: Props = tokio::time::timeout(Duration::from_secs(3), proxy.call("GetAll", &(PLAYER_IFACE,))).await.ok()?.ok()?;
        let meta: Props = props.get("Metadata").and_then(|v| Props::try_from(v.clone()).ok()).unwrap_or_default();
        let artist = meta.get("xesam:artist").and_then(|v| Vec::<String>::try_from(v.clone()).ok()).unwrap_or_default().join(", ");
        let position = number(&props, "Position").unwrap_or(0).max(0);
        Some(Player {
            title: string(&meta, "xesam:title"), artist, album: string(&meta, "xesam:album"), art_url: string(&meta, "mpris:artUrl"),
            length: number(&meta, "mpris:length").filter(|n| *n > 0), position, sampled: Some(Instant::now()),
            status: string(&props, "PlaybackStatus"), can_control: boolean(&props, "CanControl"),
            can_play: boolean(&props, "CanPlay"), can_pause: boolean(&props, "CanPause"), can_seek: boolean(&props, "CanSeek"),
            volume: float(&props, "Volume"),
            rate: float(&props, "Rate").unwrap_or(1.0),
        })
    }

    async fn discover(conn: &Connection, players: &mut BTreeMap<String, Player>, owners: &mut HashMap<String, Vec<String>>) {
        let Ok(bus) = zbus::fdo::DBusProxy::new(conn).await else { return };
        let Ok(names) = bus.list_names().await else { return };
        let mut names: Vec<String> = names.into_iter().map(|n| n.to_string()).filter(|n| n.starts_with(PREFIX)).collect();
        names.sort();
        players.retain(|n, _| names.contains(n));
        owners.clear();
        for name in &names {
            if let Some(p) = fetch(conn, name).await { players.insert(name.clone(), p); }
            else { players.remove(name); }
            if let Ok(bus_name) = name.as_str().try_into() {
                if let Ok(owner) = bus.get_name_owner(bus_name).await {
                    owners.entry(owner.to_string()).or_default().push(name.clone());
                }
            }
        }
        let changed = {
            let mut old = NAMES.lock().unwrap();
            if *old == names { false } else { *old = names; true }
        };
        if changed { send_player_list().await; }
    }

    async fn refresh_sender(conn: &Connection, players: &mut BTreeMap<String, Player>, owners: &HashMap<String, Vec<String>>, sender: Option<&str>) {
        if let Some(names) = sender.and_then(|s| owners.get(s)) {
            for name in names {
                if let Some(p) = fetch(conn, name).await { players.insert(name.clone(), p); }
                else { players.remove(name); }
            }
        }
    }

    async fn send_player_list() {
        let open = crate::OPEN_PANELS.lock().unwrap().clone();
        let contexts: Vec<String> = INSTANCES.lock().unwrap().keys().filter(|c| open.contains(*c)).cloned().collect();
        if contexts.is_empty() { return }
        let json = player_list_json();
        let mut outbound = crate::OUTBOUND_EVENT_MANAGER.lock().await;
        if let Some(out) = outbound.as_mut() {
            for context in contexts { let _ = out.send_to_property_inspector(context, json.clone()).await; }
        }
    }

    fn selected<'a>(players: &'a BTreeMap<String, Player>, choice: &str) -> Option<(&'a String, &'a Player)> {
        if choice != "auto" { return players.get_key_value(choice); }
        players.iter().filter(|(_, p)| p.status == "Playing").next().or_else(|| players.iter().next())
    }

    async fn control(conn: &Connection, players: &BTreeMap<String, Player>, cmd: Command) {
        let (context, ticks) = match cmd { Command::Press(c) => (c, None), Command::Rotate(c, t) => (c, Some(t)), _ => return };
        let Some(instance) = INSTANCES.lock().unwrap().get(&context).cloned() else { return };
        let Some((name, player)) = selected(players, &instance.settings.player) else { return };
        if !player.can_control { return }
        let Ok(proxy) = zbus::Proxy::new(conn, name.as_str(), PATH, PLAYER_IFACE).await else { return };
        let result = match ticks {
            None if (player.status == "Playing" && player.can_pause) || (player.status != "Playing" && player.can_play) => {
                proxy.call::<_, _, ()>("PlayPause", &()).await
            }
            Some(t) if t != 0 && instance.settings.rotate_mode == "volume" => {
                let Some(level) = player.volume else { return };
                let value = (level + t as f64 * 0.02).clamp(0.0, 1.0);
                proxy.set_property("Volume", &value).await.map_err(zbus::Error::from)
            }
            Some(t) if t != 0 && player.can_seek => {
                let offset = (t as i64).saturating_mul(instance.settings.seek_step.clamp(1, 60) as i64).saturating_mul(1_000_000);
                proxy.call::<_, _, ()>("Seek", &(offset,)).await
            }
            _ => return,
        };
        if let Err(e) = result { log::warn!("MPRIS {name}: sterowanie nie powiodło się: {e}"); }
    }

    pub(super) async fn monitor(mut rx: tokio::sync::mpsc::UnboundedReceiver<Command>) {
        loop {
            let conn = match Connection::session().await {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("Now Playing: brak dostępu do sesyjnego D-Bus: {e}");
                    NAMES.lock().unwrap().clear();
                    render_all(&BTreeMap::new()).await;
                    tokio::select! { cmd = rx.recv() => if matches!(cmd, None | Some(Command::Shutdown)) { return }, _ = tokio::time::sleep(Duration::from_secs(5)) => {} }
                    continue;
                }
            };
            let name_rule = MatchRule::builder().msg_type(Type::Signal).sender("org.freedesktop.DBus").unwrap().interface("org.freedesktop.DBus").unwrap().member("NameOwnerChanged").unwrap().build();
            let props_rule = MatchRule::builder().msg_type(Type::Signal).interface("org.freedesktop.DBus.Properties").unwrap().member("PropertiesChanged").unwrap().path(PATH).unwrap().build();
            let seek_rule = MatchRule::builder().msg_type(Type::Signal).interface(PLAYER_IFACE).unwrap().member("Seeked").unwrap().path(PATH).unwrap().build();
            let (Ok(mut names), Ok(mut props), Ok(mut seeks)) = (
                MessageStream::for_match_rule(name_rule, &conn, Some(64)).await,
                MessageStream::for_match_rule(props_rule, &conn, Some(64)).await,
                MessageStream::for_match_rule(seek_rule, &conn, Some(64)).await,
            ) else { tokio::time::sleep(Duration::from_secs(5)).await; continue };
            let mut players = BTreeMap::new();
            let mut owners = HashMap::new();
            discover(&conn, &mut players, &mut owners).await;
            render_all(&players).await;
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    cmd = rx.recv() => match cmd {
                        None | Some(Command::Shutdown) => return,
                        Some(Command::Wake) => render_all(&players).await,
                        Some(c) => control(&conn, &players, c).await,
                    },
                    msg = names.next() => {
                        let Some(Ok(msg)) = msg else { break };
                        if let Ok((name, _, _)) = msg.body().deserialize::<(String, String, String)>() {
                            if name.starts_with(PREFIX) { discover(&conn, &mut players, &mut owners).await; render_all(&players).await; }
                        }
                    },
                    msg = props.next() => {
                        let Some(Ok(msg)) = msg else { break };
                        if let Ok((iface, _, _)) = msg.body().deserialize::<(String, Props, Vec<String>)>() {
                            if iface == PLAYER_IFACE {
                                let sender = msg.header().sender().map(ToString::to_string);
                                refresh_sender(&conn, &mut players, &owners, sender.as_deref()).await;
                                render_all(&players).await;
                            }
                        }
                    },
                    msg = seeks.next() => {
                        let Some(Ok(msg)) = msg else { break };
                        if let (Some(sender), Ok((position,))) = (msg.header().sender(), msg.body().deserialize::<(i64,)>()) {
                            if let Some(names) = owners.get(sender.as_str()) {
                                for name in names {
                                    if let Some(player) = players.get_mut(name) {
                                        player.position = position.max(0);
                                        player.sampled = Some(Instant::now());
                                    }
                                }
                            }
                            render_all(&players).await;
                        }
                    },
                    _ = tick.tick() => render_all(&players).await,
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
use linux::monitor;

/// Klucz URL -> zdekodowany mały obraz. Nie trzymamy danych większych niż 4 MiB.
static ART: LazyLock<Mutex<HashMap<String, (Instant, Option<Arc<RgbImage>>)>>> = LazyLock::new(Default::default);

fn cover(url: &str) -> Option<Arc<RgbImage>> {
    if url.is_empty() { return None }
    if let Some((when, img)) = ART.lock().unwrap().get(url).cloned() {
        if img.is_some() || when.elapsed() < Duration::from_secs(30) { return img }
    }
    let img = load_cover(url);
    let mut cache = ART.lock().unwrap();
    if cache.len() >= 12 { cache.clear(); }
    cache.insert(url.into(), (Instant::now(), img.clone()));
    img
}

fn load_cover(url: &str) -> Option<Arc<RgbImage>> {
    let parsed = url::Url::parse(url).ok()?;
    let bytes = match parsed.scheme() {
        "file" => {
            let path = parsed.to_file_path().ok()?;
            let meta = std::fs::metadata(&path).ok()?;
            if meta.len() > 4 * 1024 * 1024 { return None }
            let mut buf = Vec::new();
            std::fs::File::open(path).ok()?.take(4 * 1024 * 1024 + 1).read_to_end(&mut buf).ok()?;
            if buf.len() > 4 * 1024 * 1024 { return None }
            buf
        }
        "http" | "https" => {
            let response = ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).redirects(2).build().get(url).call().ok()?;
            if response.header("content-length").and_then(|s| s.parse::<usize>().ok()).is_some_and(|n| n > 4 * 1024 * 1024) { return None }
            let mut buf = Vec::new();
            response.into_reader().take(4 * 1024 * 1024 + 1).read_to_end(&mut buf).ok()?;
            if buf.len() > 4 * 1024 * 1024 { return None }
            buf
        }
        _ => return None,
    };
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4000);
    limits.max_image_height = Some(4000);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let img = Arc::new(reader.decode().ok()?.resize_to_fill(240, 240, image::imageops::FilterType::Lanczos3).to_rgb8());
    Some(img)
}

async fn render_all(players: &BTreeMap<String, Player>) {
    let instances = INSTANCES.lock().unwrap().clone();
    if instances.is_empty() { return }
    {
        let mut last = LAST_RENDER.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) { return }
        *last = Some(Instant::now());
    }
    let jobs: Vec<_> = instances.into_iter().filter(|(_, i)| i.controller == "Keypad").map(|(context, instance)| {
        let chosen = if instance.settings.player == "auto" {
            players.iter().find(|(_, p)| p.status == "Playing").or_else(|| players.iter().next()).map(|(_, p)| p.clone())
        } else { players.get(&instance.settings.player).cloned() };
        (context, instance, chosen)
    }).collect();
    let frames = match tokio::task::spawn_blocking(move || {
        jobs.into_iter().filter_map(|(context, instance, player)| {
            if !instance.alive.load(Ordering::Acquire) { return None }
            let kind = crate::MODULES.lock().unwrap().get(&instance.device).map(|m| m.kind)?;
            let size = if kind == crate::hub::Kind::Dial { crate::hub::DIAL_IMAGE_SIZE } else { crate::hub::KEY_IMAGE_SIZE };
            let art = player.as_ref().filter(|_| instance.settings.show_art).and_then(|p| cover(&p.art_url));
            let img = crate::render::now_playing(size, player.as_ref().map(|p| crate::render::NowPlayingView {
                title: &p.title, artist: &p.artist, album: &p.album, paused: p.status == "Paused",
                progress: p.length.map(|len| p.position_now() as f32 / len as f32), art: art.as_deref(),
                show_progress: instance.settings.show_progress, show_artist: instance.settings.show_artist,
            }), crate::i18n::tr("Brak odtwarzacza", "No player"), instance.settings.accent(), &instance.settings.font_family);
            if kind == crate::hub::Kind::Dial {
                crate::overlay::set_direct_owned_if(&instance.device, &context, img, &instance.alive);
                None
            } else { Some((context, instance.alive, crate::render::png_data_url(&img))) }
        }).collect::<Vec<_>>()
    }).await { Ok(v) => v, Err(e) => { log::warn!("Now Playing: renderowanie: {e}"); return } };
    let mut outbound = crate::OUTBOUND_EVENT_MANAGER.lock().await;
    if let Some(out) = outbound.as_mut() {
        for (context, alive, image) in frames {
            if !alive.load(Ordering::Acquire) { continue }
            let changed = LAST_KEYS.lock().unwrap().get(&context).is_none_or(|last| last != &image);
            if changed && out.set_image(context.clone(), Some(image.clone()), None).await.is_ok() {
                LAST_KEYS.lock().unwrap().insert(context, image);
            }
        }
    }
}
