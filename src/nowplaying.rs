//! Jedno połączenie MPRIS dla wszystkich widocznych instancji akcji.
//! Sygnały aktualizują migawkę, a lokalny zegar interpoluje sam postęp.

use image::{ImageReader, RgbImage};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    sync::{
        Arc, LazyLock, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const FRAME_INTERVAL: Duration = Duration::from_secs(1);
const RETRY_INTERVAL: Duration = Duration::from_secs(5);
const ART_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const MAX_ART_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ART_ENTRIES: usize = 12;
#[cfg(target_os = "linux")]
const DBUS_TIMEOUT: Duration = Duration::from_secs(3);

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
        Self {
            player: "auto".into(),
            rotate_mode: "seek".into(),
            seek_step: 5,
            show_art: true,
            show_progress: true,
            show_artist: true,
            accent_color: "#a0dc30".into(),
            font_family: "auto".into(),
        }
    }
}

impl Settings {
    fn accent(&self) -> image::Rgb<u8> {
        let hex = self
            .accent_color
            .strip_prefix('#')
            .unwrap_or(&self.accent_color);
        if hex.len() == 6
            && hex.bytes().all(|b| b.is_ascii_hexdigit())
            && let Ok(rgb) = u32::from_str_radix(hex, 16)
        {
            return image::Rgb([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
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
    position: Option<i64>,
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
    fn position_now(&self) -> Option<i64> {
        let elapsed = if self.status == "Playing" {
            self.sampled.map_or(0, |t| {
                (t.elapsed().as_micros() as f64 * self.rate.clamp(0.0, 16.0)).min(i64::MAX as f64)
                    as i64
            })
        } else {
            0
        };
        Some(
            self.position?
                .saturating_add(elapsed)
                .clamp(0, self.length.unwrap_or(i64::MAX).max(0)),
        )
    }
}

fn select_player<'a>(
    players: &'a BTreeMap<String, Player>,
    choice: &str,
) -> Option<(&'a String, &'a Player)> {
    if choice != "auto" {
        return players.get_key_value(choice);
    }
    players
        .iter()
        .find(|(_, p)| p.status == "Playing")
        .or_else(|| players.iter().find(|(_, p)| p.status == "Paused"))
        .or_else(|| players.iter().next())
}

fn placeholder() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        crate::i18n::tr("Brak odtwarzacza", "No player")
    }
    #[cfg(not(target_os = "linux"))]
    {
        crate::i18n::tr("MPRIS: tylko Linux", "MPRIS: Linux only")
    }
}

enum Command {
    Wake,
    Press(String, Arc<AtomicBool>),
    Rotate(String, i16, Arc<AtomicBool>),
}

static INSTANCES: LazyLock<Mutex<HashMap<String, Instance>>> = LazyLock::new(Default::default);
static ENGINE: LazyLock<Mutex<Option<tokio::sync::mpsc::Sender<Command>>>> =
    LazyLock::new(Default::default);
static NAMES: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Default::default);
static LAST_KEYS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

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
    let mut settings: Settings = serde_json::from_value(settings.clone()).unwrap_or_default();
    settings.seek_step = settings.seek_step.clamp(1, 60);
    if settings.rotate_mode != "volume" {
        settings.rotate_mode = "seek".into();
    }
    let mut instances = INSTANCES.lock().unwrap();
    if controller == "Keypad"
        && crate::MODULES
            .lock()
            .unwrap()
            .get(device)
            .is_some_and(|m| m.kind == crate::hub::Kind::Dial)
    {
        let previous: Vec<String> = instances
            .iter()
            .filter(|(c, i)| {
                c.as_str() != context && i.device == device && i.controller == "Keypad"
            })
            .map(|(c, _)| c.clone())
            .collect();
        for old in previous {
            if let Some(i) = instances.remove(&old) {
                i.alive.store(false, Ordering::Release);
            }
            LAST_KEYS.lock().unwrap().remove(&old);
        }
    }
    // Każda zmiana ustawień unieważnia już przygotowywaną klatkę poprzedniej wersji.
    if let Some(previous) = instances.get(context) {
        previous.alive.store(false, Ordering::Release);
    }
    let alive = Arc::new(AtomicBool::new(true));
    instances.insert(
        context.into(),
        Instance {
            device: device.into(),
            controller: controller.into(),
            settings,
            alive,
        },
    );
    drop(instances);
    LAST_KEYS.lock().unwrap().remove(context);
    let mut engine = ENGINE.lock().unwrap();
    if engine.as_ref().is_some_and(|tx| tx.is_closed()) {
        *engine = None;
    }
    let tx = engine.get_or_insert_with(|| {
        start_art_worker();
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(monitor(rx));
        tx
    });
    let _ = tx.try_send(Command::Wake);
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
    let restore = (instance.controller == "Keypad"
        && crate::overlay::clear_direct_owned(&instance.device, context))
    .then_some(instance.device);
    if INSTANCES.lock().unwrap().is_empty() {
        NAMES.lock().unwrap().clear();
    }
    // Monitor zachowuje jednego odbiorcę, ale w stanie bez instancji zamyka D-Bus
    // i czeka bez odpytywania. Szybkie przełączenie profilu nie tworzy drugiego monitora.
    send_command(Command::Wake);
    restore
}

