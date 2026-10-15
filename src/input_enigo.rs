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
        "printscreen" => Key::PrintScr,
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

#[cfg(target_os = "linux")]
fn linux_scancode(name: &str) -> Option<u16> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "ctrl" | "control" => 29, // KEY_LEFTCTRL
        "shift" => 42,            // KEY_LEFTSHIFT
        "alt" | "option" => 56,   // KEY_LEFTALT
        "win" | "meta" | "super" | "cmd" | "command" => 125, // KEY_LEFTMETA
        "enter" | "return" => 28, // KEY_ENTER
        "esc" | "escape" => 1,    // KEY_ESC
        "tab" => 15,              // KEY_TAB
        "space" => 57,            // KEY_SPACE
        "backspace" => 14,        // KEY_BACKSPACE
        "delete" | "del" => 111,  // KEY_DELETE
        "home" => 102,
        "end" => 107,
        "pageup" => 104,
        "pagedown" => 109,
        "up" => 103,
        "down" => 108,
        "left" => 105,
        "right" => 106,
        "insert" | "ins" => 110,
        "printscreen" => 99,
        "volume_up" => 115,
        "volume_down" => 114,
        "volume_mute" => 113,
        "media_play_pause" => 164,
        "media_next" => 163,
        "media_prev" => 165,
        "media_stop" => 166,
        "f1" => 59, "f2" => 60, "f3" => 61, "f4" => 62,
        "f5" => 63, "f6" => 64, "f7" => 65, "f8" => 66,
        "f9" => 67, "f10" => 68, "f11" => 87, "f12" => 88,
        "1" => 2, "2" => 3, "3" => 4, "4" => 5, "5" => 6,
        "6" => 7, "7" => 8, "8" => 9, "9" => 10, "0" => 11,
        "a" => 30, "b" => 48, "c" => 46, "d" => 32, "e" => 18,
        "f" => 33, "g" => 34, "h" => 35, "i" => 23, "j" => 36,
        "k" => 37, "l" => 38, "m" => 50, "n" => 49, "o" => 24,
        "p" => 25, "q" => 16, "r" => 19, "s" => 31, "t" => 20,
        "u" => 22, "v" => 47, "w" => 17, "x" => 45, "y" => 21,
        "z" => 44,
        _ => return None,
    })
}

#[cfg(target_os = "linux")]
/// Argumenty dla `ydotool key`: wciśnięcie lub puszczenie całej kombinacji.
/// Wciśnięcie i puszczenie to dwa osobne wywołania – `--delay` w ydotool to
/// opóźnienie przed każdym zdarzeniem, a nie czas przytrzymania klawiszy.
fn ydotool_args(codes: &[u16], down: bool) -> Vec<String> {
    let mut args = vec!["key".to_string()];
    for code in codes {
        args.push(format!("{code}:{}", if down { 1 } else { 0 }));
    }
    args
}

#[cfg(target_os = "linux")]
fn ydotool_key(codes: &[u16], down: bool) -> Result<(), String> {
    let status = crate::host::command("ydotool")
        .args(&ydotool_args(codes, down))
        .status()
        .map_err(|e| format!("nie można uruchomić ydotool: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ydotool zakończył się błędem: {status}"))
    }
}

