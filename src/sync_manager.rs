
use crate::sync_logic::LtcState;
use chrono::{
    DateTime, Duration as ChronoDuration, Local, NaiveTime, TimeZone, Timelike, Utc,
};
use std::{
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

/// Helper to format a DateTime for the `date` command.
fn format_for_date_cmd(dt: DateTime<Local>) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        dt.hour(),
        dt.minute(),
        dt.second(),
        dt.timestamp_subsec_millis()
    )
}

/// Sets the system time using `sudo date -s HH:MM:SS.ms`.
fn set_system_time(ts: &str) -> Result<(), std::io::Error> {
    let status = Command::new("sudo").arg("date").arg("-s").arg(ts).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "date command failed",
        ))
    }
}

/// The core logic loop that was previously in `ui.rs`.
/// This runs in a dedicated thread and handles all time synchronization.
pub fn start_sync_manager(state: Arc<Mutex<LtcState>>, offset: Arc<Mutex<i64>>) {
    let mut out_of_sync_since: Option<Instant> = None;

    loop {
        // 1️⃣ Read hardware offset from watcher
        let hw_offset_ms = *offset.lock().unwrap();

        // 2️⃣ Grab a clone of the latest frame
        let latest_frame = state.lock().unwrap().latest.clone();

        // 3️⃣ Measure & record jitter and Timecode Δ when LOCKED; clear on FREE
        if let Some(frame) = latest_frame {
            let mut st = state.lock().unwrap();
            if frame.status == "LOCK" {
                // Jitter in ms
                let now = Utc::now();
                let raw = (now - frame.timestamp).num_milliseconds();
                let measured = raw - hw_offset_ms;
                st.record_offset(measured);

                // Timecode delta: how far system clock differs from LTC
                let local = Local::now();
                let sub_ms = ((frame.frames as f64 / frame.frame_rate) * 1000.0).round() as i64;
                let base_time =
                    NaiveTime::from_hms_opt(frame.hours, frame.minutes, frame.seconds)
                        .unwrap_or_else(|| local.time());
                let offset_dt =
                    local.date_naive().and_time(base_time) + ChronoDuration::milliseconds(sub_ms);
                let ltc_dt = Local
                    .from_local_datetime(&offset_dt)
                    .single()
                    .unwrap_or(local);
                let delta_ms = local.signed_duration_since(ltc_dt).num_milliseconds();
                st.record_clock_delta(delta_ms);
            } else {
                st.clear_offsets();
                st.clear_clock_deltas();
            }
        }

        // 4️⃣ Handle manual sync request
        let mut st = state.lock().unwrap();
        if st.manual_sync_request {
            if let Some(frame) = &st.latest {
                let local_now = Local::now();
                let sub_ms =
                    ((frame.frames as f64 / frame.frame_rate) * 1000.0).round() as i64;
                let base_time =
                    NaiveTime::from_hms_opt(frame.hours, frame.minutes, frame.seconds)
                        .unwrap_or_else(|| local_now.time());
                let offset_dt = local_now.date_naive().and_time(base_time)
                    + ChronoDuration::milliseconds(sub_ms);
                let ltc_dt = Local
                    .from_local_datetime(&offset_dt)
                    .single()
                    .unwrap_or(local_now);
                let ts = format_for_date_cmd(ltc_dt);
                let _ = set_system_time(&ts); // TODO: log result
            }
            st.manual_sync_request = false;
        }
        drop(st); // release lock

        // 5️⃣ Auto-sync if "OUT OF SYNC" or Δ >10ms for 5s
        let (status_str, avg_delta) = {
            let st = state.lock().unwrap();
            (st.timecode_match().to_string(), st.average_clock_delta())
        };

        if status_str == "OUT OF SYNC" || avg_delta.abs() > 10 {
            if let Some(start) = out_of_sync_since {
                if start.elapsed() >= Duration::from_secs(5) {
                    if let Some(frame) = state.lock().unwrap().latest.clone() {
                        let local_now = Local::now();
                        let sub_ms =
                            ((frame.frames as f64 / frame.frame_rate) * 1000.0).round() as i64;
                        let base_time = NaiveTime::from_hms_opt(
                            frame.hours,
                            frame.minutes,
                            frame.seconds,
                        )
                        .unwrap_or_else(|| local_now.time());
                        let offset_dt = local_now.date_naive().and_time(base_time)
                            + ChronoDuration::milliseconds(sub_ms);
                        let ltc_dt = Local
                            .from_local_datetime(&offset_dt)
                            .single()
                            .unwrap_or(local_now);
                        let ts = format_for_date_cmd(ltc_dt);
                        let _ = set_system_time(&ts); // TODO: log result
                    }
                    out_of_sync_since = None;
                }
            } else {
                out_of_sync_since = Some(Instant::now());
            }
        } else {
            out_of_sync_since = None;
        }

        thread::sleep(Duration::from_millis(50));
    }
}