fn send_command(command: Command) {
    let wake = matches!(command, Command::Wake);
    if let Some(tx) = ENGINE.lock().unwrap().as_ref()
        && let Err(error) = tx.try_send(command)
        && !wake
    {
        log::warn!("Now Playing: nie można zakolejkować sterowania: {error}");
    }
}

pub fn press(context: &str) {
    let alive = INSTANCES
        .lock()
        .unwrap()
        .get(context)
        .map(|instance| instance.alive.clone());
    if let Some(alive) = alive {
        send_command(Command::Press(context.into(), alive));
    }
}
pub fn rotate(context: &str, ticks: i16) {
    let alive = INSTANCES
        .lock()
        .unwrap()
        .get(context)
        .map(|instance| instance.alive.clone());
    if let Some(alive) = alive {
        send_command(Command::Rotate(context.into(), ticks, alive));
    }
}

pub fn refresh_device(device: &str) {
    let contexts: Vec<String> = INSTANCES
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, i)| i.device == device)
        .map(|(c, _)| c.clone())
        .collect();
    let mut keys = LAST_KEYS.lock().unwrap();
    for context in contexts {
        keys.remove(&context);
    }
    send_command(Command::Wake);
}

#[cfg(not(target_os = "linux"))]
async fn monitor(mut rx: tokio::sync::mpsc::Receiver<Command>) {
    log::info!("Now Playing: MPRIS jest dostępny tylko na Linuksie");
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::Wake => render_all(&BTreeMap::new()).await,
            _ => {}
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

    fn string(map: &Props, key: &str) -> String {
        map.get(key)
            .and_then(|v| String::try_from(v.clone()).ok())
            .unwrap_or_default()
    }
    fn boolean(map: &Props, key: &str) -> bool {
        map.get(key)
            .and_then(|v| bool::try_from(v.clone()).ok())
            .unwrap_or(false)
    }
    fn number(map: &Props, key: &str) -> Option<i64> {
        map.get(key).and_then(|v| {
            // MPRIS wymaga i64, ale Spotify przekazuje mpris:length jako u64.
            // Konwersja sprawdzana nie pozwala zawinąć dużej wartości do liczby ujemnej.
            i64::try_from(v)
                .ok()
                .or_else(|| u64::try_from(v).ok().and_then(|n| i64::try_from(n).ok()))
        })
    }
    fn float(map: &Props, key: &str) -> Option<f64> {
        map.get(key).and_then(|v| f64::try_from(v.clone()).ok())
    }

    async fn fetch(conn: &Connection, name: &str) -> Option<Player> {
        let props: Props = tokio::time::timeout(DBUS_TIMEOUT, async {
            let proxy =
                zbus::Proxy::new(conn, name, PATH, "org.freedesktop.DBus.Properties").await?;
            proxy.call("GetAll", &(PLAYER_IFACE,)).await
        })
        .await
        .ok()?
        .ok()?;
        let meta: Props = props
            .get("Metadata")
            .and_then(|v| Props::try_from(v.clone()).ok())
            .unwrap_or_default();
        let artist = meta
            .get("xesam:artist")
            .and_then(|v| Vec::<String>::try_from(v.clone()).ok())
            .unwrap_or_default()
            .join(", ");
        let position = number(&props, "Position").map(|n| n.max(0));
        Some(Player {
            title: string(&meta, "xesam:title"),
            artist,
            album: string(&meta, "xesam:album"),
            art_url: string(&meta, "mpris:artUrl"),
            length: number(&meta, "mpris:length").filter(|n| *n > 0),
            position,
            sampled: Some(Instant::now()),
            status: string(&props, "PlaybackStatus"),
            can_control: boolean(&props, "CanControl"),
            can_play: boolean(&props, "CanPlay"),
            can_pause: boolean(&props, "CanPause"),
            can_seek: boolean(&props, "CanSeek"),
            volume: float(&props, "Volume").filter(|n| n.is_finite() && *n >= 0.0),
            rate: float(&props, "Rate")
                .filter(|n| n.is_finite() && *n >= 0.0)
                .unwrap_or(1.0),
        })
    }

    async fn discover(
        conn: &Connection,
        players: &mut BTreeMap<String, Player>,
        owners: &mut HashMap<String, Vec<String>>,
    ) {
        let Ok(bus) = zbus::fdo::DBusProxy::new(conn).await else {
            return;
        };
        let Ok(Ok(names)) = tokio::time::timeout(DBUS_TIMEOUT, bus.list_names()).await else {
            return;
        };
        let mut names: Vec<String> = names
            .into_iter()
            .map(|n| n.to_string())
            .filter(|n| n.starts_with(PREFIX))
            .collect();
        names.sort();
        players.retain(|n, _| names.contains(n));
        let previous_owners = std::mem::take(owners);
        for name in &names {
            if let Ok(bus_name) = name.as_str().try_into() {
                if let Ok(Ok(owner)) =
                    tokio::time::timeout(DBUS_TIMEOUT, bus.get_name_owner(bus_name)).await
                {
                    let unchanged = previous_owners
                        .get(owner.as_str())
                        .is_some_and(|names| names.contains(name));
                    if !unchanged || !players.contains_key(name) {
                        if let Some(p) = fetch(conn, name).await {
                            players.insert(name.clone(), p);
                        } else {
                            players.remove(name);
                        }
                    }
                    owners
                        .entry(owner.to_string())
                        .or_default()
                        .push(name.clone());
                } else {
                    players.remove(name);
                }
            }
        }
        let changed = {
            let mut old = NAMES.lock().unwrap();
            if *old == names {
                false
            } else {
                *old = names;
                true
            }
        };
        if changed {
            send_player_list().await;
        }
    }

    async fn refresh_sender(
        conn: &Connection,
        players: &mut BTreeMap<String, Player>,
        owners: &HashMap<String, Vec<String>>,
        sender: Option<&str>,
    ) {
        if let Some(names) = sender.and_then(|s| owners.get(s)) {
            for name in names {
                if let Some(p) = fetch(conn, name).await {
                    players.insert(name.clone(), p);
                } else {
                    players.remove(name);
                }
            }
        }
    }

    async fn send_player_list() {
        let open = crate::OPEN_PANELS.lock().unwrap().clone();
        let contexts: Vec<String> = INSTANCES
            .lock()
            .unwrap()
            .keys()
            .filter(|c| open.contains(*c))
            .cloned()
            .collect();
        if contexts.is_empty() {
            return;
        }
        let json = player_list_json();
        let mut outbound = crate::OUTBOUND_EVENT_MANAGER.lock().await;
        if let Some(out) = outbound.as_mut() {
            for context in contexts {
                let _ = out.send_to_property_inspector(context, json.clone()).await;
            }
        }
    }

    async fn clear_player_list() {
        let changed = {
            let mut names = NAMES.lock().unwrap();
            let changed = !names.is_empty();
            names.clear();
            changed
        };
        if changed {
            send_player_list().await;
        }
    }

    async fn control(conn: &Connection, players: &mut BTreeMap<String, Player>, cmd: Command) {
        let (context, ticks, alive) = match cmd {
            Command::Press(c, alive) => (c, None, alive),
            Command::Rotate(c, t, alive) => (c, Some(t), alive),
            _ => return,
        };
        let Some(instance) = INSTANCES.lock().unwrap().get(&context).cloned() else {
            return;
        };
        if !alive.load(Ordering::Acquire) || !Arc::ptr_eq(&alive, &instance.alive) {
            return;
        }
        let Some((name, player)) =
            select_player(players, &instance.settings.player).map(|(n, p)| (n.clone(), p.clone()))
        else {
            return;
        };
        if !player.can_control {
            return;
        }
        let operation = async {
            let proxy = zbus::Proxy::new(conn, name.as_str(), PATH, PLAYER_IFACE).await?;
            if !alive.load(Ordering::Acquire) {
                return Ok(());
            }
            match ticks {
                None if (player.status == "Playing" && player.can_pause)
                    || (player.status != "Playing" && player.can_play) =>
                {
                    proxy.call::<_, _, ()>("PlayPause", &()).await
                }
                Some(t) if t != 0 && instance.settings.rotate_mode == "volume" => {
                    let Some(level) = player.volume else {
                        return Ok(());
                    };
                    let value = (level + t as f64 * 0.02).clamp(0.0, 1.0);
                    proxy
                        .set_property("Volume", &value)
                        .await
                        .map_err(zbus::Error::from)?;
                    // Dopiero potwierdzony zapis staje się bazą kolejnego obrotu;
                    // nie czekamy na sygnał Volume pomiędzy szybkimi ruchami enkodera.
                    if let Some(current) = players.get_mut(&name) {
                        current.volume = Some(value);
                    }
                    Ok(())
                }
                Some(t) if t != 0 && player.can_seek => {
                    let offset = (t as i64)
                        .saturating_mul(instance.settings.seek_step.clamp(1, 60) as i64)
                        .saturating_mul(1_000_000);
                    proxy.call::<_, _, ()>("Seek", &(offset,)).await
                }
                _ => Ok(()),
            }
        };
        match tokio::time::timeout(DBUS_TIMEOUT, operation).await {
            Ok(Err(e)) => log::warn!("MPRIS {name}: sterowanie nie powiodło się: {e}"),
            Err(_) => log::warn!("MPRIS {name}: przekroczono czas sterowania"),
            Ok(Ok(())) => {}
        }
    }

    async fn retry(rx: &mut tokio::sync::mpsc::Receiver<Command>) -> bool {
        let delay = tokio::time::sleep(RETRY_INTERVAL);
        tokio::pin!(delay);
        loop {
            tokio::select! {
                _ = &mut delay => return true,
                command = rx.recv() => {
                    if command.is_none() { return false }
                    if INSTANCES.lock().unwrap().is_empty() { return true }
                }
            }
        }
    }

    async fn connect_session() -> anyhow::Result<Connection> {
        Ok(tokio::time::timeout(DBUS_TIMEOUT, Connection::session()).await??)
    }

    pub(super) async fn monitor(mut rx: tokio::sync::mpsc::Receiver<Command>) {
        let (render_tx, render_rx) = tokio::sync::watch::channel(BTreeMap::new());
        tokio::spawn(render_worker(render_rx));
        loop {
            while INSTANCES.lock().unwrap().is_empty() {
                render_tx.send_replace(BTreeMap::new());
                if rx.recv().await.is_none() {
                    return;
                }
            }
            let conn = match connect_session().await {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("Now Playing: brak dostępu do sesyjnego D-Bus: {e}");
                    clear_player_list().await;
                    render_tx.send_replace(BTreeMap::new());
                    if !retry(&mut rx).await {
                        return;
                    }
                    continue;
                }
            };
            let name_rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender("org.freedesktop.DBus")
                .unwrap()
                .interface("org.freedesktop.DBus")
                .unwrap()
                .member("NameOwnerChanged")
                .unwrap()
                .build();
            let props_rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .interface("org.freedesktop.DBus.Properties")
                .unwrap()
                .member("PropertiesChanged")
                .unwrap()
                .path(PATH)
                .unwrap()
                .build();
            let seek_rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .interface(PLAYER_IFACE)
                .unwrap()
                .member("Seeked")
                .unwrap()
                .path(PATH)
                .unwrap()
                .build();
            let subscriptions = tokio::time::timeout(DBUS_TIMEOUT, async {
                Ok::<_, zbus::Error>((
                    MessageStream::for_match_rule(name_rule, &conn, Some(64)).await?,
                    MessageStream::for_match_rule(props_rule, &conn, Some(64)).await?,
                    MessageStream::for_match_rule(seek_rule, &conn, Some(64)).await?,
                ))
            })
            .await;
            let Ok(Ok((mut names, mut props, mut seeks))) = subscriptions else {
                log::warn!("Now Playing: nie można subskrybować sygnałów D-Bus");
                clear_player_list().await;
                render_tx.send_replace(BTreeMap::new());
                if !retry(&mut rx).await {
                    return;
                }
                continue;
            };
            let mut players = BTreeMap::new();
            let mut owners = HashMap::new();
            discover(&conn, &mut players, &mut owners).await;
            render_tx.send_replace(players.clone());
            let mut tick = tokio::time::interval(FRAME_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    cmd = rx.recv() => match cmd {
                        None => return,
                        Some(Command::Wake) => {
                            if INSTANCES.lock().unwrap().is_empty() { break }
                            render_tx.send_replace(players.clone());
                        },
                        Some(c) => control(&conn, &mut players, c).await,
                    },
                    msg = names.next() => {
                        let Some(Ok(msg)) = msg else { break };
                        if let Ok((name, _, _)) = msg.body().deserialize::<(String, String, String)>()
                            && name.starts_with(PREFIX) {
                            discover(&conn, &mut players, &mut owners).await;
                            render_tx.send_replace(players.clone());
                        }
                    },
                    msg = props.next() => {
                        let Some(Ok(msg)) = msg else { break };
                        if let Ok((iface, _, _)) = msg.body().deserialize::<(String, Props, Vec<String>)>()
                            && iface == PLAYER_IFACE {
                            let sender = msg.header().sender().map(ToString::to_string);
                            refresh_sender(&conn, &mut players, &owners, sender.as_deref()).await;
                            render_tx.send_replace(players.clone());
                        }
                    },
                    msg = seeks.next() => {
                        let Some(Ok(msg)) = msg else { break };
                        if let (Some(sender), Ok((position,))) = (msg.header().sender(), msg.body().deserialize::<(i64,)>()) {
                            if let Some(names) = owners.get(sender.as_str()) {
                                for name in names {
                                    if let Some(player) = players.get_mut(name) {
                                        player.position = Some(position.max(0));
                                        player.sampled = Some(Instant::now());
                                    }
                                }
                            }
                            render_tx.send_replace(players.clone());
                        }
                    },
                    _ = tick.tick() => { render_tx.send_replace(players.clone()); },
                }
            }
            clear_player_list().await;
            render_tx.send_replace(BTreeMap::new());
            let active = !INSTANCES.lock().unwrap().is_empty();
            if active && !retry(&mut rx).await {
                return;
            }
        }
    }
}

