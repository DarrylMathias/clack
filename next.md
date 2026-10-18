# clack — Next Agent Handover

> Written: 2026-10-18 after completing M1, M2, and the mouse-click input bug fix.
> Read this file first. Then `progress.md` (history), `further.md` (detailed task specs), `implementation.md` (the spec).
> **§ references** point to `implementation.md`.

---

## ▶ START HERE

The app is **working on Darryl's machine (Arch + Hyprland)**:

- Typing anywhere plays keyboard sounds ✅
- Mouse clicks and touchpad taps are correctly **excluded** ✅ (fixed 2026-10-18)
- Service autostarts at login via `~/.config/systemd/user/clack.service` ✅
- Pack manager (`clack list`, `clack pack add`, catalog fetch) is implemented ✅
- The service binary lives at `~/.local/bin/clack`

**What's missing:** the packs repo, onboarding TUI, `clack doctor`, `clack update`, `clack uninstall`, the release pipeline, and tests. **Your job is to implement all of it.**

Work through the milestones **in order**. M3.1 (packs repo) and M3.4 (onboarding TUI) are the most impactful — do those first.

---

## Rules (non-negotiable, from `further.md §1`)

1. **Run it, don't just compile it.** Record the command + output (or what you heard) in `progress.md`. "✅ Done" without a run is dishonest.
2. `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all` must all pass after every milestone.
3. **New logic = unit tests.** I/O = integration test or documented manual test in `progress.md`.
4. **Never log key codes.** Not even at debug level. The CI grep test (T-X1) will catch it.
5. **Audio callback is sacred.** No allocations, frees, locks, or log calls inside `render()`.
6. **One logical change per commit**, conventional-commit style, referencing the task ID (e.g. `feat(packs): scaffold clack-packs repo (M3.1)`).
7. **Update `progress.md` honestly** after each milestone. Include run output and any manual test notes.
8. **Steps needing `sudo` or a reboot:** stop and give Darryl the exact command to run. Don't block; move to the next parallelisable task while waiting.

---

## Current State Summary

| Area | Status |
|---|---|
| Phase 0 — scaffold, CI, contracts | ✅ Done |
| Phase 1 — Linux/macOS input, audio engine, pack decoder, Mechvibes importer | ✅ Done |
| Phase 2 — daemon, CLI IPC, pack manager | ✅ Done |
| Bug fix: mouse/touchpad exclusion (evdev `EV_REL` filter) | ✅ Done 2026-10-18 |
| Phase 3 — packs repo, onboarding TUI, service install, release | ⏳ **Your work** |
| Phase 4 — doctor, update, uninstall, docs, test matrix, v0.1.0 | ⏳ **Your work** |

### Known open issues / technical debt

| Issue | Where |
|---|---|
| `render()` drops `Arc` inside the audio callback (M2.1) | `mixer.rs:244` — see M2.1 below |
| Randomness uses `SystemTime` nanoseconds (M2.2) | `mixer.rs:66` |
| Nearest-neighbour resampling (audible aliasing on 44.1 → 48 kHz) (M2.3) | `decoder.rs` |
| No reload when sample rate changes after device switch (M2.4) | `engine.rs` |
| No soft limiter; fast typing at high volume can crackle (M2.5) | `mixer.rs` |
| Key-code accuracy for arrows, numpad, right-side modifiers unverified (M2.7) | `keymap.rs` |

Fix **M2.1 through M2.5** before or alongside M3 audio work — they affect sound quality.

---

## Milestone M3: v1 Features

### M3 Audio engine fixes (do these first, before the packs repo)

These are from `further.md` M2.x but are still outstanding.

#### M2.1 — Stop freeing memory in the audio callback
- **File:** `crates/clack-core/src/mixer.rs:244`
- **Problem:** `v.clip = clip` drops the old `Arc` *inside* the callback — the last reference, so memory frees in the audio thread.
- **Fix:** add a return `rtrb` ring (callback → engine thread). Push replaced/finished `Arc`s into it instead of dropping them. Engine thread drains and drops them. If the ring is full, `std::mem::forget` instead of dropping. Also change `Voice::default()` to use `Option<Arc<…>>` instead of `Arc::new(Vec::new())`.
- **Done when:** test T-M3 shows 0 allocations and 0 frees in `render()` across a pack swap (use a counting global allocator).

