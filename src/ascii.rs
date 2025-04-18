use crate::config::{ASCII_CHARS, CHAR_ASPECT_RATIO};
use crate::error::AppError;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageBuffer, Luma, Rgb};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RleRun {
    pub char: char,
    pub color: [u8; 3],
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RleFrame {
    pub width: u32,
    pub runs: Vec<RleRun>,
}

fn resize_and_center(
    img: &DynamicImage,
    terminal_cols: u16,
    terminal_lines: u16,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let term_width = terminal_cols as u32;
    let term_height = (terminal_lines.saturating_sub(1)) as u32;
    if term_width == 0 || term_height == 0 {
        log::warn!(
            "Terminal size is zero ({}, {}), returning empty canvas.",
            terminal_cols,
            terminal_lines
        );
        return ImageBuffer::from_pixel(1, 1, Rgb([0, 0, 0]));
    }

    let (orig_width, orig_height) = img.dimensions();
    if orig_width == 0 || orig_height == 0 {
        log::warn!(
            "Input image has zero dimension ({}, {}), returning empty canvas.",
            orig_width,
            orig_height
        );
        return ImageBuffer::from_pixel(term_width, term_height, Rgb([0, 0, 0]));
    }

    let mut new_width = term_width;
    let mut new_height = (orig_height as f32 / orig_width as f32 * new_width as f32
        / CHAR_ASPECT_RATIO)
        .round() as u32;

    if new_height > term_height {
        let scale = term_height as f32 / new_height as f32;
        new_height = term_height;
        new_width = (new_width as f32 * scale).round() as u32;
    }

    new_width = new_width.max(1);
    new_height = new_height.max(1);

    let filter = if new_width < orig_width || new_height < orig_height {
        FilterType::Triangle
    } else {
        FilterType::CatmullRom
    };

    let resized_img = img.resize_exact(new_width, new_height, filter);
    let mut canvas = ImageBuffer::from_pixel(term_width, term_height, Rgb([0, 0, 0]));
    let start_x = (term_width.saturating_sub(new_width)) / 2;
    let start_y = (term_height.saturating_sub(new_height)) / 2;
    image::imageops::overlay(
        &mut canvas,
        &resized_img.to_rgb8(),
        start_x as i64,
        start_y as i64,
    );
    canvas
}

fn convert_image_to_ascii(img: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> RleFrame {
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return RleFrame {
            width: 0,
            runs: Vec::new(),
        };
    }

    let gray_img: ImageBuffer<Luma<u8>, Vec<u8>> = image::imageops::grayscale(img);
    let mut runs: Vec<RleRun> = Vec::new();
    let mut current_run: Option<RleRun> = None;

    let num_ascii_chars = ASCII_CHARS.len() as f32;
    let max_ascii_index = ASCII_CHARS.len() - 1;

    for y in 0..height {
        for x in 0..width {
            let color_pixel = img.get_pixel(x, y);
            let gray_pixel = gray_img.get_pixel(x, y);

            let intensity = gray_pixel[0] as f32;
            let ascii_index = if num_ascii_chars > 1.0 {
                (intensity * (num_ascii_chars - 1.0) / 255.0).round() as usize
            } else {
                0
            };

            let ascii_char = ASCII_CHARS[ascii_index.min(max_ascii_index)];
            let rgb_array = color_pixel.0;

            match current_run.as_mut() {
                Some(run) if run.char == ascii_char && run.color == rgb_array => {
                    run.count += 1;
                }
                Some(_) => {
                    runs.push(current_run.take().unwrap());
                    current_run = Some(RleRun {
                        char: ascii_char,
                        color: rgb_array,
                        count: 1,
                    });
                }
                None => {
                    current_run = Some(RleRun {
                        char: ascii_char,
                        color: rgb_array,
                        count: 1,
                    });
                }
            }
        }
    }

    if let Some(run) = current_run {
        runs.push(run);
    }

    RleFrame { width, runs }
}

fn process_single_frame(
    image_path: &Path,
    terminal_size: (u16, u16),
) -> Result<RleFrame, AppError> {
    log::trace!("Processing frame: {}", image_path.display());
    let img = image::open(image_path)?;
    let resized_centered_img = resize_and_center(&img, terminal_size.0, terminal_size.1);
    let rle_frame = convert_image_to_ascii(&resized_centered_img);
    Ok(rle_frame)
}

pub fn process_frames_parallel(
    frame_paths: &[PathBuf],
    terminal_size: (u16, u16),
) -> Result<Vec<RleFrame>, AppError> {
    log::info!("Processing {} frames...", frame_paths.len());
    if frame_paths.is_empty() {
        log::warn!("No frame paths provided for parallel processing.");
        return Ok(Vec::new());
    }
    let start_time = std::time::Instant::now();

    let pb_len = frame_paths.len() as u64;
    let pb = ProgressBar::new(pb_len);

    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .map_err(|e| {
                AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Progress bar template error: {}", e),
                ))
            })?
            .progress_chars("#>-"),
    );

    let progress_bar = Mutex::new(pb);

    let results: Vec<Result<RleFrame, AppError>> = frame_paths
        .par_iter()
        .map(|path| {
            let result = process_single_frame(path, terminal_size);
            if let Ok(pb_guard) = progress_bar.lock() {
                pb_guard.inc(1);
            } else {
                log::error!(
                    "Failed to acquire progress bar lock for path: {}",
                    path.display()
                );
            }
            result
        })
        .collect();

    if let Ok(pb_guard) = progress_bar.lock() {
        pb_guard.finish_and_clear();
    } else {
        log::error!("Could not acquire progress bar lock to finish.");
    }

    let mut rle_frames = Vec::with_capacity(results.len());
    let mut first_error: Option<AppError> = None;

    for result in results {
        match result {
            Ok(frame) => rle_frames.push(frame),
            Err(e) => {
                log::error!("Error processing frame during collection: {}", e);
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
    }

    if let Some(err) = first_error {
        log::error!("Frame processing failed due to an error.");
        return Err(err);
    }

    log::info!(
        "Frames processed in {:.2}s",
        start_time.elapsed().as_secs_f64()
    );
    Ok(rle_frames)
}