#[cfg(target_os = "linux")]
use linux::monitor;

/// Jeden odbiorca najnowszej migawki; wolny zapis HID nie blokuje sygnałów MPRIS.
#[cfg(target_os = "linux")]
async fn render_worker(mut rx: tokio::sync::watch::Receiver<BTreeMap<String, Player>>) {
    let mut next_frame = tokio::time::Instant::now();
    while rx.changed().await.is_ok() {
        tokio::time::sleep_until(next_frame).await;
        let players = rx.borrow_and_update().clone();
        render_all(&players).await;
        next_frame = tokio::time::Instant::now() + FRAME_INTERVAL;
    }
}

struct Artwork {
    accessed: Instant,
    fetched: Instant,
    pending: bool,
    image: Option<Arc<RgbImage>>,
}

static ART: LazyLock<Mutex<HashMap<String, Artwork>>> = LazyLock::new(Default::default);
static ART_QUEUE: OnceLock<tokio::sync::mpsc::Sender<String>> = OnceLock::new();
static HTTP: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(3))
        .redirects(2)
        .build()
});

fn start_art_worker() {
    ART_QUEUE.get_or_init(|| {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(MAX_ART_ENTRIES);
        tokio::spawn(async move {
            while let Some(url) = rx.recv().await {
                if INSTANCES.lock().unwrap().is_empty() {
                    ART.lock().unwrap().remove(&url);
                    continue;
                }
                let work_url = url.clone();
                let image = match tokio::task::spawn_blocking(move || load_cover(&work_url)).await {
                    Ok(image) => image,
                    Err(e) => {
                        log::warn!("Now Playing: błąd zadania okładki: {e}");
                        None
                    }
                };
                if let Some(entry) = ART.lock().unwrap().get_mut(&url) {
                    entry.image = image;
                    entry.fetched = Instant::now();
                    entry.pending = false;
                }
                send_command(Command::Wake);
            }
        });
        tx
    });
}

