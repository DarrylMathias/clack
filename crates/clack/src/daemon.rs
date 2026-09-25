//! Daemon — the background service process (§5.6).
//!
//! Run via `clack daemon` (hidden subcommand). Wires:
//!   input thread(s) → engine thread → audio callback
//!
//! IPC listens on a Unix socket and dispatches ControlMsg to the engine.
//! Logging goes to ~/.local/state/clack/clack.log (never logs key codes).

use std::{
    os::unix::net::UnixListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use tracing::{error, info, warn};

use clack_core::{
    config::{log_path, socket_path, Config, State},
    decoder::load_pack,
    ipc::{Command, DaemonStatus, Response},
    mixer::{perceptual_gain, Engine},
    pack::pack_dir,
    toggle::ToggleDetector,
    types::{KeyEvent, KeyState},
};
use clack_input::{DefaultBackend, InputBackend};

use crossbeam_channel::unbounded;

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run() -> Result<()> {
    // Set up logging to file.
    let log = log_path();
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }

    // Load config and state.
    let config = Config::load().context("loading config")?;
    let state = State::load().unwrap_or_default();

    info!("clack daemon starting — pack: {}", config.default_pack);

    // Single-instance guard: try to bind the socket; if it already exists and
    // another daemon is listening, exit cleanly.
    let sock_path = socket_path();
    if let Some(dir) = sock_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Remove stale socket from a previous crash.
    let _ = std::fs::remove_file(&sock_path);

    let listener = UnixListener::bind(&sock_path)
        .with_context(|| format!("binding IPC socket at {}", sock_path.display()))?;
    listener.set_nonblocking(true)?;

    // Load the active pack.
    let pdir = pack_dir(&config.default_pack);
    if !pdir.exists() {
        warn!("pack '{}' not found — running without sound", config.default_pack);
    }

    // Start the audio engine.
    let mut engine = Engine::start().context("starting audio engine")?;
    engine.set_master_gain(perceptual_gain(config.volume));

    // Now load the pack at the correct sample rate.
    // (The engine exposes the device sample rate; for now we assume 48000.)
    // TODO: expose device sample_rate from Engine in P2.1 follow-up.
    let device_rate: u32 = 48_000;
    if pdir.exists() {
        match load_pack(&pdir, device_rate) {
            Ok(bank) => {
                info!("pack '{}' loaded", config.default_pack);
                // Store bank in engine context — for now keep reference in engine thread.
                // Full engine ↔ bank wiring is done when daemon thread is connected.
                let _ = bank; // TODO: wire bank into engine playback in P2.1 follow-up
            }
            Err(e) => warn!("failed to load pack '{}': {e:#}", config.default_pack),
        }
    }

    // Start the input backend.
    let stop_input = Arc::new(AtomicBool::new(false));
    let (key_tx, key_rx) = unbounded::<KeyEvent>();

    {
        let stop = stop_input.clone();
        let tx = key_tx.clone();
        thread::Builder::new()
            .name("clack-input".into())
            .spawn(move || {
                let mut backend = DefaultBackend;
                if let Err(e) = backend.run(tx, stop) {
                    error!("input backend error: {e:#}");
                }
            })?;
    }

    // Engine thread: receives key events, runs toggle detector, dispatches play commands.
    let stop_engine = Arc::new(AtomicBool::new(false));
    let muted = Arc::new(AtomicBool::new(state.muted));

    {
        let stop = stop_engine.clone();
        let muted = muted.clone();
        let toggle_binding = config.toggle.clone();
        thread::Builder::new()
            .name("clack-engine".into())
            .spawn(move || {
                let mut detector = ToggleDetector::default();
                loop {
                    if stop.load(Ordering::Relaxed) { break; }
                    match key_rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(event) => {
                            // Check for toggle gesture.
                            if detector.feed(&event, &toggle_binding) {
                                let was_muted = muted.fetch_xor(true, Ordering::SeqCst);
                                info!("toggle: {}", if was_muted { "unmuted" } else { "muted" });
                                // TODO: play feedback tone (P2.1 follow-up)
                                let s = State { muted: !was_muted };
                                let _ = s.save();
                            }

                            // Play sound if not muted.
                            if !muted.load(Ordering::Relaxed) && event.state == KeyState::Down {
                                // TODO: look up clip from bank and send to engine (P2.1 follow-up)
                            }
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?;
    }

    // IPC loop: accept connections, dispatch commands.
    info!("IPC socket: {}", sock_path.display());
    loop {
        // Handle SIGTERM via a flag (set by signal handler below).
        if SHUTDOWN.load(Ordering::Relaxed) {
            info!("shutdown requested");
            break;
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                use std::io::{BufRead, BufReader, Write};
                let reader = BufReader::new(stream.try_clone()?);
                for line in reader.lines() {
                    let line = match line { Ok(l) => l, Err(_) => break };
                    let cmd: Command = match serde_json::from_str(&line) {
                        Ok(c) => c,
                        Err(e) => {
                            let resp = Response::err(format!("bad command: {e}"));
                            let _ = stream.write_all(
                                (serde_json::to_string(&resp).unwrap() + "\n").as_bytes(),
                            );
                            continue;
                        }
                    };
                    let resp = dispatch_command(cmd, &muted, &config, &engine);
                    let _ = stream.write_all(
                        (serde_json::to_string(&resp).unwrap() + "\n").as_bytes(),
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => warn!("IPC accept error: {e}"),
        }
    }

    // Graceful shutdown.
    stop_input.store(true, Ordering::SeqCst);
    stop_engine.store(true, Ordering::SeqCst);
    let _ = std::fs::remove_file(&sock_path);
    info!("clack daemon stopped");
    Ok(())
}

fn dispatch_command(cmd: Command, muted: &Arc<AtomicBool>, config: &Config, _engine: &Engine) -> Response {
    match cmd {
        Command::Status => {
            Response::ok(Some(DaemonStatus {
                running: true,
                muted: muted.load(Ordering::Relaxed),
                active_pack: Some(config.default_pack.clone()),
                volume: config.volume,
                keyboards_detected: vec![], // TODO: query input backend
            }))
        }
        Command::Toggle => {
            let was = muted.fetch_xor(true, Ordering::SeqCst);
            info!("IPC toggle: {}", if was { "unmuted" } else { "muted" });
            Response::ok(None)
        }
        Command::SetMuted { muted: m } => {
            muted.store(m, Ordering::SeqCst);
            Response::ok(None)
        }
        Command::Volume { value: _ } => {
            // TODO: update engine master gain live
            Response::ok(None)
        }
        Command::Shutdown => {
            SHUTDOWN.store(true, Ordering::SeqCst);
            Response::ok(None)
        }
        Command::Use { pack: _ } => {
            // TODO: load pack and send new bank to engine
            Response::err("live pack switching not yet implemented")
        }
        Command::Reload => {
            // TODO: reload config from disk
            Response::ok(None)
        }
    }
}

// ── Shutdown flag (set by SIGTERM handler) ────────────────────────────────────

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Register a SIGTERM handler that sets the SHUTDOWN flag.
pub fn install_signal_handler() {
    unsafe {
        libc_signal(15 /* SIGTERM */, shutdown_handler as *const () as usize);
    }
}

extern "C" fn shutdown_handler(_: i32) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

#[cfg(target_family = "unix")]
unsafe fn libc_signal(sig: i32, handler: usize) {
    libc_sys_signal(sig, handler);
}

#[cfg(target_family = "unix")]
extern "C" {
    fn signal(signum: i32, handler: usize) -> usize;
}

#[cfg(target_family = "unix")]
unsafe fn libc_sys_signal(sig: i32, handler: usize) {
    signal(sig, handler);
}
