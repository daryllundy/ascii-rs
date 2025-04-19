use crate::ascii::RleFrame;
use crate::config::{ACSV_MAGIC, ACSV_VERSION, ZSTD_COMPRESSION_LEVEL};
use crate::error::AppError;
use indicatif::{ProgressBar, ProgressStyle};
use serde_cbor;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

pub fn save_ascii_frames(file_path: &Path, rle_frames: &[RleFrame]) -> Result<(), AppError> {
    let start_time = std::time::Instant::now();
    log::info!(
        "Saving {} frames to cache: {}",
        rle_frames.len(),
        file_path.display()
    );

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::CreateDir(parent.to_path_buf(), e))?;
    }

    let pb_encode = ProgressBar::new_spinner();
    pb_encode.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} Serializing frame data...")
            .unwrap(),
    );
    pb_encode.enable_steady_tick(Duration::from_millis(100));

    let serialized_frames_data = serde_cbor::to_vec(&rle_frames)
        .map_err(|e| AppError::CacheWrite(format!("Frames serialization failed: {}", e)))?;

    pb_encode.finish_and_clear();
    log::debug!(
        "Serialized frames size: {} bytes",
        serialized_frames_data.len()
    );

    let mut data_to_hash: Vec<u8> = Vec::new();
    data_to_hash.write_all(ACSV_MAGIC)?;
    data_to_hash.write_all(&[ACSV_VERSION])?;
    data_to_hash.write_all(&(rle_frames.len() as u32).to_le_bytes())?;
    data_to_hash.write_all(&serialized_frames_data)?;

    let checksum = Sha256::digest(&data_to_hash);
    log::debug!("Computed checksum: {:x?}", checksum.as_slice());

    let file = File::create(file_path)?;
    let mut encoder =
        zstd::Encoder::new(file, ZSTD_COMPRESSION_LEVEL).map_err(AppError::Compression)?;

    let pb_write = ProgressBar::new_spinner();
    pb_write.set_style(
        ProgressStyle::default_spinner()
            .template(&format!(
                "{{spinner}} Compressing and writing frames data to {}...",
                file_path.to_str().unwrap_or("cache file")
            ))
            .unwrap(),
    );
    pb_write.enable_steady_tick(Duration::from_millis(100));

    encoder.write_all(&data_to_hash)?;
    encoder.write_all(checksum.as_slice())?;
    encoder.finish().map_err(AppError::Compression)?;
    pb_write.finish_and_clear();

    log::info!(
        "Saved frames data to {} successfully (took {:.2}s)",
        file_path.display(),
        start_time.elapsed().as_secs_f64()
    );
    Ok(())
}

pub fn load_ascii_frames(file_path: &Path) -> Result<Vec<RleFrame>, AppError> {
    log::info!("Loading frames from {}...", file_path.display());
    let start_time = std::time::Instant::now();

    let file = File::open(file_path)?;
    let mut decoder = zstd::Decoder::new(file).map_err(AppError::Decompression)?;
    let mut full_data = Vec::new();

    let pb_read = ProgressBar::new_spinner();
    pb_read.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} Decompressing and reading cache file...")
            .unwrap(),
    );
    pb_read.enable_steady_tick(Duration::from_millis(100));
    decoder.read_to_end(&mut full_data)?;
    pb_read.finish_and_clear();

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
            "Checksum mismatch! Stored: {:x?}, Computed: {:x?}",
            stored_checksum,
            computed_checksum.as_slice()
        );
        return Err(AppError::AcsvIntegrity);
    }
    log::debug!("Checksum verified successfully.");

    let mut offset = 0;
    if &data_without_hash[offset..offset + 4] != ACSV_MAGIC {
        return Err(AppError::InvalidAcsv("Incorrect magic header".to_string()));
    }
    offset += 4;
    let version = data_without_hash[offset];
    offset += 1;
    if version != ACSV_VERSION {
        // log::warn!(
        //     "Loaded cache version {} differs from current version {}",
        //     version,
        //     ACSV_VERSION
        // );
        return Err(AppError::UnsupportedAcsvVersion(version));
    } else {
        log::debug!("Cache version {} matches current version.", version);
    }
    let frame_count_bytes: [u8; 4] = data_without_hash[offset..offset + 4]
        .try_into()
        .map_err(|_| AppError::InvalidAcsv("Could not read frame count bytes".to_string()))?;
    let frame_count = u32::from_le_bytes(frame_count_bytes);
    offset += 4;

    let serialized_frames_data = &data_without_hash[offset..];
    log::debug!(
        "Attempting to deserialize {} bytes of data for {} frames.",
        serialized_frames_data.len(),
        frame_count
    );

    let pb_decode = ProgressBar::new(frame_count as u64);
    pb_decode.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} Deserializing frame data: {pos}/{len}")
            .unwrap(),
    );
    pb_decode.enable_steady_tick(Duration::from_millis(100));

    let rle_frames: Vec<RleFrame> = serde_cbor::from_slice(serialized_frames_data)
        .map_err(|e| AppError::CacheRead(format!("Frames deserialization failed: {}", e)))?;

    pb_decode.set_position(frame_count as u64);
    pb_decode.finish_and_clear();

    if rle_frames.len() != frame_count as usize {
        log::warn!(
            "Header expected {} frames, but deserialized {} frames.",
            frame_count,
            rle_frames.len()
        );
    }

    log::info!(
        "Loaded {} frames from {} successfully (took {:.2}s)",
        rle_frames.len(),
        file_path.display(),
        start_time.elapsed().as_secs_f64()
    );
    Ok(rle_frames)
}

pub fn cleanup_frame_directory(frames_dir: &Path) -> Result<(), AppError> {
    if frames_dir.exists() && frames_dir.is_dir() {
        log::debug!(
            "Cleaning up temporary frame directory: {}",
            frames_dir.display()
        );
        fs::remove_dir_all(frames_dir)
            .map_err(|e| AppError::CleanupFrames(frames_dir.to_path_buf(), e))?;
        log::debug!("Successfully cleaned up {}", frames_dir.display());
    } else {
        log::debug!(
            "Temporary frame directory not found or not a directory, skipping cleanup: {}",
            frames_dir.display()
        );
    }
    Ok(())
}
