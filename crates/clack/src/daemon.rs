//! Daemon — the background service process (§5.6).
//!
//! Architecture (§4):
//!   input thread → crossbeam channel → engine thread → rtrb ring → audio callback
//!
//! The Engine (cpal stream) lives on the engine thread — cpal streams are not Send.
//! IPC commands are passed to the engine thread via a secondary crossbeam channel.
//!
//! Idle stream (§change-3):
//!   The cpal stream is opened on the first keypress after silence and closed
//!   `idle_timeout_secs` after the last keypress.  While idle, clack holds no
//!   PipeWire node, so the graph quantum reverts to the system default and
//!   Bluetooth A2DP sinks are unaffected.

use std::{
    os::unix::net::UnixListener,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use tracing::{error, info, warn};

use clack_core::{
    config::{log_path, socket_path, Config, State},
    decoder::load_pack,
    ipc::{Command, DaemonStatus, Response},
    mixer::{perceptual_gain, Engine, SampleBank, TARGET_RATE},
    pack::pack_dir,
    toggle::ToggleDetector,
    types::{KeyEvent, KeyState},
};
use clack_input::{DefaultBackend, InputBackend};
use crossbeam_channel::{unbounded, Sender};

// ── Internal engine-thread control messages ───────────────────────────────────

enum EngineMsg {
    SetBank(SampleBank),
    SetGain(f32),
    Reload,
    Shutdown,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run(cli_sink: Option<String>) -> Result<()> {
    // Ensure log dir exists.
    let log_dir = log_path();
    if let Some(dir) = log_dir.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let file_appender = tracing_appender::rolling::daily(log_dir.parent().unwrap(), "clack.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .with_writer(non_blocking)
        .init();

    let config = Config::load().context("loading config")?;
    let state = State::load().unwrap_or_default();

    // Priority: CLI override -> PIPEWIRE_NODE env -> config.output_sink -> default
    let output_sink = cli_sink
        .or_else(|| {
            std::env::var("PIPEWIRE_NODE")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or(config.output_sink.clone())
        .unwrap_or_else(|| clack_core::config::DEFAULT_SPEAKER_SINK.to_string());

    // Before opening the stream, set env PIPEWIRE_NODE=<that node name>
    // (do it early in main, before threads spawn; std::env::set_var is fine in edition 2021).
    unsafe {
        std::env::set_var("PIPEWIRE_NODE", &output_sink);
        if std::env::var("PIPEWIRE_LATENCY").is_err() {
            std::env::set_var("PIPEWIRE_LATENCY", "256/48000");
        }
    }

    let idle_timeout = if config.idle_timeout_secs == 0 {
        None // keep stream permanently open
    } else {
        Some(Duration::from_secs(config.idle_timeout_secs))
    };

    info!(
        "clack daemon starting — pack: {}, sink: {}",
        config.default_pack, output_sink
    );

    install_signal_handler();

    // Single-instance guard via socket bind.
    let sock_path = socket_path();
    if let Some(dir) = sock_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if sock_path.exists() {
        if std::os::unix::net::UnixStream::connect(&sock_path).is_ok() {
            eprintln!("clack is already running.");
            std::process::exit(1);
        } else {
            // Stale socket, remove it.
            let _ = std::fs::remove_file(&sock_path);
        }
    }
    let listener = UnixListener::bind(&sock_path)
        .with_context(|| format!("binding IPC socket at {}", sock_path.display()))?;
    listener.set_nonblocking(true)?;

    // Shared state (IPC thread ↔ engine thread and IPC thread ↔ status endpoint).
    let muted = Arc::new(AtomicBool::new(state.muted));
    let master_gain_bits = Arc::new(AtomicU32::new(perceptual_gain(config.volume).to_bits()));
    let active_pack_name = Arc::new(Mutex::new(config.default_pack.clone()));

    // Channel: main/IPC → engine thread.
    let (eng_tx, eng_rx) = unbounded::<EngineMsg>();
    // Channel: input backend → engine thread.
    let (key_tx, key_rx) = unbounded::<KeyEvent>();

    // ── Input thread ──────────────────────────────────────────────────────────
    let stop_input = Arc::new(AtomicBool::new(false));
    {
        let stop = stop_input.clone();
        let tx = key_tx;
        thread::Builder::new()
            .name("clack-input".into())
            .spawn(move || {
                let mut backend = DefaultBackend;
                if let Err(e) = backend.run(tx, stop) {
                    error!("input backend error: {e:#}");
                }
            })?;
    }

    // ── Engine thread ─────────────────────────────────────────────────────────
    // Owns the cpal Engine (not Send); runs the toggle detector; dispatches PlayCmd.
    let device_rate_shared = Arc::new(AtomicU32::new(TARGET_RATE));
    let device_rate_for_eng = device_rate_shared.clone();

    let toggle_binding = config.toggle.clone();
    let release_sounds = config.release_sounds;
    let humanize = config.humanize;
    let muted_eng = muted.clone();

    let default_pack_name = config.default_pack.clone();
    let active_pack_name_eng = active_pack_name.clone();

    thread::Builder::new()
        .name("clack-engine".into())
        .spawn(move || {
            let mut engine: Option<Engine> = None;

            let device_rate = TARGET_RATE;
            device_rate_for_eng.store(device_rate, Ordering::Relaxed);

            // Set initial gain.
            let init_gain = perceptual_gain(config.volume);
            let mut current_gain = init_gain;

            info!("audio engine ready (stream will open on first keypress)");

            // Load initial pack at TARGET_RATE so no resampling happens at play time.
            let pdir = pack_dir(&default_pack_name);
            let mut bank = if pdir.exists() {
                match load_pack(&pdir, device_rate) {
                    Ok(b) => {
                        info!("pack '{default_pack_name}' loaded");
                        b
                    }
                    Err(e) => {
                        warn!("pack load failed: {e:#}");
                        SampleBank::empty()
                    }
                }
            } else {
                warn!("pack '{default_pack_name}' not found");
                SampleBank::empty()
            };

            let mut rng = fastrand::Rng::new();
            let mut detector = ToggleDetector::default();

            // Idle timeout tracking.
            let mut last_key_time: Option<Instant> = None;

            loop {
                if let Some(ref mut eng) = engine {
                    eng.gc();

                    // Stream error recovery (§M2.4).
                    if eng.stream_error.load(Ordering::Relaxed) {
                        let old_rate = eng.sample_rate;
                        if let Err(e) = eng.restart(current_gain) {
                            error!("stream restart failed: {e:#}");
                            engine = None;
                        } else if eng.sample_rate != old_rate {
                            info!(
                                "sample rate changed from {} to {} on restart — reloading pack",
                                old_rate, eng.sample_rate
                            );
                            device_rate_for_eng.store(eng.sample_rate, Ordering::Relaxed);
                            let active = active_pack_name_eng.lock().unwrap().clone();
                            let pdir = pack_dir(&active);
                            if let Ok(new_bank) = load_pack(&pdir, eng.sample_rate) {
                                bank = new_bank;
                            }
                        }
                    }
                }

                // ── Idle timeout: close stream if inactive long enough ─────
                if let (Some(timeout), Some(last)) = (idle_timeout, last_key_time) {
                    if engine.is_some() && last.elapsed() >= timeout {
                        engine = None; // drops the Engine (closes the stream)
                        last_key_time = None;
                        info!("stream closed after idle timeout");
                    }
                }

                // Check for engine-thread messages (non-blocking).
                while let Ok(msg) = eng_rx.try_recv() {
                    match msg {
                        EngineMsg::SetBank(b) => bank = b,
                        EngineMsg::SetGain(g) => {
                            current_gain = g;
                            if let Some(ref mut eng) = engine {
                                eng.set_master_gain(g);
                            }
                        }
                        EngineMsg::Reload => {
                            // Reload config and pack inside the engine thread.
                            if let Ok(cfg) = Config::load() {
                                let pdir = pack_dir(&cfg.default_pack);
                                let rate = device_rate_for_eng.load(Ordering::Relaxed);
                                if let Ok(new_bank) = load_pack(&pdir, rate) {
                                    bank = new_bank;
                                    info!("reloaded pack '{}'", cfg.default_pack);
                                }
                                let g = perceptual_gain(cfg.volume);
                                current_gain = g;
                                if let Some(ref mut eng) = engine {
                                    eng.set_master_gain(g);
                                }
                            }
                        }
                        EngineMsg::Shutdown => {
                            return;
                        }
                    }
                }

                let event = match key_rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(ev) => ev,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                };

                // Toggle gesture.
                if detector.feed(&event, &toggle_binding) {
                    let was = muted_eng.fetch_xor(true, Ordering::SeqCst);
                    let now_muted = !was;
                    info!("toggle: {}", if now_muted { "muted" } else { "unmuted" });
                    if !now_muted {
                        if engine.is_none() {
                            match Engine::start() {
                                Ok(mut eng) => {
                                    let old_rate = device_rate_for_eng.load(Ordering::Relaxed);
                                    if eng.sample_rate != old_rate {
                                        device_rate_for_eng
                                            .store(eng.sample_rate, Ordering::Relaxed);
                                        let active = active_pack_name_eng.lock().unwrap().clone();
                                        let pdir = pack_dir(&active);
                                        if let Ok(new_bank) = load_pack(&pdir, eng.sample_rate) {
                                            bank = new_bank;
                                        }
                                    }
                                    eng.set_master_gain(current_gain);
                                    engine = Some(eng);
                                }
                                Err(e) => {
                                    warn!("failed to start audio engine: {e:#}");
                                }
                            }
                        }
                        if let Some(ref mut eng) = engine {
                            play_tone(eng, true);
                            last_key_time = Some(Instant::now());
                        }
                    }
                    let _ = State { muted: now_muted }.save();
                    continue;
                }

                if muted_eng.load(Ordering::Relaxed) {
                    continue;
                }

                let is_release = event.state == KeyState::Up;
                if is_release && !release_sounds {
                    continue;
                }

                if let Some(clip) = bank.pick_clip(event.code, is_release, &mut rng) {
                    let samples = clip.samples.clone();
                    let channels = clip.channels;
                    let (rate, gain) = if humanize {
                        let pitch = 0.97 + rng.f32() * 0.06; // 0.97 to 1.03
                        let g = 0.90 + rng.f32() * 0.20; // 0.90 to 1.10
                        (pitch, g)
                    } else {
                        (1.0f32, 1.0f32)
                    };

                    if engine.is_none() {
                        match Engine::start() {
                            Ok(mut eng) => {
                                let old_rate = device_rate_for_eng.load(Ordering::Relaxed);
                                if eng.sample_rate != old_rate {
                                    device_rate_for_eng.store(eng.sample_rate, Ordering::Relaxed);
                                    let active = active_pack_name_eng.lock().unwrap().clone();
                                    let pdir = pack_dir(&active);
                                    if let Ok(new_bank) = load_pack(&pdir, eng.sample_rate) {
                                        bank = new_bank;
                                    }
                                }
                                eng.set_master_gain(current_gain);
                                engine = Some(eng);
                            }
                            Err(e) => {
                                warn!("failed to start audio engine: {e:#}");
                            }
                        }
                    }

                    if let Some(ref mut eng) = engine {
                        eng.play(samples, channels, rate, gain);
                        last_key_time = Some(Instant::now());
                    }
                }
            }
        })?;

    // Wait briefly for engine to report its sample rate.
    let device_rate = {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let r = device_rate_shared.load(Ordering::Relaxed);
            if r > 0 {
                break r;
            }
            if std::time::Instant::now() > deadline {
                warn!("engine rate not reported; using 48000");
                break 48_000;
            }
            thread::sleep(Duration::from_millis(10));
        }
    };

    // ── IPC loop ──────────────────────────────────────────────────────────────
    info!("IPC socket: {}", sock_path.display());

    loop {
        if get_shutdown().load(Ordering::Relaxed) {
            info!("shutdown requested");
            break;
        }
        if get_reload().load(Ordering::Relaxed) {
            get_reload().store(false, Ordering::Relaxed);
            let _ = eng_tx.send(EngineMsg::Reload);
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                use std::io::{BufRead, BufReader, Write};
                let mut reader = BufReader::new(stream.try_clone()?);
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() && !line.is_empty() {
                    let resp = match serde_json::from_str::<Command>(line.trim()) {
                        Ok(cmd) => handle_ipc(
                            cmd,
                            &muted,
                            &master_gain_bits,
                            &active_pack_name,
                            &eng_tx,
                            device_rate,
                        ),
                        Err(e) => Response::err(format!("bad command: {e}")),
                    };
                    let _ =
                        stream.write_all((serde_json::to_string(&resp).unwrap() + "\n").as_bytes());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => warn!("IPC accept error: {e}"),
        }
    }

    let _ = eng_tx.send(EngineMsg::Shutdown);
    stop_input.store(true, Ordering::SeqCst);
    let _ = std::fs::remove_file(&sock_path);
    info!("clack daemon stopped");
    Ok(())
}

// ── IPC command handler ───────────────────────────────────────────────────────

fn handle_ipc(
    cmd: Command,
    muted: &Arc<AtomicBool>,
    gain_bits: &Arc<AtomicU32>,
    active_pack: &Arc<Mutex<String>>,
    eng_tx: &Sender<EngineMsg>,
    device_rate: u32,
) -> Response {
    match cmd {
        Command::Status => {
            let g = f32::from_bits(gain_bits.load(Ordering::Relaxed));
            let vol = (g.sqrt() * 100.0).round() as u8;
            Response::ok(Some(DaemonStatus {
                running: true,
                muted: muted.load(Ordering::Relaxed),
                active_pack: Some(active_pack.lock().unwrap().clone()),
                volume: vol,
                keyboards_detected: vec![],
            }))
        }
        Command::Toggle => {
            let was = muted.fetch_xor(true, Ordering::SeqCst);
            let now_muted = !was;
            let _ = State { muted: now_muted }.save();
            Response::ok(None)
        }
        Command::SetMuted { muted: m } => {
            muted.store(m, Ordering::SeqCst);
            let _ = State { muted: m }.save();
            Response::ok(None)
        }
        Command::Volume { value } => {
            let g = perceptual_gain(value);
            gain_bits.store(g.to_bits(), Ordering::Relaxed);
            let _ = eng_tx.send(EngineMsg::SetGain(g));
            Response::ok(None)
        }
        Command::Use { pack } => {
            let pdir = pack_dir(&pack);
            if !pdir.exists() {
                return Response::err(format!(
                    "pack '{pack}' not installed — run `clack pack add {pack}` first"
                ));
            }
            match load_pack(&pdir, device_rate) {
                Ok(bank) => {
                    let _ = eng_tx.send(EngineMsg::SetBank(bank));
                    *active_pack.lock().unwrap() = pack.clone();
                    if let Ok(mut cfg) = Config::load() {
                        cfg.default_pack = pack;
                        let _ = cfg.save();
                    }
                    Response::ok(None)
                }
                Err(e) => Response::err(format!("failed to load pack: {e:#}")),
            }
        }
        Command::AddPack { path } => {
            let src = std::path::PathBuf::from(&path);
            let name = src
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "imported".to_string());

            // basic slugify for the dest folder
            let slug = name
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .replace("--", "-")
                .trim_matches('-')
                .to_string();
            let dest = pack_dir(&slug);

            match clack_core::pack::import_pack(&src, &dest) {
                Ok(manifest) => {
                    // Try to load it
                    match load_pack(&dest, device_rate) {
                        Ok(bank) => {
                            let _ = eng_tx.send(EngineMsg::SetBank(bank));
                            *active_pack.lock().unwrap() = manifest.id.clone();
                            if let Ok(mut cfg) = Config::load() {
                                cfg.default_pack = manifest.id.clone();
                                let _ = cfg.save();
                            }
                            let g = f32::from_bits(gain_bits.load(Ordering::Relaxed));
                            let vol = (g.sqrt() * 100.0).round() as u8;
                            Response::ok(Some(DaemonStatus {
                                running: true,
                                muted: muted.load(Ordering::Relaxed),
                                active_pack: Some(manifest.id),
                                volume: vol,
                                keyboards_detected: vec![],
                            }))
                        }
                        Err(e) => {
                            Response::err(format!("pack imported, but failed to load: {e:#}"))
                        }
                    }
                }
                Err(e) => Response::err(format!("failed to import pack: {e:#}")),
            }
        }
        Command::Reload => {
            let _ = eng_tx.send(EngineMsg::Reload);
            Response::ok(None)
        }
        Command::Shutdown => {
            get_shutdown().store(true, Ordering::SeqCst);
            Response::ok(None)
        }
    }
}

// ── Toggle feedback tone (§3.5) ───────────────────────────────────────────────

fn play_tone(engine: &mut Engine, unmuting: bool) {
    let rate = engine.sample_rate;
    let frames = (rate * 80 / 1000) as usize;
    let (f1, f2) = if unmuting {
        (440.0f32, 660.0f32)
    } else {
        (660.0f32, 440.0f32)
    };

    let make_beep = |freq: f32| -> std::sync::Arc<Vec<f32>> {
        std::sync::Arc::new(
            (0..frames)
                .map(|i| {
                    let t = i as f32 / rate as f32;
                    let env = fade_env(i, frames);
                    (2.0 * std::f32::consts::PI * freq * t).sin() * 0.35 * env
                })
                .collect(),
        )
    };

    engine.play(make_beep(f1), 1, 1.0, 1.0);
    // 30 ms gap between beeps (silent clip).
    let gap_frames = (rate * 30 / 1000) as usize;
    engine.play(std::sync::Arc::new(vec![0.0f32; gap_frames]), 1, 1.0, 0.0);
    engine.play(make_beep(f2), 1, 1.0, 1.0);
}

fn fade_env(i: usize, total: usize) -> f32 {
    let fade = (total / 10).max(1);
    let attack = if i < fade {
        i as f32 / fade as f32
    } else {
        1.0
    };
    let rel_start = total.saturating_sub(fade);
    let release = if i > rel_start {
        (total - i) as f32 / fade as f32
    } else {
        1.0
    };
    attack.min(release)
}

// ── Signals ───────────────────────────────────────────────────────────────────

use std::sync::OnceLock;

static SHUTDOWN: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static RELOAD: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn get_shutdown() -> Arc<AtomicBool> {
    SHUTDOWN
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .clone()
}
fn get_reload() -> Arc<AtomicBool> {
    RELOAD
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

pub fn install_signal_handler() {
    #[cfg(unix)]
    {
        let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, get_shutdown());
        let _ = signal_hook::flag::register(signal_hook::consts::SIGHUP, get_reload());
    }
}
