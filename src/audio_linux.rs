//! Linux: głośność przez `pactl` (PulseAudio albo PipeWire z pipewire-pulse).
//!
//! - "master" / "mic": domyślne wyjście / wejście (@DEFAULT_SINK@ / @DEFAULT_SOURCE@),
//! - aplikacje: strumienie odtwarzania (sink-inputs) dopasowane po nazwie programu
//!   (application.process.binary) albo application.name, bez rozróżniania wielkości liter
//!   i z pominięciem końcówki ".exe" (żeby profile z Windowsa działały bez zmian).

use super::{Op, State};
use crate::host;

fn pactl(args: &[&str]) -> Option<String> {
    let out = host::command("pactl").args(args).output().ok()?;
    if !out.status.success() {
        log::warn!("pactl {:?}: {}", args, String::from_utf8_lossy(&out.stderr).trim());
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Pierwsza wartość procentowa z wyjścia `pactl get-*-volume` ("... 60% ...").
fn parse_percent(text: &str) -> Option<f32> {
    let pos = text.find('%')?;
    let digits: String = text[..pos].chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    digits.chars().rev().collect::<String>().parse::<f32>().ok().map(|p| p / 100.0)
}

fn percent_arg(level: f32) -> String {
    format!("{}%", (level.clamp(0.0, 1.0) * 100.0).round() as u32)
}

/// Urządzenie domyślne: `kind` = "sink" (wyjście) albo "source" (wejście).
fn device(kind: &str, op: Op) -> Option<State> {
    let target = if kind == "sink" { "@DEFAULT_SINK@" } else { "@DEFAULT_SOURCE@" };
    let get_volume = format!("get-{kind}-volume");
    let level = parse_percent(&pactl(&[&get_volume, target])?)?;
    match op {
        Op::Set(l) => {
            pactl(&[&format!("set-{kind}-volume"), target, &percent_arg(l)])?;
        }
        Op::Step(d) if d != 0.0 => {
            pactl(&[&format!("set-{kind}-volume"), target, &percent_arg(level + d)])?;
        }
        Op::ToggleMute => {
            pactl(&[&format!("set-{kind}-mute"), target, "toggle"])?;
        }
        Op::Step(_) => {}
    }
    let level = parse_percent(&pactl(&[&get_volume, target])?)?;
    let muted = pactl(&[&format!("get-{kind}-mute"), target]).is_some_and(|m| m.contains("yes"));
    Some(State { level, muted })
}

fn normalize(name: &str) -> String {
    name.trim().to_ascii_lowercase().trim_end_matches(".exe").to_string()
}

/// Strumienie odtwarzania pasujące do nazw programów: (indeks, poziom, wyciszony).
fn sink_inputs(apps: &[String]) -> Vec<(u64, f32, bool)> {
    pactl(&["-f", "json", "list", "sink-inputs"]).map(|json| parse_sink_inputs(&json, apps)).unwrap_or_default()
}

/// Parsuje wynik `pactl -f json list sink-inputs`.
fn parse_sink_inputs(json: &str, apps: &[String]) -> Vec<(u64, f32, bool)> {
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(json) else { return vec![] };
    list.iter()
        .filter_map(|s| {
            let props = &s["properties"];
            let names = [props["application.process.binary"].as_str(), props["application.name"].as_str()];
            if !names.iter().flatten().any(|n| apps.contains(&normalize(n))) {
                return None;
            }
            let level = s["volume"]
                .as_object()?
                .values()
                .next()?
                .get("value_percent")?
                .as_str()
                .and_then(parse_percent)?;
            Some((s["index"].as_u64()?, level, s["mute"].as_bool().unwrap_or(false)))
        })
        .collect()
}

fn apps(names: &[String], op: Op) -> Option<State> {
    let streams = sink_inputs(names);
    for (index, level, _) in &streams {
        let index = index.to_string();
        match op {
            Op::Set(l) => pactl(&["set-sink-input-volume", &index, &percent_arg(l)]),
            Op::Step(d) if d != 0.0 => pactl(&["set-sink-input-volume", &index, &percent_arg(level + d)]),
            Op::ToggleMute => pactl(&["set-sink-input-mute", &index, "toggle"]),
            Op::Step(_) => None,
        };
    }
    if streams.is_empty() {
        return None;
    }
    sink_inputs(names).first().map(|(_, level, muted)| State { level: *level, muted: *muted })
}

pub fn run(targets: &str, op: Op) -> Option<State> {
    let mut first = None;
    let mut app_names = vec![];
    for t in targets.split(',').map(normalize).filter(|t| !t.is_empty()) {
        match t.as_str() {
            "master" => {
                let s = device("sink", op);
                first = first.or(s);
            }
            "mic" => {
                let s = device("source", op);
                first = first.or(s);
            }
            "focused" => app_names.extend(focused_app().map(|a| normalize(&a))),
            _ => app_names.push(t),
        }
    }
    if !app_names.is_empty() {
        let s = apps(&app_names, op);
        first = first.or(s);
    }
    first
}

fn comm_from_pid(pid: &str) -> Option<String> {
    let pid = pid.trim();
    if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let comm = host::read_file(&format!("/proc/{pid}/comm"))?;
    let comm = comm.trim().to_ascii_lowercase();
    (!comm.is_empty()).then_some(comm)
}

/// Parsuje wyjście `hyprctl activewindow -j`.
fn parse_hyprland_activewindow(json_str: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(json_str).ok()?;
    if let Some(pid) = val.get("pid").and_then(|p| p.as_i64()) {
        if let Some(comm) = comm_from_pid(&pid.to_string()) {
            return Some(comm);
        }
    }
    val.get("initialClass")
        .or_else(|| val.get("class"))
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
}

fn focused_hyprland() -> Option<String> {
    let out = host::command("hyprctl").args(["activewindow", "-j"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_hyprland_activewindow(&String::from_utf8_lossy(&out.stdout))
}

/// Parsuje wyjście `swaymsg -t get_tree`.
fn parse_sway_tree(json_str: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(json_str).ok()?;
    fn find_focused(node: &serde_json::Value) -> Option<String> {
        if node.get("focused").and_then(|f| f.as_bool()) == Some(true) {
            if let Some(pid) = node.get("pid").and_then(|p| p.as_i64()) {
                if let Some(comm) = comm_from_pid(&pid.to_string()) {
                    return Some(comm);
                }
            }
            if let Some(app_id) = node.get("app_id").and_then(|a| a.as_str()) {
                let id = app_id.trim().to_ascii_lowercase();
                if !id.is_empty() {
                    return Some(id);
                }
            }
            if let Some(class) = node.get("window_properties").and_then(|wp| wp.get("class")).and_then(|c| c.as_str()) {
                let c = class.trim().to_ascii_lowercase();
                if !c.is_empty() {
                    return Some(c);
                }
            }
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(nodes) = node.get(key).and_then(|n| n.as_array()) {
                for child in nodes {
                    if let Some(found) = find_focused(child) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
    find_focused(&val)
}

fn focused_sway() -> Option<String> {
    let out = host::command("swaymsg").args(["-t", "get_tree"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_sway_tree(&String::from_utf8_lossy(&out.stdout))
}

/// KDE Plasma: kdotool (drop-in replacement dla xdotool na KWin).
fn focused_kde() -> Option<String> {
    let out = host::command("kdotool").args(["getactivewindow", "getwindowpid"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    comm_from_pid(&String::from_utf8_lossy(&out.stdout))
}

/// Parsuje odpowiedź D-Bus `org.gnome.Shell.Introspect.GetWindows`.
fn parse_gnome_introspect(text: &str) -> Option<String> {
    for block in text.split("},") {
        if block.contains("'has-focus': <true>") {
            for key in ["'wm-class': <'", "'app-id': <'"] {
                if let Some(pos) = block.find(key) {
                    let rest = &block[pos + key.len()..];
                    if let Some(end) = rest.find('\'') {
                        let raw = rest[..end].trim();
                        let s = raw.strip_suffix(".desktop").unwrap_or(raw);
                        let s = s.split('.').last().unwrap_or(s).to_ascii_lowercase();
                        if !s.is_empty() {
                            return Some(s);
                        }
                    }
                }
            }
        }
    }
    None
}

fn focused_gnome() -> Option<String> {
    let out = host::command("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell",
            "--object-path",
            "/org/gnome/Shell/Introspect",
            "--method",
            "org.gnome.Shell.Introspect.GetWindows",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_gnome_introspect(&String::from_utf8_lossy(&out.stdout))
}

fn focused_x11() -> Option<String> {
    let out = host::command("xdotool").args(["getactivewindow", "getwindowpid"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    comm_from_pid(&String::from_utf8_lossy(&out.stdout))
}

/// Program aktywnego okna (X11 / Wayland).
///
/// Na Waylandzie sprawdza kolejno: Hyprland (hyprctl), Sway (swaymsg), KDE (kdotool),
/// GNOME Shell (gdbus Introspect), a na końcu xdotool (dla X11 lub okien XWayland).
pub fn focused_app() -> Option<String> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if let Some(app) = focused_hyprland() {
            return Some(app);
        }
        if let Some(app) = focused_sway() {
            return Some(app);
        }
        if let Some(app) = focused_kde() {
            return Some(app);
        }
        if let Some(app) = focused_gnome() {
            return Some(app);
        }
    }
    focused_x11()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_from_pactl_output() {
        let out = "Volume: front-left: 39321 /  60% / -13.31 dB,   front-right: 39321 /  60% / -13.31 dB";
        assert_eq!(parse_percent(out), Some(0.6));
        assert_eq!(parse_percent("Volume: mono: 65536 / 100% / 0.00 dB"), Some(1.0));
    }

    #[test]
    fn sink_inputs_match_by_binary_and_name() {
        let json = r#"[
            {"index": 42, "mute": false,
             "volume": {"front-left": {"value": 32768, "value_percent": "50%", "db": "-18 dB"}},
             "properties": {"application.name": "Spotify", "application.process.binary": "spotify"}},
            {"index": 43, "mute": true,
             "volume": {"mono": {"value": 65536, "value_percent": "100%", "db": "0 dB"}},
             "properties": {"application.name": "Firefox", "application.process.binary": "firefox"}}
        ]"#;
        // Nazwy jak z profili Windowsa (".exe") też pasują.
        let apps = vec![normalize("Spotify.exe")];
        assert_eq!(parse_sink_inputs(json, &apps), vec![(42, 0.5, false)]);
        assert_eq!(parse_sink_inputs(json, &[normalize("firefox")]), vec![(43, 1.0, true)]);
        assert!(parse_sink_inputs(json, &[normalize("discord")]).is_empty());
    }

    #[test]
    fn parse_hyprland_output() {
        let json = r#"{"class": "Spotify", "initialClass": "spotify", "title": "Spotify"}"#;
        assert_eq!(parse_hyprland_activewindow(json), Some("spotify".into()));
    }

    #[test]
    fn parse_sway_output() {
        let json = r#"{
            "nodes": [
                {"focused": false, "app_id": "firefox"},
                {"focused": true, "app_id": "com.spotify.Client"}
            ]
        }"#;
        assert_eq!(parse_sway_tree(json), Some("com.spotify.client".into()));
    }

    #[test]
    fn parse_gnome_output() {
        let text = r#"([{'app-id': <'firefox.desktop'>, 'has-focus': <false>}, {'app-id': <'org.gnome.Nautilus.desktop'>, 'has-focus': <true>, 'wm-class': <'org.gnome.Nautilus'>}],)"#;
        assert_eq!(parse_gnome_introspect(text), Some("nautilus".into()));
    }
}