#[cfg(target_os = "linux")]
fn try_ydotool(combo: &str, hold_ms: u64) -> Result<(), String> {
    let parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let codes: Vec<u16> = parts
        .iter()
        .map(|p| linux_scancode(p).ok_or_else(|| format!("nieznany klawisz dla ydotool: {p}")))
        .collect::<Result<_, _>>()?;

    let mut release: Vec<u16> = codes.clone();
    release.reverse();
    if let Err(e) = ydotool_key(&codes, true) {
        let cleanup = ydotool_key(&release, false);
        return Err(format!("wciśnięcie klawiszy nie udało się: {e}; próba puszczenia: {cleanup:?}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(hold_ms));
    if let Err(e) = ydotool_key(&release, false) {
        let retry = ydotool_key(&release, false);
        return Err(format!("puszczenie klawiszy nie udało się (mogą pozostać wciśnięte): {e}; ponowienie: {retry:?}"));
    }
    Ok(())
}

/// Wysyła skrót typu "ctrl+shift+esc": wciska klawisze po kolei i puszcza w odwrotnej kolejności.
pub fn send_combo(combo: &str) -> Result<(), String> {
    send_combo_held(combo, 0)
}

#[cfg(target_os = "linux")]
/// Nazwa displaya, który na pewno nie istnieje – enigo 0.6 wysyła każde zdarzenie
/// do wszystkich ustanowionych połączeń, więc na sesji Wayland z XWayland klawisz
/// trafiłby do systemu dwa razy (raz przez protokół wayland, raz przez XTest do
/// XWayland). Podanie nieistniejącej nazwy wyłącza drugi backend.
const NO_DISPLAY: &str = "opendeck-masterhub-none";

#[cfg(target_os = "linux")]
fn enigo_backend(wayland: bool) -> Result<Enigo, String> {
    let mut settings = Settings::default();
    if wayland {
        settings.x11_display = Some(NO_DISPLAY.into());
    } else {
        settings.wayland_display = Some(NO_DISPLAY.into());
    }
    Enigo::new(&settings).map_err(|e| format!("{}: {e}", if wayland { "wayland" } else { "x11" }))
}

#[cfg(target_os = "linux")]
fn new_enigo() -> Result<Enigo, String> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        // XWayland nie dostarcza zdarzeń do natywnych aplikacji Wayland.
        enigo_backend(true)
    } else {
        enigo_backend(false)
    }
}

#[cfg(not(target_os = "linux"))]
fn new_enigo() -> Result<Enigo, String> {
    Enigo::new(&Settings::default()).map_err(|e| e.to_string())
}

/// Jak `send_combo`, ale trzyma klawisze `hold_ms` ms (dla gier).
pub fn send_combo_held(combo: &str, hold_ms: u64) -> Result<(), String> {
    let keys = combo
        .split('+')
        .map(|p| key(p.trim()).ok_or_else(|| format!("nieznany klawisz: {p}")))
        .collect::<Result<Vec<_>, _>>()?;

    let mut enigo = match new_enigo() {
        Ok(enigo) => enigo,
        Err(e) => {
            #[cfg(target_os = "linux")]
            return try_ydotool(combo, hold_ms).map_err(|yd| format!("{e}; ydotool: {yd}"));
            #[cfg(not(target_os = "linux"))]
            return Err(e);
        }
    };
    for (index, k) in keys.iter().enumerate() {
        if let Err(e) = enigo.key(*k, Direction::Press) {
            #[cfg(target_os = "linux")]
            if index == 0
                && matches!(e, enigo::InputError::Simulate("no keymap available" | "no way to enter key"))
            {
                // Enigo mógł połączyć się z Wayland bez protokołu klawiatury.
                // Ten błąd występuje przed wysłaniem pierwszego zdarzenia.
                drop(enigo);
                return try_ydotool(combo, hold_ms).map_err(|yd| format!("{e}; ydotool: {yd}"));
            }
            #[cfg(not(target_os = "linux"))]
            let _ = index;
            return Err(e.to_string());
        }
    }
    if hold_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(hold_ms));
    }
    for k in keys.iter().rev() {
        enigo.key(*k, Direction::Release).map_err(|e| e.to_string())?;
    }
    // Po błędzie w połowie sekwencji enigo zwalnia wciśnięte klawisze przy Drop
    // (release_keys_when_dropped), więc modyfikatory nie zostają wciśnięte.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regresja: każdy klawisz obsługiwany przez enigo musi mieć też scancode
    /// dla fallbacku ydotool – inaczej fallback pada na znanym klawiszu.
    #[test]
    #[cfg(target_os = "linux")]
    fn kazdy_klawisz_ma_scancode_dla_ydotool() {
        for name in [
            "ctrl",
            "shift",
            "alt",
            "win",
            "enter",
            "esc",
            "tab",
            "space",
            "backspace",
            "delete",
            "home",
            "end",
            "pageup",
            "pagedown",
            "up",
            "down",
            "left",
            "right",
            "insert",
            "printscreen",
            "volume_up",
            "volume_down",
            "volume_mute",
            "media_play_pause",
            "media_next",
            "media_prev",
            "media_stop",
            "f1",
            "f5",
            "f12",
            "a",
            "z",
            "0",
            "9",
        ] {
            assert!(key(name).is_some(), "enigo nie zna klawisza: {name}");
            assert!(
                linux_scancode(name).is_some(),
                "ydotool nie zna klawisza: {name}"
            );
        }
    }

    /// Regresja: wciśnięcie i puszczenie klawiszy to dwa wywołania ydotool bez
    /// `--delay` (opóźnienie ydotool to nie czas przytrzymania klawiszy).
    #[test]
    #[cfg(target_os = "linux")]
    fn ydotool_bez_flagi_delay() {
        assert_eq!(
            ydotool_args(&[29, 46], true),
            vec!["key".to_string(), "29:1".to_string(), "46:1".to_string()]
        );
        assert_eq!(
            ydotool_args(&[46, 29], false),
            vec!["key".to_string(), "46:0".to_string(), "29:0".to_string()]
        );
    }

    #[test]
    fn nieznane_klawisze_sa_odrzucane() {
        assert!(key("foo").is_none());
        assert!(key("ctrl+shift").is_none());
    }
}
