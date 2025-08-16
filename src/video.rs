use crate::{error::AppError, utils::get_file_stem};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, info};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use tempfile::TempDir;

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
    pub frames_dir: TempDir,
    pub audio_path: PathBuf,
    pub ascii_cache_path: PathBuf,
}

impl VideoInfo {
    pub fn analyze(video_path: &Path, terminal_size: (u16, u16)) -> Result<Self, AppError> {
        if !video_path.is_file() {
            return Err(AppError::VideoNotFound(video_path.to_path_buf()));
        }

        let base_name = get_file_stem(video_path);
        info!("Analyzing video: {}", video_path.display());

        let frames_dir = tempfile::Builder::new()
            .prefix(&format!("frames_{}", base_name))
            .tempdir()
            .map_err(|e| {
                error!("Failed to create temp directory for frames: {}", e);
                AppError::Io {
                    source: e,
                    context: Some("tempdir creation".to_string()),
                }
            })?;

        debug!("Created temporary directory at: {:?}", frames_dir.path());

        let data_dir = PathBuf::from("data").join(&base_name);
        fs::create_dir_all(&data_dir).map_err(|e| {
            error!("Failed to create data directory: {}", e);
            AppError::Io {
                source: e,
                context: Some(data_dir.display().to_string()),
            }
        })?;

        debug!("Created data directory at: {}", data_dir.display());

        let audio_path = data_dir.join("audio.wav");
        let ascii_cache_path = data_dir.join(format!(
            "frames_{}-{}.acsv",
            terminal_size.0, terminal_size.1
        ));

        let output = Command::new("ffprobe")
            .args(&[
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height,r_frame_rate,nb_frames",
                "-of",
                "csv=p=0:s=,",
                video_path
                    .to_str()
                    .ok_or_else(|| AppError::VideoMetadata(video_path.to_path_buf()))?,
            ])
            .output()
            .map_err(|e| AppError::FFprobe(format!("Failed to execute ffprobe: {}", e)))?;

        if !output.status.success() {
            return Err(AppError::FFprobe(
                String::from_utf8_lossy(&output.stderr).into(),
            ));
        }

        let binding = String::from_utf8(output.stdout).map_err(|e| AppError::Utf8 {
            source: e,
            context: Some("ffprobe output".to_string()),
        })?;
        let parts: Vec<&str> = binding.trim().split(',').collect();
        if parts.len() != 4 {
            return Err(AppError::VideoMetadata(video_path.to_path_buf()));
        }

        let width: u32 = parts[0].parse().map_err(|e| AppError::ParseInt {
            source: e,
            context: Some("width parse".to_string()),
        })?;
        let height: u32 = parts[1].parse().map_err(|e| AppError::ParseInt {
            source: e,
            context: Some("height parse".to_string()),
        })?;
        let frame_rate = parse_fps(parts[2]);
        let total_frames: u64 = parts[3].parse().map_err(|e| AppError::ParseInt {
            source: e,
            context: Some("total_frames parse".to_string()),
        })?;
        let duration = Duration::from_secs_f32(total_frames as f32 / frame_rate);

        Ok(VideoInfo {
            video_path: video_path.to_path_buf(),
            frame_rate,
            total_frames,
            duration,
            width,
            height,
            base_name,
            data_dir,
            frames_dir,
            audio_path,
            ascii_cache_path,
        })
    }

    pub fn extract_audio(&self) -> Result<(), AppError> {
        let status = Command::new("ffmpeg")
            .args(&[
                "-y",
                "-i",
                self.video_path.to_str().unwrap(),
                "-vn",
                "-ar",
                "44100",
                "-ac",
                "2",
                "-loglevel",
                "quiet",
                self.audio_path.to_str().unwrap(),
            ])
            .status()
            .map_err(|e| AppError::FFmpeg(format!("Failed to run ffmpeg: {}", e)))?;
        if status.success() {
            Ok(())
        } else {
            if status.code().unwrap() == -22 {
                Ok(())
            } else {
                Err(AppError::FFmpeg(format!(
                    "Audio extraction failed with code {:?}",
                    status.code()
                )))
            }
        }
    }

    pub fn extract_frames(&self) -> Result<Vec<PathBuf>, AppError> {
        fs::create_dir_all(self.frames_dir.path()).map_err(|e| AppError::Io {
            source: e,
            context: Some(self.frames_dir.path().display().to_string()),
        })?;
        let pattern = self.frames_dir.path().join("frame_%06d.png");

        let pb = ProgressBar::new(self.total_frames);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("Extracting frames:  [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
                .unwrap()
                .progress_chars("=> "),
        );

        let mut child = Command::new("ffmpeg")
            .args(&[
                "-i",
                self.video_path.to_str().unwrap(),
                "-vf",
                &format!("fps={}", self.frame_rate),
                "-loglevel",
                "quiet",
                pattern.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::FFmpeg(format!("Failed to start ffmpeg for frames: {}", e)))?;

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

        let mut paths: Vec<_> = fs::read_dir(self.frames_dir.path())
            .map_err(|e| AppError::Io {
                source: e,
                context: Some(self.frames_dir.path().display().to_string()),
            })?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map_or(false, |ext| ext == "png"))
            .collect();
        paths.sort();
        Ok(paths)
    }
}

fn parse_fps(s: &str) -> f32 {
    if let Some((num, den)) = s.split_once('/') {
        num.parse::<f32>().unwrap_or(30.0) / den.parse::<f32>().unwrap_or(1.0)
    } else {
        s.parse::<f32>().unwrap_or(30.0)
    }
}
