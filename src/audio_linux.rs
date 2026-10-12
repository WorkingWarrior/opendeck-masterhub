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

/// Program aktywnego okna (X11, przez xdotool). Na Wayland nie ma ogólnego sposobu – `None`.
pub fn focused_app() -> Option<String> {
    let out = host::command("xdotool").args(["getactivewindow", "getwindowpid"]).output().ok()?;
    let pid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if pid.is_empty() {
        return None;
    }
    let comm = host::read_file(&format!("/proc/{pid}/comm"))?;
    Some(comm.trim().to_ascii_lowercase())
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
}
