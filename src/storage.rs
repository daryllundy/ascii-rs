use crate::config::{ACSV_MAGIC, ACSV_VERSION, ZSTD_COMPRESSION_LEVEL};
use crate::error::AppError;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

pub fn save_ascii_frames(file_path: &Path, ascii_frames: &[String]) -> Result<(), AppError> {
    let start_time = std::time::Instant::now();

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::CreateDir(parent.to_path_buf(), e))?;
    }

    let file = File::create(file_path)?;
    let mut encoder =
        zstd::Encoder::new(file, ZSTD_COMPRESSION_LEVEL).map_err(AppError::Compression)?;

    let mut data_to_hash: Vec<u8> = Vec::new();

    data_to_hash.write_all(ACSV_MAGIC)?;
    data_to_hash.write_all(&[ACSV_VERSION])?;
    data_to_hash.write_all(&(ascii_frames.len() as u32).to_le_bytes())?;

    let mut frames_data: Vec<u8> = Vec::new();
    let pb_frames = ProgressBar::new(ascii_frames.len() as u64);
    pb_frames.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} Encoding frame data: {pos}/{len}")
            .unwrap(),
    );

    pb_frames.enable_steady_tick(Duration::from_millis(100));

    for frame in ascii_frames {
        let encoded_frame = frame.as_bytes();
        let frame_len = encoded_frame.len() as u32;
        frames_data.write_all(&frame_len.to_le_bytes())?;
        frames_data.write_all(encoded_frame)?;
        pb_frames.inc(1);
    }
    pb_frames.finish_and_clear();
    data_to_hash.append(&mut frames_data);

    let checksum = Sha256::digest(&data_to_hash);

    let pb_write = ProgressBar::new_spinner();
    pb_write.set_style(
        ProgressStyle::default_spinner()
            .template(&format!(
                "{{spinner}} Compressing and writing frames data to {}...",
                file_path.to_str().unwrap()
            ))
            .unwrap(),
    );
    pb_write.enable_steady_tick(Duration::from_millis(100));

    encoder.write_all(&data_to_hash)?;
    encoder.write_all(checksum.as_slice())?;
    encoder.finish().map_err(AppError::Compression)?;
    pb_write.finish_and_clear();

    log::info!(
        "Saved ASCII frames to {} successfully in {:.2}s",
        file_path.display(),
        start_time.elapsed().as_secs_f64()
    );
    Ok(())
}

pub fn load_ascii_frames(file_path: &Path) -> Result<Vec<String>, AppError> {
    log::info!("Loading ASCII frames from {}...", file_path.display());
    let start_time = std::time::Instant::now();

    let file = File::open(file_path)?;
    let mut decoder = zstd::Decoder::new(file).map_err(AppError::Decompression)?;
    let mut full_data = Vec::new();
    decoder.read_to_end(&mut full_data)?;

    let checksum_len = 32;
    if full_data.len() < (4 + 1 + 4 + checksum_len) {
        return Err(AppError::InvalidAcsv(format!(
            "File too small ({} bytes) to contain header and checksum",
            full_data.len()
        )));
    }

    let data_end_index = full_data.len() - checksum_len;
    let data_without_hash = &full_data[..data_end_index];
    let stored_checksum = &full_data[data_end_index..];
    let computed_checksum = Sha256::digest(data_without_hash);

    if stored_checksum != computed_checksum.as_slice() {
        log::error!(
            "ACSV Checksum mismatch! Stored: {:x?}, Computed: {:x?}",
            stored_checksum,
            computed_checksum.as_slice()
        );
        return Err(AppError::AcsvIntegrity);
    }

    let mut offset = 0;
    if &data_without_hash[offset..offset + 4] != ACSV_MAGIC {
        return Err(AppError::InvalidAcsv("Incorrect magic header".to_string()));
    }
    offset += 4;
    let version = data_without_hash[offset];
    offset += 1;
    if version != ACSV_VERSION {
        return Err(AppError::UnsupportedAcsvVersion(version));
    }
    let frame_count_bytes: [u8; 4] = data_without_hash[offset..offset + 4]
        .try_into()
        .map_err(|_| AppError::InvalidAcsv("Could not read frame count bytes".to_string()))?;
    let frame_count = u32::from_le_bytes(frame_count_bytes);
    offset += 4;

    let mut ascii_frames = Vec::with_capacity(frame_count as usize);
    let pb = ProgressBar::new(frame_count as u64);
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} Decoding frame data: {pos}/{len}")
            .unwrap(),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    for i in 0..frame_count {
        if offset + 4 > data_without_hash.len() {
            return Err(AppError::InvalidAcsv(format!(
                "Unexpected end of file while reading frame length for frame {}",
                i + 1
            )));
        }
        let frame_len_bytes: [u8; 4] =
            data_without_hash[offset..offset + 4]
                .try_into()
                .map_err(|_| {
                    AppError::InvalidAcsv(format!(
                        "Could not read length bytes for frame {}",
                        i + 1
                    ))
                })?;
        let frame_len = u32::from_le_bytes(frame_len_bytes) as usize;
        offset += 4;

        if offset + frame_len > data_without_hash.len() {
            return Err(AppError::InvalidAcsv(format!(
                "Unexpected end of file ({} bytes short) while reading frame data for frame {}",
                (offset + frame_len).saturating_sub(data_without_hash.len()),
                i + 1
            )));
        }
        let frame_data = &data_without_hash[offset..offset + frame_len];
        offset += frame_len;

        let frame_string = String::from_utf8(frame_data.to_vec())?;
        ascii_frames.push(frame_string);
        pb.inc(1);
    }
    pb.finish_and_clear();

    if offset != data_without_hash.len() {
        log::warn!(
            "Data length mismatch after reading frames. Expected offset {}, actual {}. File might have trailing data.",
            data_without_hash.len(),
            offset
        );
    }

    log::info!(
        "Loaded {} frames from {} successfully in {:.2}s",
        ascii_frames.len(),
        file_path.display(),
        start_time.elapsed().as_secs_f64()
    );
    Ok(ascii_frames)
}

pub fn cleanup_frame_directory(frames_dir: &Path) -> Result<(), AppError> {
    if frames_dir.exists() && frames_dir.is_dir() {
        log::info!(
            "Cleaning up temporary frame directory: {}",
            frames_dir.display()
        );
        fs::remove_dir_all(frames_dir)
            .map_err(|e| AppError::CleanupFrames(frames_dir.to_path_buf(), e))?;
        log::info!("Successfully cleaned up {}", frames_dir.display());
    } else {
        log::debug!(
            "Temporary frame directory not found or not a directory, skipping cleanup: {}",
            frames_dir.display()
        );
    }
    Ok(())
}
