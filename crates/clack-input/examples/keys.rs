use clack_input::{DefaultBackend, InputBackend};
use clack_core::types::KeyState;
use std::sync::{atomic::AtomicBool, Arc};
use crossbeam_channel::unbounded;
use std::thread;

fn main() {
    match DefaultBackend::probe() {
        Ok(devices) => {
            println!("Found {} devices:", devices.len());
            for dev in devices {
                println!("- {} ({})", dev.name, dev.path);
            }
        }
        Err(e) => {
            eprintln!("Permission error probing devices: {:?}", e);
            std::process::exit(1);
        }
    }

    let (tx, rx) = unbounded();
    let stop = Arc::new(AtomicBool::new(false));

    let mut backend = DefaultBackend;
    let stop_for_backend = stop.clone();
    
    // Run the backend in a separate thread
    let _handle = thread::spawn(move || {
        if let Err(e) = backend.run(tx, stop_for_backend) {
            eprintln!("Backend error: {}", e);
        }
    });

    println!("Listening for key events... Press Ctrl-C to stop.");
    
    // Print events as they arrive
    while let Ok(event) = rx.recv() {
        let state_str = match event.state {
            KeyState::Down => "Down",
            KeyState::Up => "Up",
        };
        println!("{} {}", state_str, event.code);
    }
}
