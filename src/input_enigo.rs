//! Linux / macOS: skróty klawiszowe przez bibliotekę enigo (X11 na Linuksie, CoreGraphics na macOS).
//! Nazwy klawiszy jak w wersji dla Windowsa; "win" to klawisz Super / Command.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

fn key(name: &str) -> Option<Key> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "ctrl" | "control" => Key::Control,
        "shift" => Key::Shift,
        "alt" | "option" => Key::Alt,
        "win" | "meta" | "super" | "cmd" | "command" => Key::Meta,
        "enter" | "return" => Key::Return,
        "esc" | "escape" => Key::Escape,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "volume_up" => Key::VolumeUp,
        "volume_down" => Key::VolumeDown,
        "volume_mute" => Key::VolumeMute,
        "media_play_pause" => Key::MediaPlayPause,
        "media_next" => Key::MediaNextTrack,
        "media_prev" => Key::MediaPrevTrack,
        #[cfg(target_os = "linux")]
        "insert" | "ins" => Key::Insert,
        #[cfg(target_os = "linux")]
        "printscreen" => Key::Print,
        #[cfg(target_os = "linux")]
        "media_stop" => Key::MediaStop,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        "f13" => Key::F13,
        "f14" => Key::F14,
        "f15" => Key::F15,
        "f16" => Key::F16,
        "f17" => Key::F17,
        "f18" => Key::F18,
        "f19" => Key::F19,
        "f20" => Key::F20,
        _ => {
            let mut chars = n.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => Key::Unicode(c),
                _ => return None,
            }
        }
    })
}

/// Wysyła skrót typu "ctrl+shift+esc": wciska klawisze po kolei i puszcza w odwrotnej kolejności.
pub fn send_combo(combo: &str) -> Result<(), String> {
    send_combo_held(combo, 0)
}

/// Jak `send_combo`, ale trzyma klawisze `hold_ms` ms (dla gier).
pub fn send_combo_held(combo: &str, hold_ms: u64) -> Result<(), String> {
    let keys = combo
        .split('+')
        .map(|p| key(p.trim()).ok_or_else(|| format!("nieznany klawisz: {p}")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    for k in &keys {
        enigo.key(*k, Direction::Press).map_err(|e| e.to_string())?;
    }
    if hold_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(hold_ms));
    }
    for k in keys.iter().rev() {
        enigo.key(*k, Direction::Release).map_err(|e| e.to_string())?;
    }
    Ok(())
}
