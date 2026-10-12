//! Głośność: wspólny interfejs, implementacja zależna od systemu.
//!
//! Cele (`targets`, po przecinku): "master" (wyjście), "mic" (wejście), "focused" (aplikacja
//! aktywnego okna) albo nazwy programów, np. "spotify.exe" / "spotify".

#[derive(Debug, Clone, Copy)]
pub enum Op {
    /// Ustaw poziom 0.0–1.0.
    Set(f32),
    /// Zmień poziom o podaną wartość (np. 0.02 = +2%).
    Step(f32),
    ToggleMute,
}

/// Stan po wykonaniu polecenia (dla pierwszego znalezionego celu).
#[derive(Debug, Clone, Copy)]
pub struct State {
    pub level: f32,
    pub muted: bool,
}

#[cfg(windows)]
#[path = "audio_windows.rs"]
mod imp;

#[cfg(target_os = "linux")]
#[path = "audio_linux.rs"]
mod imp;

#[cfg(target_os = "macos")]
#[path = "audio_macos.rs"]
mod imp;

/// Wykonuje polecenie (blokująco) i zwraca stan pierwszego znalezionego celu.
pub use imp::run;
/// Nazwa programu aktywnego okna (małymi literami), jeśli da się ją ustalić.
pub use imp::focused_app;
