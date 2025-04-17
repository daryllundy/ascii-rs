mod ascii;
mod cli;
mod config;
mod error;
mod metrics;
mod playback;
mod storage;
mod terminal;
mod video;

use crate::error::AppError;
use crate::terminal::TerminalManager;
use crate::video::VideoInfo;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn run_app() -> Result<(), AppError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Starting ASCII Video Player v{}", env!("CARGO_PKG_VERSION"));
    log::info!("By: {}", config::AUTHOR);

    let mut terminal_manager = TerminalManager::new();

    let global_stop_signal = Arc::new(AtomicBool::new(false));
    let signal_clone = Arc::clone(&global_stop_signal);
    ctrlc::set_handler(move || {
        log::info!("Ctrl+C detected, setting stop signal.");
        signal_clone.store(true, Ordering::Relaxed);
    })
    .map_err(|e| {
        log::error!("Failed to set Ctrl-C handler: {}", e);
        AppError::Io(io::Error::new(
            io::ErrorKind::Other,
            format!("Ctrl-C handler setup failed: {}", e),
        ))
    })?;

    let args = cli::parse_args();
    let video_path = match args.video {
        Some(path) => path,
        None => {
            let path = {
                let mut stdout_handle = io::stdout();
                crossterm::execute!(stdout_handle, crossterm::cursor::Show)
                    .map_err(error::map_terminal_error)?;
                crossterm::terminal::disable_raw_mode().map_err(error::map_terminal_error)?;

                print!("Enter path to video file: ");
                stdout_handle.flush()?;

                let mut input_line = String::new();
                io::stdin().read_line(&mut input_line)?;

                PathBuf::from(input_line.trim().to_string())
            };

            crossterm::terminal::enable_raw_mode().map_err(error::map_terminal_error)?;
            crossterm::execute!(io::stdout(), crossterm::cursor::Hide)
                .map_err(error::map_terminal_error)?;
            terminal_manager.clear()?;

            path
        }
    };

    log::info!("Video file selected: {}", video_path.display());
    if global_stop_signal.load(Ordering::Relaxed) {
        return Err(AppError::Interrupted);
    }

    let terminal_size = TerminalManager::get_size()?;
    log::info!("Terminal size: {}x{}", terminal_size.0, terminal_size.1);
    if terminal_size.0 < 10 || terminal_size.1 < 5 {
        log::warn!("Terminal size is very small, playback might look strange.");
    }

    let video_info = VideoInfo::analyze(&video_path, terminal_size)?;
    if global_stop_signal.load(Ordering::Relaxed) {
        return Err(AppError::Interrupted);
    }

    video_info.extract_audio()?;

    if global_stop_signal.load(Ordering::Relaxed) {
        return Err(AppError::Interrupted);
    }

    let ascii_frames: Vec<String>;
    let mut cleanup_frames_dir = false;

    if video_info.ascii_cache_path.exists() && !args.regenerate {
        log::info!(
            "Found existing cache file: {}",
            video_info.ascii_cache_path.display()
        );
        match storage::load_ascii_frames(&video_info.ascii_cache_path) {
            Ok(frames) => {
                log::info!("Successfully loaded {} frames from cache.", frames.len());
                ascii_frames = frames;
            }
            Err(e) => {
                log::warn!(
                    "Failed to load cache file {}: {}. Regenerating frames.",
                    video_info.ascii_cache_path.display(),
                    e
                );
                let frame_paths = video_info.extract_frames()?;
                if global_stop_signal.load(Ordering::Relaxed) {
                    return Err(AppError::Interrupted);
                }
                cleanup_frames_dir = true;
                ascii_frames = ascii::process_frames_parallel(&frame_paths, terminal_size)?;
                if global_stop_signal.load(Ordering::Relaxed) {
                    return Err(AppError::Interrupted);
                }
                storage::save_ascii_frames(&video_info.ascii_cache_path, &ascii_frames)?;
            }
        }
    } else {
        if args.regenerate {
            log::info!("Regenerate flag set, processing frames...");
        } else {
            log::info!(
                "No valid cache file found at {}, processing frames...",
                video_info.ascii_cache_path.display()
            );
        }

        let frame_paths = video_info.extract_frames()?;
        if global_stop_signal.load(Ordering::Relaxed) {
            return Err(AppError::Interrupted);
        }
        cleanup_frames_dir = true;
        ascii_frames = ascii::process_frames_parallel(&frame_paths, terminal_size)?;
        if global_stop_signal.load(Ordering::Relaxed) {
            return Err(AppError::Interrupted);
        }
        storage::save_ascii_frames(&video_info.ascii_cache_path, &ascii_frames)?;
    }

    if cleanup_frames_dir {
        storage::cleanup_frame_directory(&video_info.frames_dir)?;
    }
    if global_stop_signal.load(Ordering::Relaxed) {
        return Err(AppError::Interrupted);
    }

    if ascii_frames.is_empty() {
        log::error!("No ASCII frames were generated or loaded. Cannot play.");
        return Err(AppError::FrameProcessing);
    }

    log::info!("Prepared {} ASCII frames for playback.", ascii_frames.len());

    let metrics_monitor = metrics::MetricsMonitor::new()?;

    let mut player = playback::Player::new(
        ascii_frames,
        video_info.audio_path.clone(),
        video_info.frame_rate,
        terminal_manager,
        metrics_monitor,
    )?;

    player.stop_signal = global_stop_signal;

    let play_result = player.play();

    play_result?;

    Ok(())
}

fn main() {
    match run_app() {
        Ok(_) => {
            log::info!("Playback finished successfully.");
            println!("Playback finished. Press Enter to exit...");
            let mut buffer = String::new();
            let _ = io::stdin().read_line(&mut buffer);
            exit(0);
        }
        Err(AppError::Interrupted) => {
            eprintln!("\nPlayback interrupted by user.");
            exit(130);
        }
        Err(e) => {
            eprintln!("\n\x1b[0m\x1b[31mError:\x1b[0m {}", e);
            println!("An error occurred. Press Enter to exit...");
            let mut buffer = String::new();
            let _ = io::stdin().read_line(&mut buffer);
            exit(1);
        }
    }
}
