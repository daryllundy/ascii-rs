use crate::config::METRICS_UPDATE_INTERVAL;
use crate::error::AppError;
// Need ProcessesToUpdate enum
use std::sync::{Arc, Mutex};
use std::thread;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System}; // Keep ProcessesToUpdate

pub struct MetricsMonitor {
    system: Arc<Mutex<System>>,
    pid: Pid,
    metrics_str: Arc<Mutex<String>>,
    running: Arc<Mutex<bool>>,
}

impl MetricsMonitor {
    pub fn new() -> Result<Self, AppError> {
        let mut system = System::new_all();
        system.refresh_all();
        let pid = sysinfo::get_current_pid().map_err(|e| AppError::SystemInfo(e.to_string()))?;

        if system.process(pid).is_none() {
            log::warn!(
                "Failed to find current process (PID: {}) after initial refresh. Retrying refresh_all...",
                pid
            );
            system.refresh_all();
            if system.process(pid).is_none() {
                return Err(AppError::SystemInfo(format!(
                    "Could not find own process PID {}",
                    pid
                )));
            }
        }

        Ok(MetricsMonitor {
            system: Arc::new(Mutex::new(system)),
            pid,
            metrics_str: Arc::new(Mutex::new("Metrics initializing...".to_string())),
            running: Arc::new(Mutex::new(false)),
        })
    }

    pub fn start(&self) {
        let system_arc = Arc::clone(&self.system);
        let metrics_str_arc = Arc::clone(&self.metrics_str);
        let running_arc = Arc::clone(&self.running);
        let pid = self.pid;

        *running_arc.lock().unwrap() = true;

        thread::spawn(move || {
            log::debug!("Metrics thread started.");
            while *running_arc.lock().unwrap() {
                let mut sys_guard = system_arc.lock().unwrap();

                // --- CORRECTED CALL for sysinfo 0.34 (using plural + Some variant) ---
                let refresh_kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
                // Create array first to extend its lifetime
                let pids_array = [pid];
                // Use ProcessesToUpdate::Some with a slice containing the pid
                let pids_to_update = ProcessesToUpdate::Some(&pids_array);
                // Call the plural version, passing the correct enum variant
                let updated_count: usize = (&mut *sys_guard).refresh_processes_specifics(
                    pids_to_update,
                    false, // Don't remove dead processes
                    refresh_kind,
                );
                // --- End Corrected Call ---

                // Check if *our* process was updated (updated_count > 0 implies it was)
                let refreshed = updated_count > 0;

                let (mem_usage_mb, cpu_usage) = if refreshed {
                    // Access process using the guard (immutable deref)
                    if let Some(proc) = sys_guard.process(pid) {
                        let mem_usage_bytes = proc.memory();
                        let mem_usage_mb = mem_usage_bytes as f64 / 1024.0 / 1024.0;
                        let cpu_usage = proc.cpu_usage();
                        (mem_usage_mb, cpu_usage)
                    } else {
                        log::warn!(
                            "Process PID {} was updated but not found immediately after.",
                            pid
                        );
                        (0.0, 0.0)
                    }
                } else {
                    // This now means the process likely didn't exist or wasn't updated
                    log::trace!(
                        "Process PID {} not found or not updated during refresh.",
                        pid
                    );
                    (0.0, 0.0)
                };

                let new_metrics =
                    format!("Memory: {:6.2} MB | CPU: {:5.2}%", mem_usage_mb, cpu_usage);

                *metrics_str_arc.lock().unwrap() = new_metrics;

                drop(sys_guard); // Release lock

                if *running_arc.lock().unwrap() {
                    thread::sleep(METRICS_UPDATE_INTERVAL);
                }
            }
            log::debug!("Metrics thread finished.");
        });
    }

    pub fn stop(&self) {
        *self.running.lock().unwrap() = false;
        log::debug!("Metrics stop signal sent.");
    }

    pub fn get_metrics(&self) -> String {
        self.metrics_str.lock().unwrap().clone()
    }
}
