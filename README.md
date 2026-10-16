# MasterHUB for OpenDeck

**English** · [Polski](README.pl.md)

An unofficial [OpenDeck](https://github.com/nekename/OpenDeck) plugin that drives
**Cooler Master MasterHUB** modules directly over USB HID – no official Cooler Master software
needed. Not affiliated with or endorsed by Cooler Master.

## What works

| Module | In OpenDeck |
|---|---|
| 15xKey (IPS screens) | 5×3 grid, OpenDeck icons on the keys, OpenDeck brightness slider |
| 1xDial (dial with a round screen) | a key (press + 240×240 image on the screen) and an encoder (rotation) |
| 3xKnob | 3 encoders (rotate + press) |
| 2xRoller | 2 encoders |
| 5xFader | 5 encoders; with the "Volume" action the slider position is the volume |
| Base | lighting only (through the "Lighting" action) |

Every module is a separate OpenDeck device with its own profiles, so re-plugging modules
doesn't break assignments.

Actions in the **MasterHUB** category:

- **Lighting** – colour, effect, brightness and speed of the LEDs on the base,
  knobs and fader (each LED separately).
- **Volume** – system, microphone, the focused window's app or chosen apps;
  absolute on the fader, stepped on knobs / rollers / the dial, press to mute. While changing
  it briefly shows a round level gauge on the dial screen.
- **GIF animation** – on a key, on the dial screen, stretched across the whole
  15xKey module or the same GIF on every key.
- **Clock** – time and date, e.g. on the dial's round screen. Pressing it can open
  a file / program / URL, send a keyboard shortcut or mute the system / microphone / focused app.
- **Now Playing** (Linux) – MPRIS track, artwork and progress on the round dial screen,
  or compact artwork on a 15xKey key. Press to play/pause; rotate to seek or adjust the
  player's own volume. On 1xDial, assign it **separately** to the Keypad for the screen
  and press, and to the Encoder for rotation; assigning either does not assign the other.
  Auto chooses the lexicographically first playing MPRIS service, then the first available
  service. A specific service can be selected in the action settings. With no matching
  player the screen shows a placeholder and controls do nothing.
- **Sensor** – temperatures, load, power, fans etc. from
  [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor).
  On Windows also **FPS and frame time of the focused window** from RivaTuner Statistics Server
  (e.g. bundled with MSI Afterburner) – works without LibreHardwareMonitor.

## Languages

The plugin speaks **English and Polish**: settings panels, texts drawn on the screens
(volume overlay, clock date…) and sensor messages. The language follows the package you
install and can be changed in any action's settings (**Język / Language**: Auto / Polski /
English – shared by all actions). Releases contain two packages that differ only in the action
names shown in OpenDeck's action list (OpenDeck reads them from the manifest and has no Polish
UI of its own):

- `io.github.opendeck-masterhub-en.streamDeckPlugin` – English names,
- `io.github.opendeck-masterhub-pl.streamDeckPlugin` – Polish names.

## Installation

All systems:

1. Install OpenDeck (2.14 or newer).
2. **Close the official Cooler Master MasterHUB app** if it's running – both would fight over
   the modules.
3. Download `io.github.opendeck-masterhub-en.streamDeckPlugin` (or `-pl`) from
   [Releases](../../releases) and in OpenDeck choose **Plugins → Install from file**.
4. The modules show up in OpenDeck's device list within a few seconds.

### Windows (10 / 11)

Nothing else is needed. The "Sensor" action needs LibreHardwareMonitor with its web server
enabled (**Options → Remote Web Server → Run**, port 8085); **Options → Run On Windows Startup**
is handy too.

### Linux (x86_64, glibc 2.34+: Ubuntu 22.04+, Debian 12+, Fedora 35+, Arch…)

1. **USB access** – without it a regular user can't open the modules. The rule file is in the
   plugin folder (`~/.config/opendeck/plugins/io.github.opendeck-masterhub.sdPlugin/`,
   for the Flatpak version `~/.var/app/me.amankhanna.opendeck/config/opendeck/plugins/…`):

   ```
   sudo cp 70-masterhub.rules /etc/udev/rules.d/
   sudo udevadm control --reload-rules && sudo udevadm trigger
   ```

   Then unplug and replug the MasterHUB.
2. Programs used by the actions (usually already installed):
   - `pactl` (package `pulseaudio-utils`, works with PipeWire too) – "Volume",
   - the "focused window" volume target:
     - on X11 and XWayland windows: `xdotool`,
     - on Wayland: Hyprland (`hyprctl`) and Sway (`swaymsg`) work out of the box; KDE Plasma: `kdotool`; GNOME Shell: via D-Bus (`gdbus`),
   - `xdg-open` – opening files / URLs.
