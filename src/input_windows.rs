//! Symulacja skrótów klawiszowych przez SendInput.

use std::mem::size_of;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

/// Zwraca (kod VK, czy klawisz rozszerzony).
fn vk(name: &str) -> Option<(u16, bool)> {
    let n = name.to_ascii_lowercase();
    let simple = |v: u16| Some((v, false));
    let ext = |v: u16| Some((v, true));
    match n.as_str() {
        "ctrl" | "control" => simple(VK_CONTROL),
        "shift" => simple(VK_SHIFT),
        "alt" => simple(VK_MENU),
        "win" => ext(VK_LWIN),
        "enter" | "return" => simple(VK_RETURN),
        "esc" | "escape" => simple(VK_ESCAPE),
        "tab" => simple(VK_TAB),
        "space" => simple(VK_SPACE),
        "backspace" => simple(VK_BACK),
        "delete" | "del" => ext(VK_DELETE),
        "insert" | "ins" => ext(VK_INSERT),
        "home" => ext(VK_HOME),
        "end" => ext(VK_END),
        "pageup" => ext(VK_PRIOR),
        "pagedown" => ext(VK_NEXT),
        "up" => ext(VK_UP),
        "down" => ext(VK_DOWN),
        "left" => ext(VK_LEFT),
        "right" => ext(VK_RIGHT),
        "printscreen" => ext(VK_SNAPSHOT),
        "volume_up" => ext(VK_VOLUME_UP),
        "volume_down" => ext(VK_VOLUME_DOWN),
        "volume_mute" => ext(VK_VOLUME_MUTE),
        "media_play_pause" => ext(VK_MEDIA_PLAY_PAUSE),
        "media_next" => ext(VK_MEDIA_NEXT_TRACK),
        "media_prev" => ext(VK_MEDIA_PREV_TRACK),
        "media_stop" => ext(VK_MEDIA_STOP),
        "num_plus" | "kp_plus" => simple(VK_ADD),
        "num_minus" | "kp_minus" => simple(VK_SUBTRACT),
        "num_mul" | "kp_multiply" => simple(VK_MULTIPLY),
        "num_div" | "kp_slash" => ext(VK_DIVIDE),
        "num_del" | "kp_del" => simple(VK_DECIMAL),
        _ => {
            // Klawiatura numeryczna: "num5" albo nazwy jak w grach Source ("kp_5").
            if let Some(d) = n.strip_prefix("num").or_else(|| n.strip_prefix("kp_")).and_then(|s| s.parse::<u16>().ok()) {
                return (d <= 9).then_some((VK_NUMPAD0 + d, false));
            }
            if let Some(f) = n.strip_prefix('f').and_then(|s| s.parse::<u16>().ok()) {
                return (1..=24).contains(&f).then_some((VK_F1 + f - 1, false));
            }
            let mut chars = n.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => simple(c.to_ascii_uppercase() as u16),
                _ => None,
            }
        }
    }
}

fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn send(inputs: &[INPUT]) {
    unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
}

/// Wysyła skrót typu "ctrl+shift+esc".
pub fn send_combo(combo: &str) -> Result<(), String> {
    send_combo_held(combo, 0)
}

/// Jak `send_combo`, ale trzyma klawisze `hold_ms` ms – gry sprawdzają klawiaturę co klatkę
/// i za krótkie wciśnięcie mogą przegapić.
pub fn send_combo_held(combo: &str, hold_ms: u64) -> Result<(), String> {
    let keys = combo
        .split('+')
        .map(|p| vk(p.trim()).ok_or_else(|| format!("nieznany klawisz: {p}")))
        .collect::<Result<Vec<_>, _>>()?;
    // Oprócz kodu VK podajemy kod skanowania – gry (np. przez SDL) rozpoznają klawisze po nim.
    let scan = |v: u16| unsafe { MapVirtualKeyW(v as u32, MAPVK_VK_TO_VSC) } as u16;
    let mut down = vec![];
    for &(v, e) in &keys {
        down.push(key_input(v, scan(v), if e { KEYEVENTF_EXTENDEDKEY } else { 0 }));
    }
    let mut up = vec![];
    for &(v, e) in keys.iter().rev() {
        let f = KEYEVENTF_KEYUP | if e { KEYEVENTF_EXTENDEDKEY } else { 0 };
        up.push(key_input(v, scan(v), f));
    }
    if hold_ms == 0 {
        down.extend(up);
        send(&down);
    } else {
        send(&down);
        std::thread::sleep(std::time::Duration::from_millis(hold_ms));
        send(&up);
    }
    Ok(())
}
