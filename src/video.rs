use crate::error::AppError;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use tempfile::Builder;

#[derive(Debug)]
#[allow(dead_code)]
pub struct VideoInfo {
    pub video_path: PathBuf,
    pub frame_rate: f32,
    pub total_frames: u64,
    pub duration: Duration,
    pub width: u32,
    pub height: u32,
    pub base_name: String,
    pub data_dir: PathBuf,
    pub frames_dir: PathBuf,
    pub audio_path: PathBuf,
    pub ascii_cache_path: PathBuf,
}

impl VideoInfo {
    pub fn analyze(video_path: &Path, terminal_size: (u16, u16)) -> Result<Self, AppError> {
        if !video_path.exists() {
            return Err(AppError::VideoNotFound(video_path.to_path_buf()));
        }

        let base_name = video_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video")
            .to_string();

        let temp_frames_dir = Builder::new()
            .prefix(&format!("ascii_frames_{}", base_name))
            .tempdir()
            .map_err(|e| AppError::Io(e))?;
        let frames_dir_path = temp_frames_dir.path().to_path_buf();
        std::mem::forget(temp_frames_dir);

        let data_dir = PathBuf::from("data").join(&base_name);
        fs::create_dir_all(&data_dir).map_err(|e| AppError::CreateDir(data_dir.clone(), e))?;

        let audio_path = data_dir.join(format!("{}.wav", base_name));
        let ascii_cache_path = data_dir.join(format!(
            "ascii_frames_{}-{}.acsv",
            terminal_size.0, terminal_size.1
        ));

        log::info!("Running ffprobe for metadata...");
        let output: Output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height,r_frame_rate,nb_frames",
                "-of",
                "csv=p=0:s=,",
                video_path.to_str().ok_or_else(|| {
                    AppError::FFprobe("Video path contains invalid UTF-8".to_string())
                })?,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| AppError::FFprobe(format!("Failed to execute ffprobe: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::FFprobe(format!("ffprobe failed: {}", stderr)));
        }

        let metadata_str = String::from_utf8(output.stdout)
            .map_err(|_| AppError::FFprobe("ffprobe output is not valid UTF-8".to_string()))?;
        let parts: Vec<&str> = metadata_str.trim().split(',').collect();

        log::debug!("ffprobe output: {:?}", parts);

        if parts.len() != 4 {
            return Err(AppError::VideoMetadata(video_path.to_path_buf()));
        }

        let width: u32 = parts[0].parse()?;
        let height: u32 = parts[1].parse()?;
        let frame_rate_str = parts[2];
        let frame_rate = if frame_rate_str.contains('/') {
            let nums: Vec<&str> = frame_rate_str.split('/').collect();
            if nums.len() == 2 {
                nums[0].parse::<f32>()? / nums[1].parse::<f32>()?
            } else {
                log::warn!(
                    "Could not parse frame rate fraction '{}', using 30.0",
                    frame_rate_str
                );
                30.0
            }
        } else {
            frame_rate_str.parse::<f32>()?
        };

        let total_frames = parts[3].parse::<u64>()?;

        let duration = Duration::from_secs_f32(total_frames as f32 / frame_rate);

        if width == 0 || height == 0 || frame_rate <= 0.0 || total_frames == 0 {
            log::error!(
                "Parsed metadata invalid: w={}, h={}, fps={}, frames={}, duration={:?}",
                width,
                height,
                frame_rate,
                total_frames,
                duration
            );
            return Err(AppError::VideoMetadata(video_path.to_path_buf()));
        }

        log::info!(
            "Video Info: {}x{} @ {:.2}fps, {} frames, {:.2}s",
            width,
            height,
            frame_rate,
            total_frames,
            duration.as_secs_f32()
        );

        Ok(VideoInfo {
            video_path: video_path.to_path_buf(),
            frame_rate,
            total_frames,
            duration,
            width,
            height,
            base_name,
            data_dir,
            frames_dir: frames_dir_path,
            audio_path,
            ascii_cache_path,
        })
    }

    pub fn extract_audio(&self) -> Result<(), AppError> {
        log::info!("Extracting audio to {}...", self.audio_path.display());
        let start_time = std::time::Instant::now();

        let child = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "quiet",
                "-i",
                self.video_path.to_str().unwrap(),
                "-vn",
                "-f",
                "wav",
                "-ar",
                "44100",
                "-ac",
                "2",
                self.audio_path.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::FFmpeg(format!("Failed to start ffmpeg for audio: {}", e)))?;

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner} Extracting audio...")
                .unwrap(),
        );
        spinner.enable_steady_tick(Duration::from_millis(100));

        let output = child
            .wait_with_output()
            .map_err(|e| AppError::FFmpeg(format!("Failed to wait for ffmpeg (audio): {}", e)))?;

        spinner.finish_and_clear();

        if !output.status.success() {
            let stderr_output = String::from_utf8_lossy(&output.stderr);
            log::error!("FFmpeg audio extraction failed: {}", stderr_output);
            return Err(AppError::FFmpeg(format!(
                "Audio extraction failed (code {}): {}",
                output.status.code().unwrap_or(-1),
                stderr_output
            )));
        }

        log::info!(
            "Audio extracted successfully in {:.2}s",
            start_time.elapsed().as_secs_f64()
        );
        Ok(())
    }

    pub fn extract_frames(&self) -> Result<Vec<PathBuf>, AppError> {
        log::info!("Extracting frames to {}...", self.frames_dir.display());
        let start_time = std::time::Instant::now();

        fs::create_dir_all(&self.frames_dir)
            .map_err(|e| AppError::CreateDir(self.frames_dir.clone(), e))?;

        let frame_pattern = self.frames_dir.join("frame_%06d.png");

        let mut child = Command::new("ffmpeg")
            .args([
                "-i",
                self.video_path.to_str().unwrap(),
                "-vf",
                &format!("fps={}", self.frame_rate),
                "-loglevel",
                "quiet",
                frame_pattern.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::FFmpeg(format!("Failed to start ffmpeg for frames: {}", e)))?;

        let pb = ProgressBar::new(self.total_frames);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
                )
                .unwrap()
                .progress_chars("#>-"),
        );

        while match child.try_wait() {
            Ok(Some(status)) => {
                log::debug!("FFmpeg frame extraction finished with status: {}", status);
                false
            }
            Ok(None) => true,
            Err(e) => {
                pb.finish_and_clear();
                return Err(AppError::FFmpeg(format!(
                    "Error waiting for ffmpeg (frames): {}",
                    e
                )));
            }
        } {
            if let Ok(entries) = fs::read_dir(&self.frames_dir) {
                let count = entries.filter_map(Result::ok).count() as u64;
                pb.set_position(count.min(self.total_frames));
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        let output = child.wait_with_output().map_err(|e| {
            AppError::FFmpeg(format!(
                "Failed waiting for ffmpeg final status (frames): {}",
                e
            ))
        })?;

        pb.set_position(self.total_frames);
        pb.finish_and_clear();

        if !output.status.success() {
            let stderr_output = String::from_utf8_lossy(&output.stderr);
            log::error!("FFmpeg frame extraction failed: {}", stderr_output);
            return Err(AppError::FFmpeg(format!(
                "Frame extraction failed (code {}): {}",
                output.status.code().unwrap_or(-1),
                stderr_output
            )));
        }

        let mut frame_paths: Vec<PathBuf> = fs::read_dir(&self.frames_dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().map_or(false, |ext| ext == "png"))
            .collect();

        frame_paths.sort_by_key(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.split('_').last())
                .and_then(|num_str| num_str.parse::<u32>().ok())
                .unwrap_or(u32::MAX)
        });

        let min_expected_frames = (self.total_frames as f64 * 0.95).floor() as usize;
        if frame_paths.len() < min_expected_frames && self.total_frames > 0 {
            log::warn!(
                "Expected ~{} frames (min {}), but found only {}. Playback might be incomplete or end abruptly.",
                self.total_frames,
                min_expected_frames,
                frame_paths.len()
            );
        } else if frame_paths.len() > (self.total_frames as usize + 10) && self.total_frames > 0 {
            log::warn!(
                "Found significantly more frames ({}) than expected ({}). This might indicate an issue.",
                frame_paths.len(),
                self.total_frames
            );
        }

        log::info!(
            "Frames extracted successfully in {:.2}s",
            start_time.elapsed().as_secs_f64()
        );
        Ok(frame_paths)
    }
}
