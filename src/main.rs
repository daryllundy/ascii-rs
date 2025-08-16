mod ascii;
mod cli;
mod config;
mod error;
mod logging;
mod metrics;
mod playback;
mod storage;
mod terminal;
mod utils;
mod video;

use crate::{ascii::RleFrame, error::AppError, terminal::TerminalManager, video::VideoInfo};
use log::LevelFilter;
use std::{
    io,
    process::exit,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn run_app() -> Result<(), AppError> {
    // Setup logging
    logging::setup_logging(LevelFilter::Info, "latest.log").map_err(|e| AppError::Io {
        source: e,
        context: Some("Failed to initialize logging".to_string()),
    })?;

    log_app_startup!();

    let terminal_manager = TerminalManager::new();

    let global_stop_signal = Arc::new(AtomicBool::new(false));
    let signal_clone = Arc::clone(&global_stop_signal);

    ctrlc::set_handler(move || {
        log::debug!("Ctrl+C detected, setting stop signal.");
        signal_clone.store(true, Ordering::Relaxed);
    })
    .map_err(|e| {
        log::error!("Failed to set Ctrl-C handler: {}", e);
        AppError::Io {
            source: io::Error::new(
                io::ErrorKind::Other,
                format!("Ctrl-C handler setup failed: {}", e),
            ),
            context: None,
        }
    })?;

    let args = cli::parse_args();

    let video_path = args.video;

    log::info!("Video file selected: {}", video_path.display());
    if global_stop_signal.load(Ordering::Relaxed) {
        return Err(AppError::Interrupted);
    }

    let terminal_size = TerminalManager::get_size()?;
    log::info!("Terminal size: {}x{}", terminal_size.0, terminal_size.1);
    if terminal_size.0 < 100 || terminal_size.1 < 80 {
        log::warn!(
            "Terminal size ({},{}) is smaller than recommended ({},{}). Playback might be suboptimal.",
            terminal_size.0,
            terminal_size.1,
            100,
            80
        );
    }

    let video_info = VideoInfo::analyze(&video_path, terminal_size)?;
    if global_stop_signal.load(Ordering::Relaxed) {
        return Err(AppError::Interrupted);
    }

    video_info.extract_audio()?;

    if global_stop_signal.load(Ordering::Relaxed) {
        return Err(AppError::Interrupted);
    }

    let rle_frames: Vec<RleFrame>;
    let mut cleanup_frames_dir = false;

    if video_info.ascii_cache_path.exists() && !args.regenerate {
        log::info!(
            "Found existing cache file: {}",
            video_info.ascii_cache_path.display()
        );
        match storage::load_ascii_frames(&video_info.ascii_cache_path) {
            Ok(frames) => {
                log::info!("Successfully loaded {} frames from cache.", frames.len());
                rle_frames = frames;
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
                rle_frames = ascii::process_frames_parallel(&frame_paths, terminal_size)?;
                if global_stop_signal.load(Ordering::Relaxed) {
                    return Err(AppError::Interrupted);
                }
                storage::save_ascii_frames(&video_info.ascii_cache_path, &rle_frames)?;
            }
        }
    } else {
        if args.regenerate {
            log::info!("Regenerate flag set, regenerating frames...");
        } else {
            log::info!(
                "No valid cache file found at {}, regenerating frames...",
                video_info.ascii_cache_path.display()
            );
        }

        let frame_paths = video_info.extract_frames()?;
        if global_stop_signal.load(Ordering::Relaxed) {
            return Err(AppError::Interrupted);
        }
        cleanup_frames_dir = true;
        rle_frames = ascii::process_frames_parallel(&frame_paths, terminal_size)?;
        if global_stop_signal.load(Ordering::Relaxed) {
            return Err(AppError::Interrupted);
        }
        storage::save_ascii_frames(&video_info.ascii_cache_path, &rle_frames)?;
    }

    if cleanup_frames_dir {
        storage::cleanup_frame_directory(video_info.frames_dir.path())?;
    }
    if global_stop_signal.load(Ordering::Relaxed) {
        return Err(AppError::Interrupted);
    }

    if rle_frames.is_empty() {
        log::error!("No frames were generated or loaded. Cannot play.");
        return Err(AppError::FrameProcessing);
    }

    log::info!("Prepared {} frames for playback.", rle_frames.len());

    let metrics_monitor = metrics::MetricsMonitor::new()?;

    let mut player = playback::Player::new(
        rle_frames,
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
    let main_result = std::panic::catch_unwind(|| run_app());

    let mut stdout = io::stdout();
    let _ = crossterm::execute!(stdout, crossterm::cursor::Show);
    let _ = crossterm::terminal::disable_raw_mode();

    match main_result {
        Ok(Ok(_)) => {
            log::info!("Playback finished successfully");
            exit(0);
        }
        Ok(Err(AppError::Interrupted)) => {
            eprintln!("\nPlayback interrupted by user");
            log::warn!("Playback interrupted by user");
            exit(130);
        }
        Ok(Err(e)) => {
            eprintln!("\n\x1b[0m\x1b[31mError:\x1b[0m {}", e);
            log::error!("Application exited with error: {}", e);
            exit(1);
        }
        Err(panic_payload) => {
            eprintln!("\n\x1b[0m\x1b[91mCritical Error: Application Panicked!\x1b[0m");
            log::error!("Application panicked: {:?}", panic_payload);
            if let Some(s) = panic_payload.downcast_ref::<String>() {
                eprintln!("Panic message: {}", s);
                log::error!("Panic message: {}", s);
            } else if let Some(s) = panic_payload.downcast_ref::<&str>() {
                eprintln!("Panic message: {}", s);
                log::error!("Panic message: {}", s);
            } else {
                eprintln!("Panic occurred with unknown payload type");
            }
            exit(101);
        }
    }
}