/// Renderowanie nigdy nie czeka na HTTP ani dekodowanie. Brakująca okładka jest
/// pobierana przez jeden wspólny worker, a po zakończeniu budzi monitor.
fn cover(url: &str) -> Option<Arc<RgbImage>> {
    if url.is_empty() {
        return None;
    }
    let mut cache = ART.lock().unwrap();
    if let Some(entry) = cache.get_mut(url) {
        entry.accessed = Instant::now();
        if entry.pending || entry.image.is_some() || entry.fetched.elapsed() < ART_RETRY_INTERVAL {
            return entry.image.clone();
        }
    } else if cache.len() >= MAX_ART_ENTRIES {
        let oldest = cache
            .iter()
            .filter(|(_, entry)| !entry.pending)
            .min_by_key(|(_, entry)| entry.accessed)
            .map(|(key, _)| key.clone());
        let oldest = oldest?;
        cache.remove(&oldest);
    }
    cache.insert(
        url.into(),
        Artwork {
            accessed: Instant::now(),
            fetched: Instant::now(),
            pending: true,
            image: None,
        },
    );
    drop(cache);
    if ART_QUEUE
        .get()
        .is_none_or(|queue| queue.try_send(url.into()).is_err())
    {
        ART.lock().unwrap().remove(url);
    }
    None
}

