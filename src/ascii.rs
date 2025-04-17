use crate::config::{ASCII_CHARS, CHAR_ASPECT_RATIO};
use crate::error::AppError;
// Removed unused Pixel import
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageBuffer, Luma, Rgb};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ... (rest of the file remains the same as the previous corrected version) ...

/// Resizes the image to fit the terminal dimensions while maintaining aspect ratio,
/// then centers it on a black canvas.
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

/// Converts a single frame (as an image buffer) to its ASCII representation with ANSI colors.
fn convert_image_to_ascii(img: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> String {
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return String::new();
    }

    let gray_img: ImageBuffer<Luma<u8>, Vec<u8>> = image::imageops::grayscale(img);
    let mut frame_buffer = String::with_capacity((width * height * 15 + height) as usize);
    let num_ascii_chars = ASCII_CHARS.len() as f32;
    let max_ascii_index = ASCII_CHARS.len() - 1;
    let mut current_color: Option<[u8; 3]> = None;

    for y in 0..height {
        for x in 0..width {
            let color_pixel = img.get_pixel(x, y);
            let gray_pixel = gray_img.get_pixel(x, y);
            let intensity = gray_pixel[0] as f32;
            let ascii_index = (intensity * (num_ascii_chars - 1.0) / 255.0).round() as usize;
            let ascii_char = ASCII_CHARS[ascii_index.min(max_ascii_index)];
            let rgb_array = color_pixel.0;

            if current_color != Some(rgb_array) {
                frame_buffer.push_str(&format!(
                    "\x1b[38;2;{};{};{}m",
                    rgb_array[0], rgb_array[1], rgb_array[2]
                ));
                current_color = Some(rgb_array);
            }
            frame_buffer.push(ascii_char);
        }
        frame_buffer.push_str("\x1b[0m");
        if y < height - 1 {
            frame_buffer.push('\n');
        }
        current_color = None;
    }
    frame_buffer
}

/// Processes a single image file: reads, resizes, converts to ASCII.
fn process_single_frame(image_path: &Path, terminal_size: (u16, u16)) -> Result<String, AppError> {
    log::trace!("Processing frame: {}", image_path.display());
    let img = image::open(image_path)?;
    let resized_centered_img = resize_and_center(&img, terminal_size.0, terminal_size.1);
    let ascii_frame = convert_image_to_ascii(&resized_centered_img);
    Ok(ascii_frame)
}

/// Processes all frame images in parallel using Rayon.
pub fn process_frames_parallel(
    frame_paths: &[PathBuf],
    terminal_size: (u16, u16),
    // *** REMOVED total_frames parameter ***
) -> Result<Vec<String>, AppError> {
    // <--- Parameter removed here
    log::info!("Processing {} frames in parallel...", frame_paths.len());
    if frame_paths.is_empty() {
        log::warn!("No frame paths provided for parallel processing.");
        return Ok(Vec::new());
    }
    let start_time = std::time::Instant::now();

    // Use the actual number of paths for the progress bar length
    let pb_len = frame_paths.len() as u64; // <--- Uses len() here
    let pb = ProgressBar::new(pb_len);

    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} Processing Frames [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
        .map_err(|e| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Progress bar template error: {}", e))))?
        .progress_chars("#>-"));

    let progress_bar = Mutex::new(pb);

    let results: Vec<Result<String, AppError>> = frame_paths
        .par_iter()
        .map(|path| {
            let result = process_single_frame(path, terminal_size);
            if let Ok(pb_guard) = progress_bar.lock() {
                pb_guard.inc(1);
            }
            result
        })
        .collect();

    if let Ok(pb_guard) = progress_bar.lock() {
        pb_guard.finish_and_clear();
    } else {
        log::error!("Could not acquire progress bar lock to finish.");
    }

    let mut ascii_frames = Vec::with_capacity(results.len());
    let mut first_error: Option<AppError> = None;
    for result in results {
        match result {
            Ok(frame) => ascii_frames.push(frame),
            Err(e) => {
                log::error!("Error processing frame: {}", e);
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
    }

    if let Some(err) = first_error {
        return Err(err);
    }

    log::info!(
        "Frames processed in {:.2}s",
        start_time.elapsed().as_secs_f64()
    );
    Ok(ascii_frames)
}
