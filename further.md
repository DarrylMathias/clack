# clack — What's Left: Road to a Working Local Build and v1

> Written 2026-09-25 after auditing the repo at commit `206e1cb`.
> Read together with `implementation.md` (the spec; § references point there) and `progress.md` (the log).
> **This file is the task list.** Work top to bottom. Milestones are sequential. Tasks inside a milestone marked ∥ can run in parallel.

---

## 0. Where we actually are

The code exists for Phases 0–2, but **clack has never produced a sound.** There are no packs on disk, no way to create one (no Mechvibes importer), and no example binaries. Nothing has been run end to end.

Audit results at `206e1cb`:

| Check | Result |
|---|---|
| `cargo build` | ✅ passes, 3 warnings |
| `cargo test` | ✅ 14 pass, all in `clack-core`. `clack-input` and `clack` have **0 tests**. |
| `cargo clippy --all-targets -- -D warnings` | ❌ 5 errors, so **CI is red** |
| `cargo fmt --check` | ❌ fails |
| macOS backend | ⚠️ never compiled on macOS (it's behind a `cfg` flag; the dev machine is Linux) |
| End-to-end sound | ❌ never tested |

---

## 1. Rules for every agent (Definition of Done)

A task is **done** only when **all** of these are true:

1. **It has been run, not just compiled.** Paste the command and its output (or a short description of what you heard) into `progress.md`.
2. `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test --all` all pass.
3. **Tests:** new logic has unit tests. Anything touching I/O has either an integration test or a documented manual test in `progress.md`.
4. **Privacy:** no key codes or `KeyEvent`s are ever logged or printed, except in the `examples/` debug binaries (§5.6).
5. **Real-time audio:** the audio callback never allocates, frees, locks, or logs (§4).
6. **Commits:** one logical change per commit, in conventional-commit style, referencing the § or the task ID from this file (e.g. `fix(mixer): … (F1.3)`).
7. **Honest status:** update `progress.md` **honestly**. "Written, untested" is a valid status; "✅ Done" without a run is not.

---

## Milestone M0: Repo hygiene (small, do first, one agent)

| ID | Task | Details | Done when |
|---|---|---|---|
| **M0.0** | **Get CI running** | CI has **never run**: `.github/workflows/ci.yml` triggers on `main`, but the repo's only branch is `master`. Darryl approved renaming the branch: `git branch -m master main`, then `gh api -X POST repos/DarrylMathias/clack/branches/master/rename -f new_name=main`, then `git fetch origin && git branch -u origin/main main`. The repo is **private**, so Actions minutes count against Darryl's quota; keep the matrix to ubuntu + macos. | `gh run list` shows a run on `main` with both the ubuntu and macos jobs (red is expected until M0.2–M0.4) |
| **M0.1** | Stop ignoring Markdown | Remove the `*.md` / `**/*.md` lines from `.gitignore`. Commit `implementation.md`, `progress.md`, `further.md`, `README.md`, `CREDITS.md`, `PRIVACY.md`. Keep README's "Status" section honest as milestones land. | `git ls-files '*.md'` lists all of them |
| **M0.2** | Make clippy pass | Errors at: `clack-core/src/config.rs:17` (derive `Default`), `decoder.rs:54` and `decoder.rs:66` (redundant closures), `mixer.rs:288` (index loop → `iter_mut().skip(2)`), `pack.rs:114` (struct init instead of field assignment). | clippy is clean with `-D warnings` |
| **M0.3** | Fix build warnings | `daemon.rs:105`: unused `eng_tx_clone` (remove it or use it). `daemon.rs:400-401`: fn-pointer-to-integer casts. Replace the hand-rolled `libc_signal` FFI with the `signal-hook` crate (SIGTERM and SIGHUP flags). | `cargo build` has 0 warnings |
| **M0.4** | Format | `cargo fmt --all` | `cargo fmt --check` passes |
| **M0.5** | Check that the macOS code compiles | `rustup target add aarch64-apple-darwin` then `cargo check --target aarch64-apple-darwin -p clack-input`. If the missing SDK makes cross-checking impossible, push a branch and let the `macos-latest` CI job check it. Fix all errors. | the macOS CI job is green |
| **M0.6** | CI uploads ready-made binaries | In `.github/workflows/ci.yml`, after the tests pass, build `--release` on both `ubuntu-latest` and `macos-latest` (the macOS build should be a universal binary, or build both arm64 and x86_64) and upload them with `actions/upload-artifact`. This lets the Mac tester (a friend of Darryl's, at M2) download a binary without installing Rust. The proper installer comes in M3.11. | Each CI run shows downloadable `clack-linux-x86_64` and `clack-macos` artifacts |

---

## Milestone M1: First sound on the dev machine (Arch + Hyprland)

Goal: Darryl can type in any app and hear Cherry MX Blue. Manual steps (a hand-installed udev rule, running the daemon in a terminal) are fine at this stage.

| ID | Task | Details | Done when |
|---|---|---|---|
| **M1.1** ∥ | **Input debug example** | `crates/clack-input/examples/keys.rs`: calls `probe()`, lists the keyboards found, then prints `Down 30` / `Up 30` lines from `run()`. This is the only place key codes may be printed. | Running it prints events from the laptop keyboard. Without access, it prints a clear permission error. |
| **M1.2** ∥ | **Mechvibes importer** (the P1.4 gap) | `clack-core/src/mechvibes.rs`: `fn import(src: &Path, dest: &Path) -> Result<Manifest>`. Handle `key_define_type: "single"` (sprite: `defines` maps a code to `[start_ms, dur_ms]`) and `"multi"` (a code maps to a filename). Translate codes with `keymap::mechvibes_to_evdev`, **skipping unknown codes with a warning count, not by failing**. Fill in `id` (slug of the folder name), `name`, `family = other`, `license` (a CLI argument, default `"unknown"`), and `source`. Copy the audio and write `manifest.toml`. Keys missing from the Mechvibes config fall back to `press.default`: take up to 5 clips from the alphanumeric keys. | Unit tests pass using real fixtures (see T-list). All 18 upstream packs in `hainguyents13/mechvibes/src/audio/` import and load via `decoder::load_pack` (a scripted check under `#[ignore]`, run manually, output pasted). |
| **M1.3** | **Minimal `clack pack add <local path>`** | Wire `cli.rs:207`. It detects either a native pack (has `manifest.toml`) or a Mechvibes pack (has `config.json`), then imports or copies it into `packs_dir()/<id>/` atomically (temp dir + rename). Catalog and URL support come in M2. | `clack pack add ~/Soundpacks/cherrymx-blue-pbt` creates a valid pack |
| **M1.4** | **Hand-install the udev rule on the dev machine** | `sudo install -m644 assets/70-clack.rules /etc/udev/rules.d/`, then reload and trigger (§5.8). Record the exact commands in `progress.md`. These are the same steps setup will automate later. | M1.1's example works **without** the `input` group or sudo |
| **M1.5** | **First end-to-end run** | Run `clack daemon` in a terminal, then `clack use cherrymx-blue-pbt`. Type in another app. Test `clack volume 30`, `clack toggle`, and double-tap Ctrl (with its tones). | Darryl confirms he hears it. The latency feels instant (a `--measure` number is nice to have). Record it in `progress.md`. |
| **M1.6** | **Listening session with Darryl** | Clone `hainguyents13/mechvibes` and `pack add` the six Mechvibes-format starter picks (see "Starter set" below): `cherrymx-blue-pbt`, `cherrymx-blue-abs`, `cherrymx-brown-pbt`, `cherrymx-brown-abs`, `cherrymx-black-abs`, `cream-travel`. Darryl cycles through them with `clack use`. | **Not blocking:** prepare the six packs so Darryl can listen whenever he likes, and record his feedback in `progress.md` when it comes. M2 starts right away. |

**M1 exit:** typing in a browser plays sounds (confirmed by Darryl when he's available; an agent-side check that the daemon receives events and the null sink gets audio is enough to move on).

**Testing with Darryl:** M1 is tested live with Darryl on his machine (Arch + Hyprland). He runs the one `sudo` command for the udev rule himself. The daemon runs in the foreground in a spare terminal during M1; start-at-login comes in M2.8.

---

## Milestone M2: A reliable local daily-driver

Goal: it survives a whole day of real use on the dev machine, starts at login, and has no known audio bugs.

### Audio and engine fixes

| ID | Task | Details | Done when |
|---|---|---|---|
| **M2.1** ∥ | **Stop freeing memory in the audio callback** | `mixer.rs:244` (`v.clip = clip`) drops the old `Arc` inside the callback. After a pack switch, that drop can be the **last** reference, so the memory gets freed in the audio thread. Fix: add a return `rtrb` ring (callback → engine thread) and push the replaced/finished `Arc`s into it instead of dropping them. The engine thread drains and drops them. If the ring is full, leak it (`std::mem::forget`) instead of dropping; with a 256-slot ring this won't happen in practice. Also `Voice::default()` calls `Arc::new(Vec::new())`: use an `Option<Arc<…>>` instead. | Test T-M3 (a counting allocator) shows 0 allocations and 0 frees in `render()` across a pack swap |
| **M2.2** ∥ | **Proper randomness** | `mixer.rs:66` picks the clip index from `SystemTime` nanoseconds. Replace it with a small PRNG (`fastrand` or xorshift) that lives on the engine thread. Use the same PRNG for pitch/gain variation. Move `pick_clip` so it takes `&mut Rng`. | Unit test: 10k picks over 5 clips are roughly uniform (χ²-ish bound) |
| **M2.3** ∥ | **Better resampling and pitch** | `decoder.rs` resamples nearest-neighbour (audible aliasing on 44.1 → 48 kHz), and `render()` reads `voice.pos as usize` (also nearest). Switch both to linear interpolation. Keep the callback allocation-free. | Unit test: resampling a 1 kHz sine from 44.1 → 48 kHz keeps the frequency within 1 %. Clicks sound clean by ear. |
| **M2.4** ∥ | **Reload the pack on a sample-rate change** | After `Engine::restart()`, if `sample_rate` changed, reload the bank at the new rate (engine thread, off the audio path). Update the device-rate variable. | Manual test: switching between two outputs with different rates → no pitch shift |
| **M2.5** | **Mixing headroom** | Voices are summed and then hard-clamped. Add a soft limiter (e.g. `tanh`-style or a simple peak compressor) so fast typing at high volume doesn't crackle. | Manual test: a mash of 10 keys at volume 100 has no crackle |

### Input

| ID | Task | Details | Done when |
|---|---|---|---|
| **M2.6** ∥ | **Dedupe multi-node keyboards** | Some keyboards (gaming boards, QMK/NKRO) expose several `event*` nodes, so one keypress triggers several sounds. Group devices by `phys()`, stripping the `/inputN` suffix, or by the sysfs parent. Per group, **ignore a Down for the same code within 5 ms** of one already emitted. | Unit test on the dedupe filter. Manual test with an external keyboard if available. |
| **M2.7** | **Check key-code accuracy** | Using M1.1's example and a real Mechvibes pack, confirm that Space, Enter, Backspace, Shift, arrows and the numpad get their dedicated sounds (the extended-key table in `keymap.rs`). | Manual checklist recorded in `progress.md`. Add unit tests for any table fixes. |

### Service and permissions (pulled forward from Phase 3 for Linux)

| ID | Task | Details | Done when |
|---|---|---|---|
| **M2.8** | **Service management, Linux** (§5.7) | Implement it in `clack/src/service.rs`. With systemd: write `~/.config/systemd/user/clack.service` and `systemctl --user enable --now clack`. Without systemd: an XDG autostart `.desktop` file plus a detached spawn (`setsid`, stdio → log file). `start`/`stop`/`restart` must use whichever one is installed and **never block the terminal**. Detect WMs that ignore XDG autostart and print the `exec-once` hint. | On the dev machine, after a reboot and login, clack plays sounds without any command. `clack stop` / `start` work. |
| **M2.9** | **Linux permission setup, automated** (§5.8) | `clack/src/perms.rs`: install the udev rule via `sudo`, reload and trigger, then verify with `probe()`. Handle the fallbacks: no logind → `input` group plus a re-login notice; NixOS → print the `services.udev.extraRules` snippet and write nothing. Expose it as an internal step for `setup` (M3) and as `clack setup --permissions-only` for now. | Remove the hand-installed rule, run the command, and access works immediately |
| **M2.10** | **Single instance and clean shutdown** | A second `clack daemon` exits with the message "already running". A stale socket (left by a crashed daemon) is detected and removed. SIGTERM exits in under 1 s. | Integration test T-I3 |

### Pack manager (P2.3)

| ID | Task | Details | Done when |
|---|---|---|---|
| **M2.11** | **Catalog client** (§5.9) | Fetch `index.json` from `https://github.com/DarrylMathias/clack-packs/releases/latest/download/index.json` with `ureq` (rustls). Cache it in `data_dir()/index.json`, refresh it after 24 h, and silently fall back to the cache when offline. **The packs repo doesn't exist until M3**: make the URL overridable with `CLACK_INDEX_URL` and develop against a fixture. A missing or unreachable catalog must never break `list`/`use` for local packs. | Unit tests with an `index.json` fixture, including offline and no-catalog behaviour |
| **M2.12** | **`pack add <id \| url>`, `pack remove`, `use` auto-install** | Download → verify sha256 → extract to a temp dir → validate the manifest → atomic rename. `use` installs the pack if it's missing (D9). `pack remove` refuses to remove the active pack. | Integration test T-I4 against a local HTTP server (`tiny_http`) serving a fixture tarball |
| **M2.13** | **`clack list`** (§3.3) | The whole catalog plus local packs. `●` active / `○` installed / blank = available, the family column, and the license shown for anything non-MIT, plus `local` for user-added packs. No filters (per Darryl). | Snapshot test of the rendered table from fixtures |

**Hardware limits on the dev machine:** there's no keyboard that shows up as several input devices, and every audio output runs at 48 kHz. Verify M2.6 and M2.4 with unit and fake tests, and mark the real-hardware check as "pending hardware" in `progress.md`.

**M2 exit:** Darryl uses it for a full day. It autostarts, and he finds no crackle, double sounds, or crashes. All M2 tests pass in CI.

---

## Milestone M3: v1 features

### Sound packs (`DarrylMathias/clack-packs`, a separate repo)

| ID | Task | Details | Done when |
|---|---|---|---|
| **M3.1** ∥ | **Scaffold the packs repo** (§6.3) | `packs/<id>/source.toml` (upstream URL, **pinned commit**, license, notes). A build tool (reuse the M1.2 importer as a small Rust binary; this avoids a second implementation) that: fetches upstream → converts → trims silence → **re-encodes to OGG Vorbis ~q3, mono, 48 kHz** → writes `LICENSE` → packs `<id>.tar.gz` → writes `index.json` (id, name, family, license, starter, size, sha256, url, blurb). A GitHub Action publishes `packs-vN` on tag. | `index.json` plus the tarballs are published at a real release URL |
| **M3.2** ∥ | **License verification** | Confirm each source in §6.1. **daktilo:** find the origin of its recordings (check its README / commit history / issues). If it's unclear, replace the typewriter starter with another MIT/CC0 pack. Write `CREDITS.md` (both repos): each pack → upstream, author, license, link. Add a takedown contact. | Every published pack has LICENSE plus a CREDITS entry. The daktilo decision is recorded. |
| **M3.3** | **Build all packs** | The **starter set** (see "Starter set" below), plus the remaining Mechvibes, kbsim, keebsound, Monkeytype and keyboardsounds packs (GPL packs each carry their GPL LICENSE). Needs importers for two formats beyond Mechvibes: **Bucklespring** (209 per-key WAVs → a multi-file pack; filenames are hex evdev codes: `1e-0.wav` = KEY_A down, `-1` = up) and **keyboardsounds** (`profile.yaml` plus WAVs such as `key1..6.wav`, `space.wav`, `ent.wav`, `back.wav`). kbsim has its own layout too. | **≤ 1 MB per pack, starter set ≤ ~11 MB total.** Each pack loads through clack's decoder in a CI job in the packs repo. Darryl gives the final listening sign-off on the starter packs that couldn't be tested in M1.6. |

### Onboarding and setup

| ID | Task | Details | Done when |
|---|---|---|---|
| **M3.4** | **`clack setup` TUI** (§3.2, §6.4) | Built with crossterm (+ ratatui if it helps). Steps: permissions (M2.9 / macOS flow), then the picker, then autostart (M2.8 / launchd), then done. The picker **reads keys from the terminal itself**, not evdev. The 10 starter packs download in parallel in the background with a progress bar per row. A highlighted row auto-plays a short demo, and a text box plays live typing. The chosen pack must be fully downloaded before finishing. Idempotent; `--reset` redoes everything. Plain `clack` with no config → runs setup. | Manual run on the dev machine after deleting `~/.config/clack` and `~/.local/share/clack`, recorded in `progress.md`. Unit tests for the step state machine. |
| **M3.5** | **`--help` polish** (§3.4) | Match the target help text: grouped headings, EXAMPLES, FILES, PRIVACY. Every subcommand has its own `--help` with at least one example. | Snapshot test of `clack --help` and `clack use --help` |

### Maintenance commands

| ID | Task | Details | Done when |
|---|---|---|---|
| **M3.6** ∥ | **`clack doctor`** (§5.11) | ✓ / ✗ / ! lines, each with the exact command to fix it. Checks: binary on PATH; version vs latest; config parses; active pack valid; service installed and running; IPC responds; input access (Linux: readable keyboards, rule present, `getfacl` shows the ACL, logind present, NixOS detection; macOS: `CGPreflightListenEventAccess` plus an offer to open Settings); audio device opens; WM autostart hint. | Unit tests for each check against fakes. Manual run showing both healthy output and output with the udev rule removed. |
| **M3.7** ∥ | **`clack update`** (§5.12) | Via `axoupdater`: check → download → verify → replace the binary atomically → restart the service → refresh the catalog. macOS: warn before, then walk through re-granting Input Monitoring after. | Manual test updating from a `v0.0.1-test` pre-release to a newer one |
| **M3.8** ∥ | **`clack uninstall`** (§5.13) | Stop and remove the service, remove the udev rule (sudo) and reload, remove the binary, prompt `Delete packs and config too? [y/N]`. | Manual test on a VM: no leftover files except (optionally) the data directory |

### macOS

| ID | Task | Details | Done when |
|---|---|---|---|
| **M3.9** | **Run on a real Mac** | Done by **a friend of Darryl's, after M2 is complete** (decided). They download the CI binary from M0.6 and clear the quarantine flag once with `xattr -d com.apple.quarantine ./clack`. Nothing macOS-specific is verified before then, so treat macOS bugs found here as expected. Check: the Input Monitoring prompt and the setup wait loop, modifiers (including double-tap Ctrl), no repeat sounds, launchd autostart, the `kVK` → evdev table (dedicated Space/Enter sounds), silence in secure password fields (expected). | Checklist recorded in `progress.md` with the macOS version and Mac model |
| **M3.10** | **launchd service** (§5.7) | `~/Library/LaunchAgents/dev.clack.plist`, `launchctl bootstrap gui/$UID`, `KeepAlive`. | Survives a logout and login on a real Mac |

### Release pipeline

| ID | Task | Details | Done when |
|---|---|---|---|
| **M3.11** | **cargo-dist** (§3.1, §7) | Targets: `x86_64`/`aarch64-unknown-linux-gnu` built against an **old glibc** (`cargo-zigbuild`, glibc 2.28 or older), plus `aarch64`/`x86_64-apple-darwin`. The installer puts the binary in `~/.local/bin`, makes sure it's on `PATH`, then runs `clack setup`. | A `v0.0.1-rc` pre-release installs with the single curl command on a fresh Ubuntu VM |

**M3 exit:** on a fresh VM, `curl … | sh` → the setup picker → sounds play → reboot → sounds still play, with zero manual steps.

---

## Milestone M4: Verification and v1 release

| ID | Task | Done when |
|---|---|---|
| **M4.1** | **Test matrix** (§8): Arch + Hyprland · Ubuntu 24.04 GNOME Wayland · Fedora KDE · Debian 12 Xfce (X11) · Void or Alpine (non-systemd) · NixOS · macOS (Apple Silicon, plus Intel if possible). Per system: install → setup → sound → toggle → `use` another pack → reboot → `doctor` → `update` → `uninstall`. | A results table in `progress.md`. Every failure has a GitHub issue, and every blocker is fixed. |
| **M4.2** | **Latency check.** Run the daemon with `--measure` (logs aggregate p50/p95 from input timestamp to buffer write, **no codes**). | p95 < 20 ms on the dev machine; numbers recorded |
| **M4.3** | **Docs.** README: one-line install, privacy statement at the top, a GIF, command table, WM autostart snippets, how to add Mechvibes packs, troubleshooting → `clack doctor`. PRIVACY.md, CREDITS.md. | Reviewed by Darryl |
| **M4.4** | **Release `v0.1.0`** (the first public release; "v1" in this doc means this release). Tag, run cargo-dist, smoke-test the installer on 2+ systems from the matrix right after publishing. | Release is public, and the installer URL in the README works |

---

## Test inventory (every one of these must exist and pass by v1)

**Unit tests** (in `cargo test`, CI on Linux and macOS):

| ID | Covers |
|---|---|
| T-K1 | `keymap`: every ANSI/ISO key maps both ways for macOS `kVK` and Mechvibes codes; extended keys (arrows, numpad, right-side modifiers) are correct |
| T-P1 | `Manifest` parsing: sprite mode, multi-file mode, arrays (random), missing `release`, invalid ranges rejected |
| T-P2 | Mechvibes importer: `single` fixture, `multi` fixture, unknown codes skipped and counted, default clips derived |
| T-P3 | Decoder: loads OGG, WAV and MP3 fixtures; sprite slicing boundaries; linear resampling accuracy (M2.3) |
| T-M1 | Mixer: voice stealing when all 32 are busy; gain curve; mono → stereo; soft limiter bounds |
| T-M2 | `pick_clip` distribution with a seeded PRNG (M2.2); falls back to `default` |
| T-M3 | **No allocations or frees in `render()`**, including across a bank swap (counting global allocator, test calls the render function directly with a fake buffer; refactor the mixing into a pure `fn render(&mut MixerState, &mut [f32])` to make this testable) |
| T-T1 | Toggle detector: valid double tap; too slow; a key in between; auto-repeat; left then right Ctrl; `toggle = none` |
| T-D1 | Input dedupe filter (M2.6) |
| T-C1 | Config / state load/save roundtrip; missing file → defaults; bad TOML → clear error |
| T-L1 | Catalog: parse, cache freshness, offline fallback |
| T-S1 | `clack list` table snapshot; `--help` snapshots (use `insta`) |

**Integration tests** (`crates/clack/tests/`): the daemon runs with a **fake `InputBackend`** (feeds scripted events) and a **null audio sink** (an `Engine` variant writing to a buffer), so no hardware is needed and CI can run them. This requires making `daemon::run` accept injected backends.

| ID | Covers |
|---|---|
| T-I1 | IPC round-trip: `status`, `toggle`, `set_muted`, `volume`, `use`, `reload`, `shutdown` all return the expected responses |
| T-I2 | Scripted key events → the null sink receives audio; muted → silence; toggle tones play while muted |
| T-I3 | Single instance; stale-socket cleanup; SIGTERM exits quickly |
| T-I4 | `pack add` from a local HTTP server: correct sha256 installs; wrong sha256 is rejected and nothing is left behind; `use` auto-installs |

**Privacy guard (CI):**

| ID | Covers |
|---|---|
| T-X1 | A script or test greps `crates/*/src` for logging/printing macros whose arguments include `code`, `KeyEvent`, or `event`. It fails the build unless the line is annotated `// privacy-ok: <reason>`. `examples/` is excluded. |

**Manual checklists** (recorded in `progress.md`): M1.5 first sound, M2.7 key accuracy, M2.8 autostart, M3.4 onboarding, M3.9 macOS, M4.1 matrix, M4.2 latency.

---

## Starter set (Darryl's picks, 2026-09-25)

Shortlisted by listening on The Listening Museum. This **replaces the proposed list in `implementation.md` §6.2**. Final sign-off happens by listening in clack itself: in M1.6 for the Mechvibes packs, and in M3.3 for the rest.

| # | Pack | Source | License | In M1? | Status |
|---|---|---|---|---|---|
| 1 | IBM Model M | zevv/bucklespring `wav/` | GPL-2.0 | ❌ needs Bucklespring importer (M3.3) | ✅ OK |
| 2 | Alps SKCM Blue | tplai/kbsim `bluealps` | MIT | ❌ kbsim format (M3.3) | ✅ OK |
| 3 | Electric Typewriter | nathan-fiscaletti/keyboardsounds `profiles/typewriter` | GPL-3.0 | ❌ keyboardsounds format (M3.3) | ✅ OK. Separate from daktilo's "Vintage Typewriter", so the daktilo provenance check is **no longer blocking** |
| 4 | Cherry MX Blue (PBT) | hainguyents13/mechvibes `cherrymx-blue-pbt` | MIT | ✅ | ✅ OK |
| 5 | Cherry MX Blue / ABS | mechvibes `cherrymx-blue-abs` | MIT | ✅ | ✅ OK |
| 6 | Cherry MX Brown (PBT) | mechvibes `cherrymx-brown-pbt` | MIT | ✅ | ✅ OK |
| 7 | Cherry MX Brown / ABS | mechvibes `cherrymx-brown-abs` | MIT | ✅ | ✅ OK |
| 8 | Cherry MX Black / ABS | mechvibes `cherrymx-black-abs` | MIT | ✅ | ✅ OK |
| 9 | Kailh Box White | **only** found in `dusklinux/wayclick_soundpacks` (no license; originally a Mechvibes *community* pack, files `kboxwh - 01..10.mp3`) | **unknown** | — | ⛔ **Blocked**, see below |
| 10 | NK Cream (full travel) | mechvibes `cream-travel` | MIT | ✅ | ✅ OK |
| 11 | Zeal Tealios V2 | keyboardsounds `profiles/telios-v2` | GPL-3.0 | ❌ keyboardsounds format (M3.3) | ✅ OK |

**Kailh Box White (M3.2 agent):** we can't redistribute it without a license. Options, in order:
1. Find the original Mechvibes community author and ask for permission, or find an MIT/CC0 upload of the same recording.
2. Find another permissively licensed Box White recording (e.g. Freesound CC0).
3. Otherwise drop it. The starter set becomes 10, which matches the original plan, and users can still `clack pack add` their own copy.

## Decisions log (2026-09-25)

| Decision | Detail |
|---|---|
| Current scope | **M0 → M1 → M2, back to back** (updated). Goal: an end-to-end working prototype on Darryl's machine. **Don't wait** for M1 listening feedback before starting M2. M3 and M4 are paused. |
| CI | The branch gets renamed `master` → `main` so the existing workflow runs (M0.0) |
| sudo | Darryl types it himself when asked. Agents stop and hand him the exact command. |
| Day of use | Darryl agreed to the full-day test as the M2 exit check |
| Listening plan | Two rounds. Round 1 is done (shortlist from The Listening Museum, above). Round 2 is in clack: M1.6 for the Mechvibes picks, M3.3 for the rest. |
| First public release | **`v0.1.0`** |
| macOS testing | A friend of Darryl's runs M3.9 **after M2 is complete**, using the CI binary from M0.6 |
| CI binaries | Added **M0.6** so testers never need a Rust toolchain |

## Handover: next agent

**Scope: M0 → M1 → M2**, ending with a working prototype on Darryl's machine: type anywhere, hear sounds, clack starts at login. Start with the "NEXT AGENT" note at the top of `progress.md`, then this file (the tasks), with `implementation.md` as the spec. Follow the rules in §1: nothing is "done" until it has been run and the output is recorded in `progress.md`. For steps needing sudo or a reboot, stop and give Darryl the exact commands. Stop after M2.
