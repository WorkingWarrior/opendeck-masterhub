//! macOS: głośność przez AppleScript (`osascript`).
//!
//! macOS nie udostępnia głośności poszczególnych aplikacji, więc działają tylko "master"
//! (wyjście) i "mic" (wejście). Wejście nie ma wyciszenia – "wyciszenie" ustawia je na 0
//! i przy ponownym wciśnięciu przywraca poprzedni poziom.

use super::{Op, State};
use std::{process::Command, sync::Mutex};

/// Poziom mikrofonu sprzed "wyciszenia".
static MIC_BEFORE_MUTE: Mutex<Option<u32>> = Mutex::new(None);

fn osascript(script: &str) -> Option<String> {
    let out = Command::new("osascript").args(["-e", script]).output().ok()?;
    if !out.status.success() {
        log::warn!("osascript: {}", String::from_utf8_lossy(&out.stderr).trim());
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn get(field: &str) -> Option<String> {
    osascript(&format!("{field} of (get volume settings)"))
}

fn percent(level: f32) -> u32 {
    (level.clamp(0.0, 1.0) * 100.0).round() as u32
}

fn output(op: Op) -> Option<State> {
    let level: f32 = get("output volume")?.parse::<f32>().ok()? / 100.0;
    match op {
        Op::Set(l) => osascript(&format!("set volume output volume {}", percent(l))),
        Op::Step(d) if d != 0.0 => osascript(&format!("set volume output volume {}", percent(level + d))),
        Op::ToggleMute => osascript("set volume output muted (not (output muted of (get volume settings)))"),
        Op::Step(_) => None,
    };
    Some(State {
        level: get("output volume")?.parse::<f32>().ok()? / 100.0,
        muted: get("output muted")? == "true",
    })
}

fn input(op: Op) -> Option<State> {
    let level: u32 = get("input volume")?.parse().ok()?;
    match op {
        Op::Set(l) => {
            osascript(&format!("set volume input volume {}", percent(l)));
        }
        Op::Step(d) if d != 0.0 => {
            osascript(&format!("set volume input volume {}", percent(level as f32 / 100.0 + d)));
        }
        Op::ToggleMute => {
            let mut before = MIC_BEFORE_MUTE.lock().unwrap();
            if level > 0 {
                *before = Some(level);
                osascript("set volume input volume 0");
            } else {
                osascript(&format!("set volume input volume {}", before.take().unwrap_or(50)));
            }
        }
        Op::Step(_) => {}
    }
    let level: u32 = get("input volume")?.parse().ok()?;
    Some(State { level: level as f32 / 100.0, muted: level == 0 })
}

pub fn run(targets: &str, op: Op) -> Option<State> {
    let mut first = None;
    for t in targets.split(',').map(|t| t.trim().to_ascii_lowercase()) {
        let s = match t.as_str() {
            "master" => output(op),
            "mic" => input(op),
            "" => None,
            other => {
                log::warn!("macOS nie pozwala zmieniać głośności aplikacji ({other})");
                None
            }
        };
        first = first.or(s);
    }
    first
}

/// Nazwa aktywnej aplikacji (tylko do podpisu – głośności aplikacji macOS nie zmienia).
pub fn focused_app() -> Option<String> {
    osascript("tell application \"System Events\" to get name of first application process whose frontmost is true")
        .map(|n| n.to_ascii_lowercase())
}
