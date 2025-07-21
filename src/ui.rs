// src/ui.rs

use std::{
    io::{stdout, Write},
    process::{self, Command},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use chrono::{Local, Timelike, Utc, NaiveTime, Duration as ChronoDuration, TimeZone};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{poll, read, Event, KeyCode},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use get_if_addrs::get_if_addrs;
use std::collections::VecDeque;
use crate::sync_logic::LtcState;

/// Check if the ntpd service is active
fn ntp_service_active() -> bool {
    if let Ok(output) = Command::new("systemctl").args(&["is-active", "ntpd"]).output() {
        output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "active"
    } else {
        false
    }
}

/// Toggle the ntpd service (start if `start` is true, stop otherwise)
fn ntp_service_toggle(start: bool) {
    let action = if start { "start" } else { "stop" };
    let _ = Command::new("systemctl").args(&[action, "ntpd"]).status();
}

/// Launch the full-featured TUI; this is a read-only view of the sync status.
pub fn start_ui(
    state: Arc<Mutex<LtcState>>,
    serial_port: String,
) {
    let mut stdout = stdout();
    // Enter alternate screen and hide cursor
    execute!(stdout, EnterAlternateScreen, Hide).unwrap();
    terminal::enable_raw_mode().unwrap();

    // Recent log of messages (last 10)
    let mut logs: VecDeque<String> = VecDeque::with_capacity(10);

    // For caching the timecode delta display once per second
    let mut last_delta_update = Instant::now() - Duration::from_secs(1);
    let mut cached_delta_ms: i64 = 0;
    let mut cached_delta_frames: i64 = 0;

    loop {
        // 1️⃣ Check NTP service status and gather network interfaces
        let ntp_active = ntp_service_active();
        let interfaces: Vec<String> = get_if_addrs()
            .unwrap_or_default()
            .into_iter()
            .filter(|ifa| !ifa.is_loopback())
            .map(|ifa| ifa.ip().to_string())
            .collect();

        // 2️⃣ Compute averages & statuses
        let (avg_ms, avg_frames, status_str, lock_ratio, avg_delta) = {
            let st = state.lock().unwrap();
            (
                st.average_jitter(),
                st.average_frames(),
                st.timecode_match().to_string(),
                st.lock_ratio(),
                st.average_clock_delta(),
            )
        };

        // 3️⃣ Update cached delta once per second
        if last_delta_update.elapsed() >= Duration::from_secs(1) {
            cached_delta_ms = avg_delta;
            // Recompute frames equivalent
            if let Ok(st2) = state.lock() {
                if let Some(frame) = &st2.latest {
                    let ms_pf = 1000.0 / frame.frame_rate;
                    cached_delta_frames = (cached_delta_ms as f64 / ms_pf).round() as i64;
                }
            }
            last_delta_update = Instant::now();
        }

        // 4️⃣ Draw static UI header
        queue!(
            stdout,
            MoveTo(0, 0), Clear(ClearType::All),
            MoveTo(2, 1), Print("NTP Timeturner v2 - Rust Port"),
            MoveTo(2, 2), Print(format!("Using Serial Port: {}", serial_port)),
            MoveTo(2, 3), Print(format!("NTP Server       : {}", if ntp_active { "ACTIVE" } else { "INACTIVE" })),
            MoveTo(2, 4), Print(format!("Interfaces       : {}", interfaces.join(", "))),
        )
        .unwrap();

        // 5️⃣ Draw LTC and System Clock
        if let Ok(st) = state.lock() {
            if let Some(frame) = &st.latest {
                queue!(
                    stdout,
                    MoveTo(2, 6), Print(format!("LTC Status       : {}", frame.status)),
                    MoveTo(2, 7), Print(format!(
                        "LTC Timecode     : {:02}:{:02}:{:02}:{:02}",
                        frame.hours, frame.minutes, frame.seconds, frame.frames
                    )),
                    MoveTo(2, 8), Print(format!("Frame Rate       : {:.2}fps", frame.frame_rate)),
                )
                .unwrap();
            } else {
                queue!(
                    stdout,
                    MoveTo(2, 6), Print("LTC Status       : (waiting)"),
                    MoveTo(2, 7), Print("LTC Timecode     : …"),
                    MoveTo(2, 8), Print("Frame Rate       : …"),
                )
                .unwrap();
            }
            let now_local = Local::now();
            let sys_ts = format!("{:02}:{:02}:{:02}.{:03}",
                now_local.hour(), now_local.minute(), now_local.second(), now_local.timestamp_subsec_millis()
            );
            queue!(stdout, MoveTo(2, 9), Print(format!("System Clock     : {}", sys_ts))).unwrap();
        }

        // 6️⃣ Overlay metrics in new order
        // Timecode Δ line
        let dcol = if cached_delta_ms.abs() < 20 {
            Color::Green
        } else if cached_delta_ms.abs() < 100 {
            Color::Yellow
        } else {
            Color::Red
        };
        queue!(
            stdout,
            MoveTo(2, 11), SetForegroundColor(dcol),
            Print(format!("Timecode Δ       : {:+} ms ({:+} frames)", cached_delta_ms, cached_delta_frames)),
            ResetColor,
        )
        .unwrap();

        // Sync Status line
        let scol = if status_str == "IN SYNC" {
            Color::Green
        } else {
            Color::Red
        };
        queue!(
            stdout,
            MoveTo(2, 12), SetForegroundColor(scol),
            Print(format!("Sync Status      : {}", status_str)),
            ResetColor,
        )
        .unwrap();

        // Sync Jitter line
        let jstatus = if avg_ms.abs() < 10 {
            "GOOD"
        } else if avg_ms.abs() < 40 {
            "AVERAGE"
        } else {
            "BAD"
        };
        let jcol = if jstatus == "GOOD" {
            Color::Green
        } else if jstatus == "AVERAGE" {
            Color::Yellow
        } else {
            Color::Red
        };
        queue!(
            stdout,
            MoveTo(2, 13), SetForegroundColor(jcol),
            Print(format!("Sync Jitter      : {}", jstatus)),
            ResetColor,
        )
        .unwrap();

        // Lock Ratio line
        queue!(stdout,
            MoveTo(2, 14), Print(format!("Lock Ratio       : {:.1}% LOCK", lock_ratio)),
        )
        .unwrap();

        // 7️⃣ Footer and logs
        queue!(stdout,
            MoveTo(2, 16), Print("[S] Set system clock to LTC    [Q] Quit"),
        )
        .unwrap();
        for (i, log_msg) in logs.iter().enumerate() {
            queue!(stdout, MoveTo(2, 18 + i as u16), Print(log_msg)).unwrap();
        }

        stdout.flush().unwrap();

        // 8️⃣ Handle manual sync and quit keys
        if poll(Duration::from_millis(50)).unwrap() {
            if let Event::Key(evt) = read().unwrap() {
                match evt.code {
                    KeyCode::Char(c) if c.eq_ignore_ascii_case(&'q') => {
                        execute!(stdout, Show, LeaveAlternateScreen).unwrap();
                        terminal::disable_raw_mode().unwrap();
                        process::exit(0);
                    }
                    KeyCode::Char(c) if c.eq_ignore_ascii_case(&'s') => {
                        if let Ok(mut stlock) = state.lock() {
                            stlock.manual_sync_request = true;
                        }
                        if logs.len() == 10 {
                            logs.pop_front();
                        }
                        logs.push_back("Manual sync requested...".into());
                    }
                    _ => {}
                }
            }
        }

        thread::sleep(Duration::from_millis(50));
    }
}
