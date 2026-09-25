# clack

**Mechanical keyboard sounds for your whole machine.** Type anywhere on Linux or macOS and hear a Cherry MX Blue, an IBM Model M, or a typewriter through your speakers.

> ⚠️ **Status: pre-alpha, not usable yet.** The core is written but has not produced sound end to end.
> Current work: a working local prototype (milestones M0–M2 in [`further.md`](further.md)).
> The first public release will be **v0.1.0**.

## How it will work

```sh
curl -fsSL https://github.com/DarrylMathias/clack/releases/latest/download/clack-installer.sh | sh
```

A one-time setup lets you pick your keyboard sound. After that, clack runs in the background and starts at login.

- **Double-tap Ctrl** anywhere to mute or unmute.
- **Linux:** any distro, X11 or Wayland (GNOME, KDE, Hyprland, Sway…), with or without systemd.
- **macOS:** Apple Silicon and Intel.
- **Windows:** not supported.

### Commands (planned)

| Command | What it does |
|---|---|
| `clack use <pack>` | Switch keyboard sound (downloads it if needed) |
| `clack list` | Show all keyboard sounds |
| `clack toggle` / `on` / `off` | Mute / unmute |
| `clack volume [0-100]` | Get or set volume |
| `clack status` | Show what clack is doing |
| `clack start` / `stop` / `restart` | Control the background service |
| `clack pack add <id\|url\|path>` | Install a pack, including Mechvibes packs |
| `clack doctor` | Find and fix problems |
| `clack update` / `uninstall` | Update or remove clack |

Run `clack --help` for everything.

## Building from source (developers)

Requirements: a Rust toolchain, plus on Linux the ALSA headers (`alsa-lib` on Arch, `libasound2-dev` on Debian/Ubuntu).

```sh
cargo build --release
./target/release/clack --help
```

The prototype steps (installing the udev rule, importing Mechvibes packs, running the daemon) are tracked in [`progress.md`](progress.md) until `clack setup` automates them.

## Privacy

clack reads which key was pressed only to pick a sound. **Nothing is logged, stored, or sent.** See [PRIVACY.md](PRIVACY.md).

## Project docs

- [`implementation.md`](implementation.md): the design spec
- [`further.md`](further.md): the task list and milestones
- [`progress.md`](progress.md): the work log and handover notes

## Credits and license

The code is MIT licensed ([LICENSE](LICENSE)). The sound packs come from open-source projects under MIT, CC0 and GPL licenses; see [CREDITS.md](CREDITS.md).