fn load_cover(url: &str) -> Option<Arc<RgbImage>> {
    let parsed = url::Url::parse(url).ok()?;
    let bytes = match parsed.scheme() {
        "file" => {
            let path = parsed.to_file_path().ok()?;
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() || meta.len() > MAX_ART_BYTES {
                return None;
            }
            let mut buf = Vec::new();
            std::fs::File::open(path)
                .ok()?
                .take(MAX_ART_BYTES + 1)
                .read_to_end(&mut buf)
                .ok()?;
            if buf.len() as u64 > MAX_ART_BYTES {
                return None;
            }
            buf
        }
        "http" | "https" => {
            let response = HTTP.get(url).call().ok()?;
            if response
                .header("content-length")
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|n| n > MAX_ART_BYTES)
            {
                return None;
            }
            let mut buf = Vec::new();
            response
                .into_reader()
                .take(MAX_ART_BYTES + 1)
                .read_to_end(&mut buf)
                .ok()?;
            if buf.len() as u64 > MAX_ART_BYTES {
                return None;
            }
            buf
        }
        _ => return None,
    };
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4000);
    limits.max_image_height = Some(4000);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let img = Arc::new(
        reader
            .decode()
            .ok()?
            .resize_to_fill(240, 240, image::imageops::FilterType::Lanczos3)
            .to_rgb8(),
    );
    Some(img)
}

