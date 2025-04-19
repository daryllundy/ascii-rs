use crate::ascii::RleFrame;
use crate::config::ASCII_CHARS;
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
    let total_chars: usize = frame.runs.iter().map(|r| r.count as usize).sum();
    let approx_height = (total_chars as f32 / frame.width as f32).ceil() as usize;
    let estimated_capacity = total_chars + frame.runs.len() * 8 + approx_height;
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
        let ch = ASCII_CHARS
            .get(run.ascii_idx as usize)
            .copied()
            .unwrap_or(' ');
        for _ in 0..run.count {
            buffer.push(ch);
            current_col += 1;
            if current_col >= frame.width as u32 {
                buffer.push('\n');
                current_col = 0;
            }
        }
    }

    if current_color.is_some() {
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
        if rle_frames.is_empty() {
            return Err(AppError::FrameProcessing);
        }

        let num_frames = rle_frames.len();
        let audio_duration = get_audio_duration(&audio_path).ok();

        let (sync_frame_delay, total_audio_duration) = if original_frame_rate > 0.0 {
            let d = Duration::from_secs_f32(1.0 / original_frame_rate);
            (d, audio_duration.unwrap_or(d * num_frames as u32))
        } else if let Some(dur) = audio_duration {
            if !dur.is_zero() {
                (dur.div_f64(num_frames as f64), dur)
            } else {
                let d = Duration::from_secs_f32(1.0 / 10.0);
                (d, d * num_frames as u32)
            }
        } else {
            let d = Duration::from_secs_f32(1.0 / 10.0);
            (d, d * num_frames as u32)
        };

        Ok(Self {
            rle_frames,
            audio_path,
            sync_frame_delay,
            total_audio_duration,
            terminal_manager,
            metrics_monitor,
            stop_signal: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn play(&mut self) -> Result<(), AppError> {
        if self.rle_frames.is_empty() {
            return Ok(());
        }

        let (_stream, handle) = OutputStream::try_default()
            .map_err(|_| AppError::AudioPlayback(rodio::PlayError::NoDevice))?;
        let sink = Sink::try_new(&handle)
            .map_err(|_| AppError::AudioPlayback(rodio::PlayError::NoDevice))?;
        if let Ok(file) = File::open(&self.audio_path) {
            if let Ok(src) = Decoder::new(BufReader::new(file)) {
                sink.append(src);
                sink.pause();
            }
        }
        thread::sleep(Duration::from_millis(2000));
        self.terminal_manager.setup()?;
        self.terminal_manager.clear()?;
        sink.play();
        self.metrics_monitor.start();
        let start = Instant::now();
        let mut idx = 0;
        let mut times = VecDeque::with_capacity(128);
        while idx < self.rle_frames.len() && !self.stop_signal.load(Ordering::Relaxed) {
            if TerminalManager::check_for_exit()? {
                break;
            }
            let target = start + self.sync_frame_delay * (idx as u32);
            let now = Instant::now();
            if now < target {
                thread::sleep(target - now);
            }
            let frame_str = reconstruct_frame_string(&self.rle_frames[idx]);
            let elapsed = Instant::now().saturating_duration_since(start);
            let fps = times
                .iter()
                .filter(|&&t| elapsed - t < Duration::from_secs(1))
                .count() as f32;
            let status = format!(
                "Time: {} / {} | Frame: {}/{} | FPS: {:.1} | {}",
                format_duration(elapsed),
                format_duration(self.total_audio_duration),
                idx + 1,
                self.rle_frames.len(),
                fps,
                self.metrics_monitor.get_metrics()
            );
            let (cols, _) = TerminalManager::get_size()?;
            let bar = status.chars().count().min(cols as usize);
            let padding_total = cols.saturating_sub(bar as u16);
            let padding_left = padding_total / 2;
            let padding_right = padding_total - padding_left;
            let centered = format!(
                "{}{}{}",
                "=".repeat(padding_left as usize),
                status,
                "=".repeat(padding_right as usize)
            );
            self.terminal_manager
                .draw(&format!("{}{}", frame_str, centered))?;
            times.push_back(Instant::now().saturating_duration_since(start));
            if times.len() > 128 {
                times.pop_front();
            }
            idx += 1;
        }
        self.stop_signal.store(true, Ordering::Relaxed);
        self.metrics_monitor.stop();
        sink.stop();
        Ok(())
    }
}

fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let s = s % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

fn get_audio_duration(path: &PathBuf) -> Result<Duration, AppError> {
    let file = File::open(path)?;
    let dec = Decoder::new(BufReader::new(file)).map_err(AppError::AudioDecode)?;
    Ok(dec.total_duration().unwrap_or(Duration::ZERO))
}
