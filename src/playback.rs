use crate::error::AppError;
use crate::metrics::MetricsMonitor;
use crate::terminal::TerminalManager;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

const SLEEP_THRESHOLD: Duration = Duration::from_millis(2);
const YIELD_THRESHOLD: Duration = Duration::from_micros(100);

pub struct Player {
    ascii_frames: Vec<String>,
    audio_path: PathBuf,
    sync_frame_delay: Duration,
    sync_frame_delay_secs: f64,
    total_audio_duration: Duration,
    terminal_manager: TerminalManager,
    metrics_monitor: MetricsMonitor,
    pub stop_signal: Arc<AtomicBool>,
}

impl Player {
    pub fn new(
        ascii_frames: Vec<String>,
        audio_path: PathBuf,
        original_frame_rate: f32,
        terminal_manager: TerminalManager,
        metrics_monitor: MetricsMonitor,
    ) -> Result<Self, AppError> {
        let num_frames = ascii_frames.len();
        if num_frames == 0 {
            log::error!("Cannot create player with zero frames.");
            return Err(AppError::FrameProcessing);
        }

        let audio_duration_result = get_audio_duration(&audio_path);
        let (sync_frame_delay, total_audio_duration) = match audio_duration_result {
            Ok(audio_duration) if !audio_duration.is_zero() => {
                let calculated_delay = audio_duration.div_f64(num_frames as f64);
                log::info!(
                    "Using audio duration ({:?}) for sync. Calculated frame delay: {:?}",
                    audio_duration,
                    calculated_delay
                );
                (calculated_delay, audio_duration)
            }
            _ => {
                log::warn!(
                    "Failed to get valid audio duration or duration is zero. Falling back to frame rate {:.3}fps for sync.",
                    original_frame_rate
                );
                let fallback_rate = original_frame_rate.max(0.1);
                let fallback_delay = Duration::from_secs_f32(1.0 / fallback_rate);
                let fallback_total_duration = fallback_delay * num_frames as u32;
                (fallback_delay, fallback_total_duration)
            }
        };
        let sync_frame_delay_secs = sync_frame_delay.as_secs_f64().max(1e-9);

        log::debug!(
            "Player initialized with {} frames. Sync frame delay: {:?} ({:.6}s). Total sync duration: {:?}",
            num_frames,
            sync_frame_delay,
            sync_frame_delay_secs,
            total_audio_duration
        );

        Ok(Player {
            ascii_frames,
            audio_path,
            sync_frame_delay,
            sync_frame_delay_secs,
            total_audio_duration,
            terminal_manager,
            metrics_monitor,
            stop_signal: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn play(&mut self) -> Result<(), AppError> {
        if self.ascii_frames.is_empty() {
            return Ok(());
        }

        let (_stream, stream_handle) = OutputStream::try_default()
            .map_err(|_| AppError::AudioPlayback(rodio::PlayError::NoDevice))?;
        let sink = Sink::try_new(&stream_handle)
            .map_err(|_| AppError::AudioPlayback(rodio::PlayError::NoDevice))?;

        match File::open(&self.audio_path) {
            Ok(file) => match Decoder::new(BufReader::new(file)) {
                Ok(source) => {
                    sink.append(source);
                    log::info!("Audio loaded from {}", self.audio_path.display());
                }
                Err(e) => log::warn!(
                    "Failed to decode audio file {}: {}. Silent playback.",
                    self.audio_path.display(),
                    e
                ),
            },
            Err(e) => log::warn!(
                "Failed to open audio file {}: {}. Silent playback.",
                self.audio_path.display(),
                e
            ),
        }

        log::info!(
            "Starting playback for {} frames...",
            self.ascii_frames.len()
        );

        self.terminal_manager.setup()?;

        thread::sleep(Duration::from_millis(500));

        self.terminal_manager.clear()?;

        sink.play();
        log::debug!("Audio sink playing.");

        self.metrics_monitor.start();

        let playback_start_time = Instant::now();
        log::debug!("Playback loop starting at: {:?}", playback_start_time);

        let num_frames = self.ascii_frames.len();
        let mut last_frame_actual_finish_time = Instant::now();
        let mut idx: usize = 0;

        while idx < num_frames {
            if TerminalManager::check_for_exit()? || self.stop_signal.load(Ordering::Relaxed) {
                log::info!("Playback interrupted by user.");
                self.stop_signal.store(true, Ordering::Relaxed);
                break;
            }

            let target_display_time = playback_start_time + self.sync_frame_delay * (idx as u32);

            let mut now = Instant::now();
            if now < target_display_time {
                while now < target_display_time {
                    let remaining_wait = target_display_time.saturating_duration_since(now);

                    if remaining_wait > SLEEP_THRESHOLD {
                        thread::sleep(remaining_wait.saturating_sub(Duration::from_millis(1)));
                    } else if remaining_wait > YIELD_THRESHOLD {
                        thread::yield_now();
                    } else if !remaining_wait.is_zero() {
                        std::hint::spin_loop();
                    }
                    now = Instant::now();
                }
            }

            let actual_elapsed_since_last_frame = last_frame_actual_finish_time.elapsed();
            let actual_elapsed_playback_time = now.saturating_duration_since(playback_start_time);
            let current_fps = if actual_elapsed_since_last_frame.as_secs_f32() > 1e-6 {
                1.0 / actual_elapsed_since_last_frame.as_secs_f32()
            } else {
                f32::INFINITY
            };
            let time_str = format_duration(actual_elapsed_playback_time);
            let total_time_str = format_duration(self.total_audio_duration);
            let metrics_text = self.metrics_monitor.get_metrics();
            let status_line = format!(
                "Time: {} / {} | Frame: {}/{} | FPS: {:>6.1} | {}",
                time_str,
                total_time_str,
                idx + 1,
                num_frames,
                current_fps,
                metrics_text
            );

            let frame = &self.ascii_frames[idx];
            let (cols, _lines) = TerminalManager::get_size()?;
            let status_bar = format!("\x1b[0m[{}]", status_line);
            let status_bar_trimmed = if status_bar.chars().count() > cols as usize {
                status_bar.chars().take(cols as usize).collect::<String>()
            } else {
                status_bar
            };
            let padding_total = cols.saturating_sub(status_bar_trimmed.chars().count() as u16 + 2);
            let padding_left = padding_total / 2;
            let padding_right = padding_total - padding_left;
            let centered_status = format!(
                "{}{}{}",
                "=".repeat(padding_left as usize),
                status_bar_trimmed,
                "=".repeat(padding_right as usize)
            );
            let output_buffer = format!("{}\n{}", frame, centered_status);

            self.terminal_manager.draw(&output_buffer)?;
            last_frame_actual_finish_time = Instant::now();

            let time_after_draw = last_frame_actual_finish_time;
            let next_frame_target_time =
                playback_start_time + self.sync_frame_delay * (idx as u32 + 1);

            if time_after_draw > next_frame_target_time {
                let lag_duration =
                    time_after_draw.saturating_duration_since(next_frame_target_time);
                let num_frames_to_skip_float =
                    lag_duration.as_secs_f64() / self.sync_frame_delay_secs;
                let num_frames_to_skip = num_frames_to_skip_float.floor() as usize;

                if num_frames_to_skip > 0 {
                    log::debug!(
                        "Lag detected: {:?}. Skipping {} frame(s). (Current: {}, Next target: {})",
                        lag_duration,
                        num_frames_to_skip,
                        idx + 1,
                        idx + 1 + num_frames_to_skip + 1
                    );
                    idx += num_frames_to_skip + 1;
                } else {
                    idx += 1;
                }
            } else {
                idx += 1;
            }
            idx = idx.min(num_frames);
        }

        log::info!("Playback finished.");
        self.stop_signal.store(true, Ordering::Relaxed);
        self.metrics_monitor.stop();
        sink.stop();
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }
}

fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

fn get_audio_duration(audio_path: &PathBuf) -> Result<Duration, AppError> {
    let file = File::open(audio_path)?;
    let source = Decoder::new(BufReader::new(file))?;
    match source.total_duration() {
        Some(duration) => {
            log::debug!(
                "Got audio duration {:?} for {}",
                duration,
                audio_path.display()
            );
            Ok(duration)
        }
        None => {
            log::warn!(
                "Could not determine exact duration for audio file: {}",
                audio_path.display()
            );
            Ok(Duration::ZERO)
        }
    }
}
