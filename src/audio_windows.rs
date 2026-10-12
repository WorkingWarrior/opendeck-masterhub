//! Windows: głośność przez Core Audio: urządzenie domyślne (master), mikrofon i sesje aplikacji.
//!
//! COM działa na jednym, dedykowanym wątku; reszta pluginu wysyła mu polecenia przez `run`.

use super::{Op, State};
use std::{
    path::Path,
    sync::{LazyLock, Mutex, mpsc},
    thread,
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, MAX_PATH},
        Media::Audio::{
            EDataFlow, Endpoints::IAudioEndpointVolume, IAudioSessionControl2, IAudioSessionManager2,
            IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eCapture, eConsole, eRender,
        },
        System::{
            Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
        },
    },
    core::{Interface, PWSTR, Result},
};

type Request = (String, Op, mpsc::Sender<Option<State>>);

static WORKER: LazyLock<Mutex<mpsc::Sender<Request>>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::channel::<Request>();
    thread::spawn(move || {
        let audio = match Audio::new() {
            Ok(a) => a,
            Err(e) => {
                log::error!("Nie udało się otworzyć audio: {e}");
                return;
            }
        };
        for (targets, op, reply) in rx {
            let _ = reply.send(audio.apply(&targets, op));
        }
    });
    Mutex::new(tx)
});

/// Wykonuje polecenie (blokująco). `targets`: "master", "mic" lub nazwy procesów po przecinku.
pub fn run(targets: &str, op: Op) -> Option<State> {
    let (tx, rx) = mpsc::channel();
    WORKER.lock().unwrap().send((targets.to_string(), op, tx)).ok()?;
    rx.recv().ok().flatten()
}

enum Volume {
    Endpoint(IAudioEndpointVolume),
    Session(ISimpleAudioVolume),
}

impl Volume {
    fn apply(&self, op: Op) -> Result<State> {
        unsafe {
            match self {
                Volume::Endpoint(v) => {
                    match op {
                        Op::Set(l) => v.SetMasterVolumeLevelScalar(l.clamp(0.0, 1.0), std::ptr::null())?,
                        Op::Step(d) => {
                            let l = v.GetMasterVolumeLevelScalar()?;
                            v.SetMasterVolumeLevelScalar((l + d).clamp(0.0, 1.0), std::ptr::null())?
                        }
                        Op::ToggleMute => v.SetMute(!v.GetMute()?.as_bool(), std::ptr::null())?,
                    }
                    Ok(State { level: v.GetMasterVolumeLevelScalar()?, muted: v.GetMute()?.as_bool() })
                }
                Volume::Session(v) => {
                    match op {
                        Op::Set(l) => v.SetMasterVolume(l.clamp(0.0, 1.0), std::ptr::null())?,
                        Op::Step(d) => {
                            let l = v.GetMasterVolume()?;
                            v.SetMasterVolume((l + d).clamp(0.0, 1.0), std::ptr::null())?
                        }
                        Op::ToggleMute => v.SetMute(!v.GetMute()?.as_bool(), std::ptr::null())?,
                    }
                    Ok(State { level: v.GetMasterVolume()?, muted: v.GetMute()?.as_bool() })
                }
            }
        }
    }
}

struct Audio {
    enumerator: IMMDeviceEnumerator,
}

impl Audio {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            Ok(Audio { enumerator: CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? })
        }
    }

    fn apply(&self, targets: &str, op: Op) -> Option<State> {
        let mut first = None;
        for v in self.volumes(targets) {
            match v.apply(op) {
                Ok(s) => {
                    first.get_or_insert(s);
                }
                Err(e) => log::warn!("Błąd audio: {e}"),
            }
        }
        first
    }

    fn volumes(&self, targets: &str) -> Vec<Volume> {
        let targets: Vec<String> =
            targets.split(',').map(|t| t.trim().to_ascii_lowercase()).filter(|t| !t.is_empty()).collect();
        let mut out = vec![];
        for t in &targets {
            let flow = match t.as_str() {
                "master" => eRender,
                "mic" => eCapture,
                _ => continue,
            };
            match self.endpoint(flow) {
                Ok(v) => out.push(Volume::Endpoint(v)),
                Err(e) => log::warn!("Brak urządzenia audio {t}: {e}"),
            }
        }
        // "focused" = aplikacja aktywnego okna (sprawdzana przy każdej operacji).
        let focused = targets.iter().any(|t| t == "focused").then(focused_app).flatten();
        let apps: Vec<&String> = targets
            .iter()
            .filter(|t| *t != "master" && *t != "mic" && *t != "focused")
            .chain(focused.as_ref())
            .collect();
        if !apps.is_empty() {
            match self.sessions(&apps) {
                Ok(s) => out.extend(s.into_iter().map(Volume::Session)),
                Err(e) => log::warn!("Błąd sesji audio: {e}"),
            }
        }
        out
    }

    fn endpoint(&self, flow: EDataFlow) -> Result<IAudioEndpointVolume> {
        unsafe { self.enumerator.GetDefaultAudioEndpoint(flow, eConsole)?.Activate(CLSCTX_ALL, None) }
    }

    fn sessions(&self, apps: &[&String]) -> Result<Vec<ISimpleAudioVolume>> {
        let mut out = vec![];
        unsafe {
            let dev = self.enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let mgr: IAudioSessionManager2 = dev.Activate(CLSCTX_ALL, None)?;
            let sessions = mgr.GetSessionEnumerator()?;
            for i in 0..sessions.GetCount()? {
                let Ok(ctl) = sessions.GetSession(i) else { continue };
                let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
                let Ok(pid) = ctl2.GetProcessId() else { continue };
                let Some(name) = process_name(pid) else { continue };
                if apps.iter().any(|a| **a == name) {
                    if let Ok(vol) = ctl.cast::<ISimpleAudioVolume>() {
                        out.push(vol);
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Proces aktywnego okna, np. "chrome.exe". Wszystkie sesje audio tego programu (także z innych
/// procesów o tej samej nazwie, jak w przeglądarkach) są traktowane jako jego dźwięk.
pub fn focused_app() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        process_name(pid)
    }
}

/// Nazwa pliku procesu małymi literami, np. "spotify.exe".
fn process_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&path).file_name().map(|n| n.to_string_lossy().to_ascii_lowercase())
    }
}