async fn render_all(players: &BTreeMap<String, Player>) {
    let instances = INSTANCES.lock().unwrap().clone();
    if instances.is_empty() {
        return;
    }
    let jobs: Vec<_> = instances
        .into_iter()
        .filter(|(_, i)| i.controller == "Keypad")
        .map(|(context, instance)| {
            let chosen = select_player(players, &instance.settings.player).map(|(_, p)| p.clone());
            (context, instance, chosen)
        })
        .collect();
    let frames = match tokio::task::spawn_blocking(move || {
        jobs.into_iter()
            .filter_map(|(context, instance, player)| {
                if !instance.alive.load(Ordering::Acquire) {
                    return None;
                }
                let kind = crate::MODULES
                    .lock()
                    .unwrap()
                    .get(&instance.device)
                    .map(|m| m.kind)?;
                let size = if kind == crate::hub::Kind::Dial {
                    crate::hub::DIAL_IMAGE_SIZE
                } else {
                    crate::hub::KEY_IMAGE_SIZE
                };
                let art = player
                    .as_ref()
                    .filter(|_| instance.settings.show_art)
                    .and_then(|p| cover(&p.art_url));
                let img = crate::render::now_playing(
                    size,
                    player.as_ref().map(|p| crate::render::NowPlayingView {
                        title: &p.title,
                        artist: &p.artist,
                        album: &p.album,
                        paused: p.status == "Paused",
                        progress: p
                            .length
                            .zip(p.position_now())
                            .map(|(len, position)| position as f32 / len as f32),
                        art: art.as_deref(),
                        show_progress: instance.settings.show_progress,
                        show_artist: instance.settings.show_artist,
                    }),
                    placeholder(),
                    instance.settings.accent(),
                    &instance.settings.font_family,
                );
                if kind == crate::hub::Kind::Dial {
                    crate::overlay::set_direct_owned_if(
                        &instance.device,
                        &context,
                        img,
                        &instance.alive,
                    );
                    None
                } else {
                    Some((context, instance.alive, crate::render::png_data_url(&img)))
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    {
        Ok(v) => v,
        Err(e) => {
            log::warn!("Now Playing: renderowanie: {e}");
            return;
        }
    };
    let mut outbound = crate::OUTBOUND_EVENT_MANAGER.lock().await;
    if let Some(out) = outbound.as_mut() {
        for (context, alive, image) in frames {
            if !alive.load(Ordering::Acquire) {
                continue;
            }
            let changed = LAST_KEYS
                .lock()
                .unwrap()
                .get(&context)
                .is_none_or(|last| last != &image);
            if changed
                && out
                    .set_image(context.clone(), Some(image.clone()), None)
                    .await
                    .is_ok()
            {
                LAST_KEYS.lock().unwrap().insert(context, image);
            }
        }
    }
}