3. "Sensor" reads `/sys/class/hwmon` (temperatures, fans, power), CPU load and used memory –
   no extra programs.

Keyboard shortcuts work on X11 and natively on Wayland (Hyprland, Sway, KDE Plasma). On GNOME,
a fallback via `ydotool` is available (requires `ydotoold` daemon and `/dev/uinput` permissions). OpenDeck from Flatpak
is supported – the plugin runs host programs through `flatpak-spawn --host`.

**Now Playing on Linux** uses the native session D-Bus. KDE Plasma/Wayland needs no
keyboard injection for this action, but the media app must expose
`org.mpris.MediaPlayer2.*` on the same user session bus. When OpenDeck runs as a Flatpak,
the sandbox may hide other players; check `flatpak info --show-permissions
me.amankhanna.opendeck` and, if necessary, grant only MPRIS access with
`flatpak override --user --talk-name=org.mpris.MediaPlayer2.* me.amankhanna.opendeck`,
then restart OpenDeck. The ID may differ for another build. Flatpak also limits
local artwork paths; a `file://` URL that is invisible inside its sandbox falls back to
the music icon. HTTP(S) artwork needs network access. See the
[Flatpak session-bus permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)
and [MPRIS specification](https://specifications.freedesktop.org/mpris/latest/).
Windows and macOS builds keep the action visible but do not provide an MPRIS backend.

The settings include an accent colour and a font family. Japanese text uses a CJK
fallback font when the selected font lacks the glyphs; on Arch/CachyOS install
`noto-fonts-cjk` if needed. New fonts are detected after restarting the plugin.

To check the action on hardware, build/package and install the updated plugin, restart
OpenDeck, then assign **Now Playing** to the 1xDial Keypad and Encoder separately.
Start an MPRIS player (for example a Chromium media tab), check artwork/title/progress,
pause and resume with the dial press, and turn the dial in both rotation modes. Repeat
with a 15xKey key. In the settings select a specific player, then close it, switch
profiles, and rotate a different action that shows the volume overlay; the old artwork
must not return and the widget must reappear after the overlay. Check
`masterhub.log` for D-Bus or artwork errors. This repository change has not been
verified on a physical MasterHUB.

### macOS (11+, Apple Silicon and Intel)

- "Volume" controls only the system and microphone volume – macOS doesn't allow per-app volume.
- Keyboard shortcuts need permission: **System Settings → Privacy & Security → Accessibility** –
  add OpenDeck.
- "Sensor" doesn't work (macOS doesn't expose sensors without extra software).
- macOS may block the unsigned plugin binary on first start – then use
  **System Settings → Privacy & Security → "Open Anyway"**.
- The macOS build hasn't been tested with real hardware yet.

## Good to know

- Rotation directions and roller order are set up after the unit the plugin was written on.
- The plugin doesn't update module firmware.
- Plugin log: `masterhub.log` in the plugin folder
  (Windows: `%APPDATA%\opendeck\plugins\io.github.opendeck-masterhub.sdPlugin\`).

## Building

Requires [Rust](https://rustup.rs) (1.89+, matching the locked dependencies) and Python 3 for packaging. Code comments are in Polish.

| System | Command | Output |
|---|---|---|
| Windows | `cargo build --release` | `target/release/opendeck-masterhub.exe` |
| Linux / WSL | `sh build-linux.sh` | `dist/bin/opendeck-masterhub-linux` |
| macOS | `sh macos/build.sh` (universal binary) | `dist/bin/opendeck-masterhub-mac` |

`python package.py` then bundles whatever binaries exist into
`dist/io.github.opendeck-masterhub-{en,pl}.streamDeckPlugin` (`--lang en` / `--lang pl` for just one).
On Windows `install.ps1` builds and installs straight into OpenDeck (close OpenDeck first);
the action names follow the Windows display language, `-Lang en` / `-Lang pl` overrides it.
Other systems: `python3 package.py --install --lang en`.
GitHub Actions builds all three systems on every push; pushing a `v*` tag publishes a release.

Translations: panel texts are in the HTML (Polish) with English in `data-en` attributes
(`pi/i18n.js`), texts drawn by the plugin use `i18n::tr("polski", "English")`, and the Polish
action names live in `pl.json` (Stream Deck localization format).

`examples/` contains small hardware and rendering test tools (`cargo run --example …`).

## Credits and license

MIT – see [LICENSE](LICENSE). The HID protocol is based on
[cooltek-sdk-probe](https://github.com/projectzero7/cooltek-sdk-probe) (MIT) plus own analysis
(dial screen, LEDs, GIFs). Cooler Master and MasterHUB are trademarks of Cooler Master
Technology Inc.