#### M2.2 — Proper randomness
- **File:** `crates/clack-core/src/mixer.rs:66`
- **Problem:** clip index picked from `SystemTime` nanoseconds — biased and not truly random.
- **Fix:** replace with `fastrand` (or a small xorshift) living on the engine thread. Use it for pitch/gain variation too. Change `pick_clip` signature to `pick_clip(&mut Rng)`.
- **Done when:** unit test — 10k picks over 5 clips are roughly uniform.

#### M2.3 — Linear resampling
- **Files:** `decoder.rs`, `mixer.rs`
- **Problem:** `decoder.rs` resamples nearest-neighbour; `render()` reads `voice.pos as usize` (also nearest). Audible aliasing on 44.1 → 48 kHz.
- **Fix:** switch both to linear interpolation. Keep callback allocation-free.
- **Done when:** unit test — resampling a 1 kHz sine from 44.1 → 48 kHz keeps the frequency within 1%. Clicks sound clean by ear.

#### M2.4 — Reload pack on sample-rate change
- **File:** `crates/clack-core/src/mixer.rs` (Engine::restart)
- **Problem:** `Engine::restart()` rebuilds the cpal stream but doesn't reload the bank if the new device's sample rate differs → pitch shift.
- **Fix:** after restart, if `sample_rate` changed, reload the bank at the new rate (engine thread, off the audio path).
- **Done when:** manual test — switch between two outputs with different rates → no pitch shift. Record in `progress.md`.

#### M2.5 — Soft limiter
- **File:** `crates/clack-core/src/mixer.rs`
- **Problem:** voices are summed then hard-clamped. Fast typing at high volume crackles.
- **Fix:** add a `tanh`-style soft limiter or a simple peak compressor after the mix sum.
- **Done when:** manual test — mash 10 keys at volume 100, no crackle.

---

### M3.1 — Scaffold the `clack-packs` repo (separate GitHub repo: `DarrylMathias/clack-packs`)

> Spec: `implementation.md §6.3`, `further.md M3.1`

This is a **separate repository**, not inside this one.

**Structure:**
```
clack-packs/
├── packs/<id>/source.toml      # upstream URL, pinned commit, license, notes
├── build/                      # build tool (Rust binary or Python script)
│   └── src/main.rs             # fetches upstream → converts → re-encodes → packages
├── dist/                       # generated: <id>.tar.gz files + index.json
├── .github/workflows/
│   └── build.yml               # publishes packs-vN release on tag
├── CREDITS.md
└── LICENSE
```

**`source.toml` format:**
```toml
id = "cherry-mx-blue-pbt"
name = "Cherry MX Blue (PBT)"
family = "clicky"
license = "MIT"
upstream_url = "https://github.com/hainguyents13/mechvibes"
upstream_commit = "<pinned commit hash>"
format = "mechvibes"           # mechvibes | bucklespring | keyboardsounds | kbsim | native
notes = ""
```

