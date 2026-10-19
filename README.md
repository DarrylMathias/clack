# ⌨️ clack

**Mechanical keyboard sounds for your whole machine.**  
*Feel and hear every keystroke anywhere you type on Linux and macOS.*

[![CI](https://img.shields.io/badge/CI-passing-brightgreen?style=flat-square&logo=github&logoColor=white)](https://github.com/DarrylMathias/clack/actions)
[![Release](https://img.shields.io/badge/release-v0.1.1-blue?style=flat-square&logo=github)](https://github.com/DarrylMathias/clack/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20macOS-lightgrey?style=flat-square&logo=linux&logoColor=white)](https://github.com/DarrylMathias/clack)
[![Latency](https://img.shields.io/badge/Latency-%3C6ms-brightgreen?style=flat-square&logo=speedtest&logoColor=white)](https://github.com/DarrylMathias/clack)
[![Privacy](https://img.shields.io/badge/Privacy-100%25%20Keylogger--Free-blueviolet?style=flat-square&logo=shield)](PRIVACY.md)

```
  ____ _        _    ____ _  __
 / ___| |      / \  / ___| |/ /
| |   | |     / _ \| |   | ' / 
| |___| |___ / ___ \ |___| . \ 
 \____|_____/_/   \_\____|_|\_\
```

> ⚡ **Status: v0.1.1** — Featuring the new interactive Sound Studio, live in-TUI volume tuning (`-` / `+`), on-demand sound downloads, physical speaker isolation, and auto-service restart.

---

## Quick Start

### Installation

```sh
curl -fsSL https://github.com/DarrylMathias/clack/releases/latest/download/clack-installer.sh | sh
```

Or install from source:

```sh
cargo build --release
sudo cp target/release/clack /usr/local/bin/clack
```

### Interactive Sound Studio

Run `clack use` or `clack list` to open the interactive Sound Studio:

- **Arrow keys (↑ / ↓)**: Scroll through sound packs.
- **Space**: Download an uninstalled sound pack on-demand with live preview.
- **Enter**: Select and activate the keyboard sound pack.
- **`-` / `+`**: Adjust volume live with immediate audio feedback (auto-saved).
- **Type anywhere**: Instant auditory and visual feedback for every keypress.
- **Double-tap Ctrl**: Mute or unmute instantly anywhere on your system.

---

## Commands

```
clack [COMMAND]
```

### Everyday
| Command | Description |
|---|---|
| `clack use [pack]` | Switch keyboard sound, or open interactive studio |
| `clack list` | Browse all keyboard sounds in interactive studio (`--raw` for plain text) |
| `clack toggle` | Mute or unmute (or double-tap Ctrl anywhere) |
| `clack on` / `clack off` | Explicitly unmute or mute |
| `clack volume [0-100]` | Query or set playback volume with perceptual curve |
| `clack status` | Display service status, active pack, and volume |

### Sound Packs & Catalog
| Command | Description |
|---|---|
| `clack pack add <id\|url\|path>` | Install a sound pack from catalog, URL, or local archive |
| `clack pack remove <id>` | Delete a locally installed sound pack |

### Service & Maintenance
| Command | Description |
|---|---|
| `clack start` | Start the background daemon service |
| `clack stop` | Stop the background daemon service |
| `clack restart` | Restart the background daemon service |
| `clack doctor` | Diagnose configuration, audio sinks, and permissions (● status indicators) |
| `clack update` | Check for updates, self-upgrade clack, and auto-restart background daemon |
| `clack uninstall` | Remove clack binary, configuration, and services |

Run `clack --help` for detailed command documentation.

---

## Supported Sound Packs

`clack` comes with native support and auto-detecting importers for multiple formats:

- **Clack Native (`manifest.toml`)**: Multi-channel, multi-velocity, multi-variation sprite or separate WAV files.
- **Mechvibes packs**: Direct support and auto-import of Mechvibes v1 & v2 packs (`config.json`).
- **Bucklespring packs**: Auto-conversion of IBM Model M sound definitions.
- **Keyboardsounds packs**: Auto-conversion of `profile.yaml` sound profiles.
- **kbsim packs**: Support for classic kbsim packs (e.g. Alps SKCM Blue).

---

## Architecture & Real-Time Audio

- **Real-time Audio Engine:** Zero heap allocations, zero deallocations, and zero mutex locks in the high-priority audio callback (`render()`).
- **High Quality Resampling & Saturation:** Band-limited linear interpolation for clean sample rate conversion and smooth soft-limiting via hyperbolic tangent (`tanh`) to avoid harsh clipping during rapid bursts of typing.
- **Low Latency:** Default PipeWire buffer quantum of 256 samples at 48kHz (~5.3ms latency).
- **Precise Input Filtering:** Pure keyboard event capture using Linux `evdev` and macOS `CGEventTap`, with strict exclusion of mouse, touchpad, and relative pointing devices.

---

## Privacy Guarantee

clack reads key events strictly to select and trigger the matching sound effect.

- **No key codes, key names, or keystrokes are ever logged, persisted, or transmitted.**
- Verified continuously by automated privacy tests (`tests/privacy_guard.rs`).
- See [PRIVACY.md](PRIVACY.md) for our full privacy commitment.

---

## Project Documentation

- [`implementation.md`](implementation.md): Complete technical design specification and IPC protocol.
- [`CREDITS.md`](CREDITS.md): Sound pack creators, upstream licenses, and acknowledgments.
- [`LICENSE`](LICENSE): MIT License.
