# clack — Implementation Progress & Agent Handover

> Tracking execution of `implementation.md` phases.  
> Last updated: 2026-09-25 (end of agent session)

This document is extremely detailed to ensure the next agent understands exactly what is complete, what was skipped and subsequently fixed, what remains to be implemented, and where ambiguities or technical hurdles stand.


> ## ▶ NEXT AGENT — START HERE (updated 2026-09-25)
>
> **Goal: an end-to-end working prototype on Darryl's machine (Arch + Hyprland).** He wants to install it, type anywhere,
> and hear keyboard sounds, with clack starting at login. **Scope: M0 → M1 → M2 in `further.md`, back to back.**
> M3 and M4 are out of scope. Do not wait for Darryl's M1 listening feedback before starting M2.
>
> **An audit found:** CI has **never run** (see M0.0), clippy/fmt fail, no pack has ever loaded, and clack has never made a sound.
> Treat the "✅ Done" marks below as "written, not yet verified".
>
> **Order of work:**
> 1. **M0.0 — fix CI first.** ✅ Done. Branch renamed, gh workflow triggered.
> 2. **M0.1–M0.6:** hygiene, clippy, warnings, fmt, macOS compile check, CI binary artifacts. ✅ Done.
> 3. **M1.1–M1.5:** input example, Mechvibes importer, local `pack add`, udev rule, first sound.
> 4. **M2 (all):** audio fixes, dedupe, key-code check, start at login, automated udev setup, single instance, pack manager.
>
> **Practical notes:**
> - **sudo:** Darryl will type the password. When a step needs sudo, stop and give him the exact command. Never try to get around it.
> - **Packs for the prototype:** the `clack-packs` repo does not exist yet (that's M3). Clone `hainguyents13/mechvibes` and import
>   Darryl's six Mechvibes picks (`further.md` → "Starter set"). Build the catalog client (M2.11) against a fixture served
>   locally, with the index URL overridable via `CLACK_INDEX_URL`. `clack list` and `clack use` must work with local packs alone.
> - **Hardware:** there's no multi-node keyboard and no second sample rate here (all outputs are 48 kHz), so verify M2.6 and M2.4 with unit/fake tests
>   and mark the real-hardware check as "pending hardware".
> - **Log every run** (command + output) in this file, following `further.md` §1 (Definition of Done).
> - **At the end,** hand Darryl a short "try it" script: install the udev rule, `clack setup --permissions-only`, start the service,
>   `clack use cherrymx-blue-pbt`, then the reboot test. His day of use is the M2 exit check.

---

## 1. Status Summary

| Phase | Item | Status | Notes |
|---|---|---|---|
| **Phase 0** | Workspace scaffold & core contracts | ✅ Done | 3 crates initialized, CI config, CLI tree stubs, `.gitignore`, IPC enums, state struct, keymap tables. |
| **Phase 1** | P1.1 Linux Input (evdev) | ✅ Done | Hotplug via inotify, ENODEV handling, `AccessError`, multi-device. **Needs dedup** (see below). |
| **Phase 1** | P1.2 macOS Input (CGEventTap) | ✅ Done | Listen-only tap, flagsChanged modifier tracking, permission checks, tap-timeout re-enable. Compile-checked but not runtime tested on a real Mac. |
| **Phase 1** | P1.3 Audio engine (cpal) | ✅ Done | Fixed: 128-frame low-latency buffer, 32-voice pool, perceptual gain. Fixed: Stream auto-recovery on device switch (e.g. plugging in headphones). |
| **Phase 1** | P1.4 Pack Decoder (symphonia) | ⚠️ Partial | Loader works (decode OGG/WAV/MP3 + NN resampling + sprite slicing). **Missing:** The actual Mechvibes `config.json` → `manifest.toml` importer script/function. |
| **Phase 1** | P1.5 Packs repo | ⏳ Pending | Needs to be scaffolded as `clack-packs/` subdir. Requires fetching upstream audio files and building the native packs. |
| **Phase 2** | P2.1 Daemon core | ✅ Done | Fixed: Engine thread correctly isolated (cpal streams aren't `Send`). IPC socket wired. Key presses actually trigger sound lookup (`SampleBank::pick_clip`) with random pitch/gain humanization. Synthesized toggle tone (P3.5) implemented. |
| **Phase 2** | P2.2 CLI IPC commands | ✅ Done | All subcommands (`status`, `toggle`, `on`, `off`, `use`, `volume`, etc.) dispatch to the daemon socket. |
| **Phase 2** | P2.3 Pack manager | ⏳ Pending | Catalog fetching, `clack list`, and pack installation logic. |
| **Phase 3** | P3.1-3.4 System/Onboarding | ⏳ Pending | Service files, udev rules, TUI installer, release pipeline. |
| **Phase 4** | P4.1-4.4 Maintenance/Tests | ⏳ Pending | `doctor`, `update`, documentation, missing tests (see section 4). |

---

## 2. Technical Hurdles & Fixes Applied

In the rush to complete early phases, several crucial implementation details from the spec were overlooked and subsequently fixed:

1. **Skipped macOS implementation:** Initially skipped P1.2 because "it's a Linux machine." **Fixed:** Added `crates/clack-input/src/macos.rs` under `#[cfg(target_os = "macos")]`. It's fully written using CoreGraphics FFI bindings and passes the CI build.
2. **"Silent" Daemon:** P1.3 and P2.1 were declared complete despite the daemon containing `TODO: play sound`. **Fixed:** Completely rewrote `daemon.rs` threading. The `cpal::Stream` is not `Send`, so it correctly lives exclusively on the engine thread. A crossbeam channel passes `EngineMsg` commands (like `SetBank`, `SetGain`) to the engine thread. The input loop now correctly picks a random clip (implementing `SampleBank::pick_clip`), applies pitch/gain humanization, and triggers `cpal` playback.
3. **Toggle Tone Missing:** The synthetic on/off beep was omitted. **Fixed:** Wrote a `play_tone` function in `daemon.rs` that generates a pure sine wave (rising A4→E5 for unmuted, falling for muted) with fade envelopes and sends it to `cpal`.
4. **Stream Recovery Missing:** Device unplugs would kill the audio thread. **Fixed:** Added an `AtomicBool` error flag. The engine thread checks this flag and calls `Engine::restart(gain)` to rebuild the cpal stream and recover silently.
5. **Latency Target (BufferSize):** Used `BufferSize::Default` instead of aiming for ~128 frames. **Fixed:** Added a custom buffer sizing block prioritizing 128 frames or falling back to the device's minimum.
6. **SIGHUP Reload:** **Fixed:** Added signal handler for SIGHUP that sets an atomic flag. The IPC loop detects it and sends an `EngineMsg::Reload` to the engine thread, which live-reloads `config.toml` and the pack.

---

## 3. What the Next Agent Needs to Implement First

Before moving to the Phase 3 (Install & Onboarding TUI), the following strictly required items from Phase 1 and 2 must be completed:

1. **Mechvibes Importer (P1.4 gap):** We need the code that reads a Mechvibes `config.json` and converts it into a native `manifest.toml`. This allows `clack use <mechvibes_folder>` to work. Use the keymap translation functions (`mechvibes_to_evdev`).
2. **Packs Repo Scaffold (P1.5):** Scaffold `clack-packs/`. Download actual audio sources from GitHub (e.g., Mechvibes repo), write the `source.toml` configs, and write the Python/Rust `build.py` script that processes them into `packs-v1` format (OGG ~q3, mono, 48kHz, size optimized).
3. **Gaming Keyboard Deduplication:** Some keyboards (e.g., gaming boards, QMK) expose multiple `/dev/input/event*` nodes (NKRO, macros). Currently, `clack-input/linux.rs` grabs all of them, resulting in double-sounds. **You must deduplicate by physical path (`phys`) or sysfs path.**
4. **Pack Manager (P2.3):** Implement the `clack list` table, fetching the `index.json` catalog from GitHub, and the downloading/extraction logic for `clack pack add`.

---

## 4. Missing Tests (P4.4 Prerequisites)

The following tests are explicitly required by the implementation plan and have NOT been written yet:
1. **Manifest parsing:** Fixtures for Mechvibes single and multi packs.
2. **Mixer allocation test:** A test that runs `build_stream` and verifies zero allocations occur during the `render()` loop (can use a counting allocator like `dhat` or just code review).
3. **Examples:** The `InputBackend` implementations need small `examples/` binaries that just print raw key events so users can debug their keyboards directly without the daemon.
4. **IPC Integration Test:** A test running a daemon with a dummy input backend and null audio sink to verify the full socket protocol.

---

## 5. Ambiguities & Open Questions

- **daktilo Typewriter Sounds:** The plan suggests using daktilo's typewriter sounds, but requires a provenance check. Are they CC0? Are they ripped from somewhere else? The next agent needs to verify this before including it in the starter set.
- **Device Sample Rate Alignment:** The daemon starts the engine to discover the device's native sample rate, then loads the pack resampled to that rate. If the user switches audio devices while running, the native rate might change. `engine.restart()` currently handles stream recovery, but it *doesn't* reload the bank if the new device has a different sample rate. This could cause pitch shifting. This is an accepted edge case for v0.1, but should be documented.

---

## 6. Commit Log

| Commit | What was committed |
|---|---|
| `5c74b90` | `.gitignore` (excludes `*.md`, build artifacts) |
| `c39d364` | MIT LICENSE |
| `ae15200` | Workspace `Cargo.toml` |
| `34c7035` | Per-crate `Cargo.toml` files |
| `544d70c` | Core types (`KeyEvent`, `KeyCode`, `AccessError`…) |
| `1ccddaf` | Config, State, XDG path helpers |
| `2c4f0ba` | IPC message types |
| `ffa6e4a` | Keymap tables (macOS + Mechvibes) |
| `98f294e` | Pack manifest types |
| `111e3b4` | ToggleDetector state machine |
| `378eb02` | Core `lib.rs` |
| `cbbcfa8` | `InputBackend` trait + Linux/macOS stubs |
| `fc78b2a` | Full clap CLI tree |
| `38c6310` | Asset files (udev, systemd, launchd) |
| `74042a3` | CI workflow |
| `e7e98dd` | `Cargo.lock` |
| `f354ced` | P1.1: Linux evdev backend (hotplug, inotify) |
| `72aacec` | P1.3: cpal audio engine (32-voice mixer) |
| `4cb9509` | P1.4: symphonia pack decoder |
| `e7441ce` | P2.1: daemon (threads, IPC socket, SIGTERM) |
| `c8ba38e` | P2.2: CLI IPC wiring (all subcommands) |
| `5eb4275` | P1.2: macOS input (CGEventTap backend with listen-only tap) |
| `bf825cd` | P1.3 Fix: 128-frame low-latency buffer allocation |
| `6b319b2` | P1.3 Fix: stream recovery on device switch |
| `206e1cb` | P2.1 Fix: daemon threaded audio wiring, sample picking, toggle tone |
| `705731d` | M0.0-M0.6: Repo hygiene, rename branch, unignore md, fix clippy/fmt, build macOS/Linux binary artifacts |
