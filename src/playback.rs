use crate::ascii::RleFrame;
use crate::error::AppError;
use crate::metrics::MetricsMonitor;
use crate::terminal::TerminalManager;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

fn reconstruct_frame_string(frame: &RleFrame) -> String {
    if frame.width == 0 || frame.runs.is_empty() {
        return String::new();
    }
    let approx_height = (frame.runs.iter().map(|r| r.count).sum::<usize>() as f32
        / frame.width as f32)
        .ceil() as usize;
    let estimated_capacity =
        (frame.width as usize * approx_height) + (frame.runs.len() * 16) + approx_height;
    let mut buffer = String::with_capacity(estimated_capacity.max(frame.width as usize + 1));
    let mut current_col: u32 = 0;
    let mut current_color: Option<[u8; 3]> = None;

    for run in &frame.runs {
        if current_color != Some(run.color) {
            if current_color.is_some() {
                buffer.push_str("\x1b[0m");
            }
            buffer.push_str(&format!(
                "\x1b[38;2;{};{};{}m",
                run.color[0], run.color[1], run.color[2]
            ));
            current_color = Some(run.color);
        }

        for _ in 0..run.count {
            buffer.push(run.char);
            current_col += 1;

            if current_col >= frame.width {
                buffer.push_str("\x1b[0m");
                buffer.push('\n');
                current_col = 0;
                current_color = None;
            }
        }
    }

    if current_col > 0 {
        buffer.push_str("\x1b[0m");
    }

    if buffer.ends_with('\n') {
        buffer.pop();
    }

    buffer
}

