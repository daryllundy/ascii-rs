use crate::config::{ASCII_CHARS, CHAR_ASPECT_RATIO};
use crate::error::AppError;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RleRun {
    pub ascii_idx: u8,
    pub color: [u8; 3],
    pub count: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RleFrame {
    pub width: u16,
    pub runs: Vec<RleRun>,
}

fn resize_and_center(img: &DynamicImage, cols: u16, lines: u16) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let term_w = cols as u32;
    let term_h = lines.saturating_sub(1) as u32;
    if term_w == 0 || term_h == 0 {
        return ImageBuffer::from_pixel(1, 1, Rgb([0, 0, 0]));
    }
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return ImageBuffer::from_pixel(term_w, term_h, Rgb([0, 0, 0]));
    }
    let mut nw = term_w;
    let mut nh = ((h as f32 / w as f32 * nw as f32) / CHAR_ASPECT_RATIO).round() as u32;
    if nh > term_h {
        let s = term_h as f32 / nh as f32;
        nh = term_h;
        nw = (nw as f32 * s).round() as u32;
    }
    nw = nw.max(1);
    nh = nh.max(1);
    let filter = if nw < w || nh < h {
        FilterType::Triangle
    } else {
        FilterType::CatmullRom
    };
    let r = img.resize_exact(nw, nh, filter).to_rgb8();
    let mut canvas = ImageBuffer::from_pixel(term_w, term_h, Rgb([0, 0, 0]));
    let sx = (term_w - nw) / 2;
    let sy = (term_h - nh) / 2;
    image::imageops::overlay(&mut canvas, &r, sx as i64, sy as i64);
    canvas
}

fn convert_image_to_ascii(img: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> RleFrame {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return RleFrame {
            width: 0,
            runs: Vec::new(),
        };
    }
    let na = ASCII_CHARS.len() as f32;
    let mi = (ASCII_CHARS.len() - 1) as u8;
    let mut runs = Vec::new();
    for y in 0..h {
        let mut cur: Option<RleRun> = None;
        for x in 0..w {
            let px = img.get_pixel(x, y).0;
            let intensity =
                (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32) / 255.0;
            let idx = if na > 1.0 {
                (intensity * (na - 1.0)).round() as u8
            } else {
                0
            };
            let idx = idx.min(mi);
            match cur.as_mut() {
                Some(r) if r.ascii_idx == idx && r.color == px => r.count = r.count.wrapping_add(1),
                Some(_) => {
                    runs.push(cur.take().unwrap());
                    cur = Some(RleRun {
                        ascii_idx: idx,
                        color: px,
                        count: 1,
                    });
                }
                None => {
                    cur = Some(RleRun {
                        ascii_idx: idx,
                        color: px,
                        count: 1,
                    })
                }
            }
        }
        if let Some(r) = cur {
            runs.push(r);
        }
    }
    RleFrame {
        width: w as u16,
        runs,
    }
}

fn process_single_frame(path: &Path, size: (u16, u16)) -> Result<RleFrame, AppError> {
    let img = image::open(path)?;
    let buf = resize_and_center(&img, size.0, size.1);
    Ok(convert_image_to_ascii(&buf))
}

pub fn process_frames_parallel(
    paths: &[PathBuf],
    size: (u16, u16),
) -> Result<Vec<RleFrame>, AppError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let pb = ProgressBar::new(paths.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("Generating ASCII frames:  [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("=> "),
    );
    let m = Mutex::new(pb);
    let mut frames = Vec::with_capacity(paths.len());
    let results: Vec<_> = paths
        .par_iter()
        .map(|p| {
            let f = process_single_frame(p, size);
            if let Ok(ref pb) = m.lock() {
                pb.inc(1);
            }
            f
        })
        .collect();
    for res in results {
        frames.push(res?);
    }
    if let Ok(pb) = m.lock() {
        pb.finish_and_clear();
    }
    Ok(frames)
}
