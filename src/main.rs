// src/main.rs

mod config;
mod serial_input;
mod sync_logic;
mod sync_manager;
mod ui;

use crate::config::watch_config;
use crate::serial_input::start_serial_thread;
use crate::sync_logic::LtcState;
use crate::sync_manager::start_sync_manager;
use crate::ui::start_ui;

use std::{
    env,
    fs,
    path::Path,
    sync::{mpsc, Arc, Mutex},
    thread,
};

/// Embed the default config.json at compile time.
const DEFAULT_CONFIG: &str = include_str!("../config.json");

/// If no `config.json` exists alongside the binary, write out the default.
fn ensure_config() {
    let p = Path::new("config.json");
    if !p.exists() {
        fs::write(p, DEFAULT_CONFIG)
            .expect("Failed to write default config.json");
        eprintln!("⚙️  Emitted default config.json");
    }
}

fn main() {
    // Check for --daemon flag
    let is_daemon = env::args().any(|a| a == "--daemon");

    // 🔄 Ensure there's always a config.json present
    ensure_config();

    // 1️⃣ Start watching config.json for changes
    let hw_offset = watch_config("config.json");
    if !is_daemon {
        println!("🔧 Watching config.json (hardware_offset_ms)...");
    }

    // 2️⃣ Channel for raw LTC frames
    let (tx, rx) = mpsc::channel();
    if !is_daemon {
        println!("✅ Channel created");
    }

    // 3️⃣ Shared state for UI and serial reader
    let ltc_state = Arc::new(Mutex::new(LtcState::new()));
    if !is_daemon {
        println!("✅ State initialised");
    }

    // 4️⃣ Spawn the serial reader thread (no offset here)
    {
        let tx_clone = tx.clone();
        let state_clone = ltc_state.clone();
        thread::spawn(move || {
            if !is_daemon {
                println!("🚀 Serial thread launched");
            }
            start_serial_thread(
                "/dev/ttyACM0",
                115200,
                tx_clone,
                state_clone,
                0, // ignored in serial path
            );
        });
    }

    // 5️⃣ Spawn the sync manager thread
    {
        let state_clone = ltc_state.clone();
        let offset_clone = hw_offset.clone();
        thread::spawn(move || {
            if !is_daemon {
                println!("⚙️ Sync manager thread launched");
            }
            start_sync_manager(state_clone, offset_clone);
        });
    }

    // 6️⃣ Spawn UI thread if not in daemon mode, otherwise loop forever
    if !is_daemon {
        let ui_state = ltc_state.clone();
        let port = "/dev/ttyACM0".to_string();
        let ui_handle = thread::spawn(move || {
            println!("🖥️ UI thread launched");
            start_ui(ui_state, port);
        });

        // Wait for UI to exit
        ui_handle.join().unwrap();
    } else {
        println!("🚀 Timeturner running in daemon mode.");
        // In daemon mode, just keep the main thread alive by consuming from the channel.
        for _frame in rx {
            // no-op
        }
    }
}