**Build tool responsibilities:**
1. Fetch upstream at the pinned commit.
2. Convert using the appropriate importer (reuse `clack-core`'s Mechvibes importer as a library, or call the binary).
3. Trim silence from start/end of each clip (threshold ~-50 dBFS).
4. Re-encode to **OGG Vorbis quality 3, mono, 48 kHz** (use `ffmpeg` or the `vorbis` crate). Target ≤ 1 MB per pack.
5. Write `manifest.toml` and copy `LICENSE`.
6. Produce `<id>.tar.gz` and update `index.json` (id, name, family, license, starter, size, sha256, url, blurb).

**GitHub Action:** on tag `packs-vN`, run the build tool for all packs, create a release, upload tarballs + `index.json`.

**Starter set to build (Darryl's picks from `further.md`):**
| Pack ID | Source | Format | License |
|---|---|---|---|
| `cherry-mx-blue-pbt` | mechvibes | mechvibes | MIT |
| `cherry-mx-blue-abs` | mechvibes | mechvibes | MIT |
| `cherry-mx-brown-pbt` | mechvibes | mechvibes | MIT |
| `cherry-mx-brown-abs` | mechvibes | mechvibes | MIT |
| `cherry-mx-black-abs` | mechvibes | mechvibes | MIT |
| `cream-travel` | mechvibes | mechvibes | MIT |
| `ibm-model-m` | bucklespring | bucklespring | GPL-2.0 |
| `alps-skcm-blue` | kbsim `bluealps` | kbsim | MIT |
| `electric-typewriter` | keyboardsounds `typewriter` | keyboardsounds | GPL-3.0 |
| `zeal-tealios-v2` | keyboardsounds `telios-v2` | keyboardsounds | GPL-3.0 |

**Kailh Box White:** no known license — skip for now. Document in CREDITS.md that it's excluded.

**Additional importers needed (beyond Mechvibes, which already exists):**
- **Bucklespring:** 209 per-key WAVs named `<hex_evdev_code>-<0|1>.wav` (e.g. `1e-0.wav` = KEY_A down, `1e-1.wav` = KEY_A up). Map directly to a multi-file `manifest.toml`.
- **kbsim:** has its own layout file. Parse it and map to evdev codes via `keymap::mechvibes_to_evdev` (kbsim uses the same VC_* codes as Mechvibes).
- **keyboardsounds:** `profile.yaml` + WAVs (`key1..6.wav`, `space.wav`, `ent.wav`, `back.wav`). Map named files to `press.default` (key1–6 as the array), `press.57` (space), `press.28` (enter), `press.14` (backspace).

**Done when:** `index.json` plus all 10 tarballs are published at a real GitHub release URL. Every pack loads through `decoder::load_pack` in CI.

---

### M3.2 — License verification + CREDITS.md

> Spec: `further.md M3.2`

- **daktilo:** we are NOT using daktilo sounds (Darryl replaced it with `electric-typewriter` from `keyboardsounds`). The daktilo provenance check is **no longer blocking**. Record this decision in `CREDITS.md`.
- For every pack in the starter set: verify the upstream license matches what's listed in `source.toml`. Record in `CREDITS.md`:
  ```
  ## <Pack Name>
  Source: <upstream URL>
  Author: <upstream author>
  License: <SPDX identifier>
  Notes: <any conversion or provenance notes>
  ```
- Add a takedown contact to both `CREDITS.md` and `README.md`: `takedown@<email or GitHub issue link>`.
- **Done when:** every published pack has a `LICENSE` file inside its tarball and a `CREDITS.md` entry.

---

### M3.3 — Build all packs

> Spec: `further.md M3.3`, `implementation.md §6.3`

After the build tool and importers are working (M3.1):
- Build all 10 starter packs.
- Build remaining packs from mechvibes (the other 12), kbsim, keebsound, Monkeytype, and keyboardsounds.
- Each must be **≤ 1 MB**. Total starter set ≤ ~11 MB.
- Each pack loads cleanly through `decoder::load_pack` in CI (a `#[test] #[ignore]` that runs against the local dist/).
- **Done when:** Darryl gives listening sign-off. Record feedback in `progress.md`.

---

### M3.4 — `clack setup` onboarding TUI

> Spec: `implementation.md §3.2`, `further.md M3.4`

**File:** `crates/clack/src/setup.rs`

This is the most user-facing feature. When the user first runs `clack` with no config, it runs setup automatically.

**Steps (in order):**

**Step 1 of 4 — Keyboard access**
- Linux: run `clack setup --permissions-only` (M2.9, already implemented in `perms.rs`). Show progress and confirm access works.
- macOS: print a message explaining Input Monitoring. Poll `CGPreflightListenEventAccess()` every 500ms until granted (or user presses S to skip).

**Step 2 of 4 — Pick your keyboard**
- Use `crossterm` raw mode to read keys **from the terminal** (not evdev). This works before the udev rule is in place.
- Show a scrollable list of the starter packs. Download all 10 in parallel in the background using `std::thread` + `ureq`. Show a per-row progress indicator (e.g. `████ ready` / `██░░ 47%` / `░░░░ waiting`).
- While a row is highlighted, auto-play a short ~2-second clip demo (a pre-baked burst, or use the IPC `use` + `toggle` trick).
- Show a text box labelled "try it" — keys typed there play the highlighted pack live via IPC.
- Press Enter to confirm selection.

**Step 3 of 4 — Start at login**
- Call the service management code (M2.8, `service.rs`) to write and enable the unit.
- Print the result (e.g. `✓ systemd user service enabled`).
- If the WM doesn't use systemd user services (detect Hyprland, Sway, i3 by `$XDG_CURRENT_DESKTOP` or `$WAYLAND_DISPLAY`+`$SWAYSOCK` etc.): also print `Add this to your compositor config: exec-once = clack start`.

**Step 4 of 4 — Done**
- Print the done message. Show the double-tap Ctrl hint and `clack --help`.

**TUI implementation notes:**
- Use `crossterm` (already in `Cargo.toml`). Add `ratatui` if the layout gets complex.
- The picker must handle terminal resize gracefully.
- `clack setup` is idempotent — re-running it skips already-completed steps unless `--reset` is passed.
- `clack setup --reset` redoes everything from scratch (deletes `~/.config/clack/` and `~/.local/share/clack/packs/` after confirming with the user).

**Done when:** manual run on the dev machine after deleting `~/.config/clack` and `~/.local/share/clack`. Record the full run in `progress.md`. Unit tests for the step state machine.

---

### M3.5 — `--help` polish

> Spec: `implementation.md §3.4`

Update the clap definitions to match the **exact** target output in `implementation.md §3.4`:
- Use `help_template` for the custom layout with grouped headings (EVERYDAY / SERVICE / PACKS / MAINTENANCE / EXAMPLES / FILES / PRIVACY).
- Use `next_help_heading` for subcommand groups.
- Every subcommand has its own `--help` with at least one example.

**Done when:** snapshot test (use `insta`) of `clack --help` and `clack use --help` passes. (T-S1)

---

## Milestone M4: Maintenance Commands, Docs, Release

### M3.6 — `clack doctor`

> Spec: `implementation.md §5.11`, `further.md M3.6`

**File:** `crates/clack/src/doctor.rs`

Print a `✓` / `✗` / `!` line for each check, each with the **exact command** to fix it:

| Check | How |
|---|---|
| Binary on PATH, version correct | `which clack`, parse `--version` |
| Newer version available | hit GitHub releases API |
| Config parses cleanly | `Config::load()`, report the error line |
| Active pack exists and manifest is valid | `decoder::load_pack()` |
| Service installed and running | `systemctl --user is-active clack` (Linux) / `launchctl list dev.clack` (macOS) |
| IPC responds | open the socket, send `{"cmd":"status"}` |
| **Linux input access** | `InputBackend::probe()` — if none: check udev rule present, `getfacl /dev/input/eventN` for uaccess tag, logind present, NixOS detection |
| **macOS input access** | `CGPreflightListenEventAccess()` — if missing: print the settings URL |
| Audio device | try to open the default cpal output; show latency estimate |
| Autostart (WM hint) | detect WMs that ignore XDG autostart and print `exec-once = clack start` |

**Done when:** unit tests for each check using fakes. Manual run showing (a) healthy output and (b) output with the udev rule removed. Record both in `progress.md`.

---

### M3.7 — `clack update`

> Spec: `implementation.md §5.12`, `further.md M3.7`

**File:** `crates/clack/src/update.rs`

- Use `axoupdater` (cargo-dist's updater): check latest GitHub release → download binary → verify checksum → atomically replace binary → restart service.
- Also refresh the pack catalog after updating.
- **macOS:** warn the user *before* updating that Input Monitoring will reset. After the update, walk them through re-enabling it (same flow as setup step 1). Offer to open System Settings directly.

**Done when:** manual test — update from a `v0.0.1-test` pre-release to a newer one. Record in `progress.md`.

---

### M3.8 — `clack uninstall`

> Spec: `implementation.md §5.13`, `further.md M3.8`

**File:** `crates/clack/src/uninstall.rs`

Steps:
1. Stop the service (`clack stop`).
2. Remove the service unit and disable it (`systemctl --user disable clack` / `launchctl bootout`).
3. Remove the udev rule and reload udev (Linux only — requires `sudo`; hand the command to Darryl).
4. Remove the binary (`~/.local/bin/clack`).
5. Prompt: `Delete packs and config too? [y/N]`. If yes: remove `~/.config/clack/` and `~/.local/share/clack/`.

**Done when:** manual test on a spare terminal — `clack uninstall --yes` leaves no files except optionally the data dir. Record in `progress.md`.

---

### M3.9 — macOS testing

> Spec: `further.md M3.9`

**This is done by a friend of Darryl's, not by an agent.** But the agent should:
1. Make sure the CI binary from M0.6 (already implemented) is downloadable.
2. Prepare a checklist (add it to `progress.md`) for the Mac tester to fill in:
   - macOS version + Mac model
   - Input Monitoring prompt appeared and worked
   - Modifiers (Ctrl, Shift, Cmd, Option) triggered sounds
   - No duplicate sounds
   - Space, Enter, Backspace have dedicated sounds
   - launchd autostart survived a logout/login
   - Silence in password fields (expected)
   - `clack doctor` output (paste it)
3. Leave the checklist in `progress.md` with status `⏳ awaiting Mac tester`.

---

### M3.10 — launchd service (macOS)

> Spec: `implementation.md §5.7`, `further.md M3.10`

**File:** `assets/dev.clack.plist` (already exists as a template), `crates/clack/src/service.rs`

- Complete the `service.rs` implementation for macOS: write `~/Library/LaunchAgents/dev.clack.plist`, run `launchctl bootstrap gui/$UID ~/Library/LaunchAgents/dev.clack.plist`.
- `start`/`stop`/`restart` should use `launchctl kickstart`/`bootout`.
- **Done when:** survives a logout and login on a real Mac (covered under M3.9).

---

### M3.11 — cargo-dist release pipeline

> Spec: `implementation.md §7`, `further.md M3.11`

- Add `dist-workspace.toml` (cargo-dist config).
- Build targets:
  - `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` built against **glibc 2.28 or older** using `cargo-zigbuild`.
  - `aarch64-apple-darwin` and `x86_64-apple-darwin`.
- The installer (`clack-installer.sh`, generated by cargo-dist):
  1. Detects OS + arch.
  2. Downloads binary + verifies checksum.
  3. Puts it in `~/.local/bin/clack` and ensures PATH.
  4. Runs `clack setup`.
- Add a `.github/workflows/release.yml` (generated by cargo-dist) that triggers on tag `v*`.

**Done when:** a `v0.0.1-rc` pre-release installs cleanly with the single curl command on a fresh Ubuntu VM. Record in `progress.md`.

---

## Milestone M4: Verification and v0.1.0 Release

### M4.1 — Test matrix

> Spec: `implementation.md §8`, `further.md M4.1`

Test on each system below. For each: install → setup → sound → toggle → `use` another pack → reboot → `doctor` → `update` → `uninstall`.

| System | Why |
|---|---|
| Arch + Hyprland (Wayland) | dev machine — already works |
| Ubuntu 24.04 GNOME (Wayland) | most common |
| Fedora KDE (Wayland) | PipeWire, SELinux |
| Debian 12 + Xfce (X11) | X11 and older glibc |
| Void Linux (runit + elogind) | non-systemd path |
| NixOS | read-only `/etc` |
| macOS 14/15 Apple Silicon | TCC flow (done by Mac tester) |

Record a results table in `progress.md`. Every failure gets a GitHub issue. Fix all blockers before tagging.

---

### M4.2 — Latency measurement

- Add a `--measure` flag to `clack daemon` that logs aggregate p50/p95 from input `Instant` to buffer write delta. **No codes ever in those logs.**
- Run it on the dev machine. Record in `progress.md`.
- **Target:** p95 < 20 ms.

---

### M4.3 — Documentation

**README.md** — update to include:
- One-line install command (from M3.11).
- Privacy statement **at the top** (before features).
- A GIF or screenshot of `clack setup`.
- Full command table (§3.3).
- WM autostart snippets (Hyprland `exec-once`, Sway `exec`, i3 `exec`).
- How to add a Mechvibes pack (`clack pack add ~/Downloads/my-pack`).
- Troubleshooting section → `clack doctor`.

**CREDITS.md** — finalize (from M3.2).

**PRIVACY.md** — confirm it's accurate. It should state plainly: key codes are read only to select a sound; nothing is logged, stored, or transmitted.

**Done when:** Darryl reviews and gives thumbs-up. Record in `progress.md`.

---

### M4.4 — Release `v0.1.0`

1. Tag `v0.1.0`.
2. cargo-dist generates and uploads the release.
3. Smoke-test the installer on at least 2 systems from the M4.1 matrix immediately after publishing.
4. Check the README's install command works end-to-end.

---

## Test inventory — everything that must pass before v0.1.0

### Unit tests (in `cargo test`, CI on Linux and macOS)

| ID | What to test | Where |
|---|---|---|
| T-K1 | `keymap`: every ANSI/ISO key maps both ways for macOS `kVK` and Mechvibes codes; extended keys (arrows, numpad, right-side modifiers) correct | `clack-core/src/keymap.rs` |
| T-P1 | `Manifest` parsing: sprite mode, multi-file mode, arrays (random pick), missing `release`, invalid ranges rejected | `clack-core/src/pack.rs` |
| T-P2 | Mechvibes importer: `single` fixture, `multi` fixture, unknown codes skipped and counted, default clips derived | `clack-core/src/mechvibes.rs` |
| T-P3 | Decoder: loads OGG, WAV, MP3 fixtures; sprite slicing boundaries; linear resampling accuracy (M2.3) | `clack-core/src/decoder.rs` |
| T-M1 | Mixer: voice stealing when all 32 busy; gain curve; mono→stereo; soft limiter bounds | `clack-core/src/mixer.rs` |
| T-M2 | `pick_clip` distribution with seeded PRNG; falls back to `default` (M2.2) | `clack-core/src/mixer.rs` |
| T-M3 | **0 allocations and 0 frees in `render()`**, including across a bank swap — use counting global allocator | `clack-core/src/mixer.rs` |
| T-T1 | Toggle detector: valid double-tap; too slow; key in between; auto-repeat; left then right Ctrl; `toggle = none` | `clack-core/src/toggle.rs` |
| T-D1 | Input dedupe filter | `clack-input/src/linux.rs` |
| T-C1 | Config / state load/save roundtrip; missing file → defaults; bad TOML → clear error | `clack-core/src/config.rs` |
| T-L1 | Catalog: parse, cache freshness, offline fallback | `clack-core/src/catalog.rs` |
| T-S1 | `clack list` table snapshot; `clack --help` and `clack use --help` snapshots (use `insta`) | `crates/clack/` |

### Integration tests (`crates/clack/tests/`)

The daemon must accept injected backends (`InputBackend` trait) so tests need no hardware.

| ID | What to test |
|---|---|
| T-I1 | IPC round-trip: `status`, `toggle`, `set_muted`, `volume`, `use`, `reload`, `shutdown` return expected responses |
| T-I2 | Scripted key events → null sink receives audio; muted → silence; toggle tones play while muted |
| T-I3 | Single instance; stale-socket cleanup; SIGTERM exits in < 1 s |
| T-I4 | `pack add` from a local HTTP server (`tiny_http`): correct sha256 installs; wrong sha256 rejected, no leftover files; `use` auto-installs |

### Privacy guard (CI)

| ID | What |
|---|---|
| T-X1 | Script/test greps `crates/*/src` for `tracing`/`println` calls whose arguments contain `code`, `KeyEvent`, or `event`. Fails unless the line has `// privacy-ok: <reason>`. `examples/` excluded. |

---

## Decisions already made (don't re-litigate)

| Decision | Detail |
|---|---|
| Canonical keycode | Linux evdev `KEY_*` codes (D15) |
| Audio backend | `cpal` — wraps ALSA/PipeWire/Pulse/CoreAudio (D4) |
| Packs separate repo | `DarrylMathias/clack-packs` (D11) |
| GPL packs | Shipped as separate data files with their own LICENSE — "mere aggregation" keeps clack MIT (D12) |
| `use` auto-installs | If the pack isn't installed, run `pack add` first (D9) |
| First public release | `v0.1.0` |
| macOS testing | A friend of Darryl's, after M3 is complete, using the CI binary |
| Windows | Non-goal. Don't design against it, but don't build for it. |
| Filters on `clack list` | Non-goal. No filters. |
| Mouse click sounds | Non-goal (now also technically fixed — mice are excluded at the evdev level). |
| daktilo sounds | Not used. Replaced with `electric-typewriter` from `keyboardsounds`. |
| Kailh Box White | Excluded — no known license. Document in CREDITS. |

---

## Commit log (for context)

See `progress.md §6` for the full history. Most recent commits:

| Commit | What |
|---|---|
| `1f2eee4` | fix(audio): route to built-in speaker sink, idle close stream, service hardening |
| `586ecc4` | fix: privacy, audio routing, idle stream, service hardening |
| `d32452a` | feat(cli): implement pack manager (M2.11-M2.13) |
| `f00f140` | docs(progress): record audio fix |
| `7ec5ec6` | fix(linux): make inotify and evdev non-blocking |
| `ebd9ede` | feat: complete M2 through M2.10 |
| + mouse fix | fix(input): reject mice and touchpads by checking EV_REL axes |

---

*Good luck. Darryl wants to hear the IBM Model M.*