pub struct Player {
    rle_frames: Vec<RleFrame>,
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
        rle_frames: Vec<RleFrame>,
        audio_path: PathBuf,
        original_frame_rate: f32,
        terminal_manager: TerminalManager,
        metrics_monitor: MetricsMonitor,
    ) -> Result<Self, AppError> {
        let num_frames = rle_frames.len();
        if num_frames == 0 {
            log::error!("Cannot create player with zero frames.");
            return Err(AppError::FrameProcessing);
        }

        let audio_duration_result = get_audio_duration(&audio_path);
        let (sync_frame_delay, total_audio_duration, sync_source_msg) = if original_frame_rate > 0.0
        {
            let frame_rate = original_frame_rate;
            let calculated_delay = Duration::from_secs_f32(1.0 / frame_rate);
            let estimated_total_duration = calculated_delay * num_frames as u32;
            let msg = format!("Using provided frame rate {:.3}fps", frame_rate);
            log::info!("Sync Method: {}", msg);
            let display_audio_duration = audio_duration_result.unwrap_or(estimated_total_duration);
            (calculated_delay, display_audio_duration, msg)
        } else if let Ok(audio_duration) = audio_duration_result {
            if !audio_duration.is_zero() {
                let calculated_delay = audio_duration.div_f64(num_frames as f64);
                let msg = format!("Using audio duration ({:?})", audio_duration);
                log::info!("Sync Method: {}", msg);
                (calculated_delay, audio_duration, msg)
            } else {
                log::warn!(
                    "Audio duration is zero, and no valid frame rate. Falling back to 10fps."
                );
                let fallback_rate = 10.0;
                let fallback_delay = Duration::from_secs_f32(1.0 / fallback_rate);
                let fallback_total_duration = fallback_delay * num_frames as u32;
                let msg = "Fallback to 10fps".to_string();
                (fallback_delay, fallback_total_duration, msg)
            }
        } else {
            log::warn!(
                "Failed to get audio duration, and no valid frame rate. Falling back to 10fps."
            );
            let fallback_rate = 10.0;
            let fallback_delay = Duration::from_secs_f32(1.0 / fallback_rate);
            let fallback_total_duration = fallback_delay * num_frames as u32;
            let msg = "Fallback to 10fps".to_string();
            (fallback_delay, fallback_total_duration, msg)
        };
        let sync_frame_delay_secs = sync_frame_delay.as_secs_f64().max(1e-9);

        log::debug!(
            "Player initialized with {} frames. Sync method: [{}]. Sync frame delay: {:?} ({:.6}s). Display duration: {:?}",
            num_frames,
            sync_source_msg,
            sync_frame_delay,
            sync_frame_delay_secs,
            total_audio_duration
        );

        Ok(Player {
            rle_frames,
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
        if self.rle_frames.is_empty() {
            log::warn!("No frames to play.");
            return Ok(());
        }

        let (_stream, stream_handle) = OutputStream::try_default().map_err(|e| {
            log::error!("Failed to get default audio output stream: {}", e);
            AppError::AudioPlayback(rodio::PlayError::NoDevice)
        })?;
        let sink = Sink::try_new(&stream_handle).map_err(|e| {
            log::error!("Failed to create audio sink: {}", e);
            AppError::AudioPlayback(rodio::PlayError::NoDevice)
        })?;

        match File::open(&self.audio_path) {
            Ok(file) => match Decoder::new(BufReader::new(file)) {
                Ok(source) => {
                    sink.append(source);
                    log::info!("Audio loaded from {}", self.audio_path.display());
                    sink.pause();
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

        log::info!("Starting playback for {} frames...", self.rle_frames.len());

        thread::sleep(Duration::from_millis(2000));

        self.terminal_manager.setup()?;
        self.terminal_manager.clear()?;

        sink.play();
        log::debug!("Audio sink playing.");

        self.metrics_monitor.start();

        let playback_start_time = Instant::now();
        log::debug!("Playback loop starting at: {:?}", playback_start_time);

        let num_frames = self.rle_frames.len();
        let mut idx: usize = 0;

        let mut frame_finish_times: VecDeque<Instant> = VecDeque::with_capacity(128);

        while idx < num_frames {
            if TerminalManager::check_for_exit()? || self.stop_signal.load(Ordering::Relaxed) {
                self.stop_signal.store(true, Ordering::Relaxed);
                log::info!("Playback interrupted.");
                break;
            }

            let target_display_time = playback_start_time + self.sync_frame_delay * (idx as u32);
            let mut now_before_wait = Instant::now();

            if now_before_wait < target_display_time {
                let remaining_wait = target_display_time.saturating_duration_since(now_before_wait);
                if !remaining_wait.is_zero() {
                    thread::sleep(remaining_wait);
                }
                now_before_wait = Instant::now();
            }

            let rle_frame_to_draw = &self.rle_frames[idx];
            let frame_string = reconstruct_frame_string(rle_frame_to_draw);

            let actual_elapsed_playback_time =
                now_before_wait.saturating_duration_since(playback_start_time);
            let time_str = format_duration(actual_elapsed_playback_time);
            let total_time_str = format_duration(self.total_audio_duration);
            let metrics_text = self.metrics_monitor.get_metrics();

            let now_for_fps = Instant::now();
            while let Some(first_time) = frame_finish_times.front() {
                if now_for_fps.duration_since(*first_time) > Duration::from_secs(1) {
                    frame_finish_times.pop_front();
                } else {
                    break;
                }
            }
            let current_fps = frame_finish_times.len() as f32;

            let status_line = format!(
                "Time: {} / {} | Frame: {}/{} | FPS: {:>6.1} | {}",
                time_str,
                total_time_str,
                idx + 1,
                num_frames,
                current_fps,
                metrics_text
            );

            let (cols, _lines) = TerminalManager::get_size()?;

            let status_bar_content = format!("[{}]", status_line);
            let status_bar_trimmed = if status_bar_content.chars().count() > cols as usize {
                status_bar_content
                    .chars()
                    .take(cols as usize)
                    .collect::<String>()
            } else {
                status_bar_content
            };
            let padding_total = cols.saturating_sub(status_bar_trimmed.chars().count() as u16);
            let padding_left = padding_total / 2;
            let padding_right = padding_total - padding_left;
            let centered_status = format!(
                "{}{}{}",
                "=".repeat(padding_left as usize),
                status_bar_trimmed,
                "=".repeat(padding_right as usize)
            );

            let output_buffer = format!("{}\n{}", frame_string, centered_status);
            self.terminal_manager.draw(&output_buffer)?;

            let time_after_draw = Instant::now();
            frame_finish_times.push_back(time_after_draw);

            let next_frame_target_time =
                playback_start_time + self.sync_frame_delay * (idx as u32 + 1);

            if time_after_draw > next_frame_target_time {
                let lag_duration =
                    time_after_draw.saturating_duration_since(next_frame_target_time);
                let num_frames_to_skip_float = if self.sync_frame_delay_secs > 0.0 {
                    lag_duration.as_secs_f64() / self.sync_frame_delay_secs
                } else {
                    0.0
                };

                let num_frames_to_skip = num_frames_to_skip_float.floor() as usize;

                if num_frames_to_skip > 0 {
                    log::debug!(
                        "Lag detected: {:?}. Skipping {} frame(s). (Current: {}, Next target: {})",
                        lag_duration,
                        num_frames_to_skip,
                        idx + 1,
                        idx + num_frames_to_skip + 1 + 1
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

        self.stop_signal.store(true, Ordering::Relaxed);
        self.metrics_monitor.stop();
        sink.stop();

        log::info!("Playback loop finished.");
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
    let source = Decoder::new(BufReader::new(file)).map_err(AppError::AudioDecode)?;

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
                "Could not determine exact duration for audio file: {}. Timing might be based on frame rate.",
                audio_path.display()
            );
            Ok(Duration::ZERO)
        }
    }
}
