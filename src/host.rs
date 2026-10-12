//! Linux: uruchamianie programów systemowych (pactl, xdotool, xdg-open).
//!
//! OpenDeck bywa instalowany jako Flatpak – plugin działa wtedy w piaskownicy, gdzie tych
//! programów nie ma. W takim przypadku uruchamiamy je w systemie przez `flatpak-spawn --host`
//! (tak samo OpenDeck uruchamia pluginy w Node.js).

use std::{process::Command, sync::LazyLock};

static IN_FLATPAK: LazyLock<bool> = LazyLock::new(|| std::path::Path::new("/.flatpak-info").exists());

/// `Command` dla programu z systemu hosta.
pub fn command(program: &str) -> Command {
    if *IN_FLATPAK {
        let mut cmd = Command::new("flatpak-spawn");
        cmd.args(["--host", program]);
        cmd
    } else {
        Command::new(program)
    }
}

/// Czyta plik z systemu hosta (np. /proc/<pid>/comm – w piaskownicy procesy hosta nie są widoczne).
pub fn read_file(path: &str) -> Option<String> {
    if *IN_FLATPAK {
        let out = command("cat").arg(path).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        std::fs::read_to_string(path).ok()
    }
}
