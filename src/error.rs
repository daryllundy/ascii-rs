use image::ImageError; // Import for clarity
use rodio::{PlayError, decoder::DecoderError};
use std::num::{ParseFloatError, ParseIntError}; // Import ParseFloatError
use std::path::PathBuf;
use std::string::FromUtf8Error; // Import for clarity
use thiserror::Error; // Import for clarity

#[derive(Error, Debug)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image processing error: {0}")]
    Image(#[from] ImageError),

    #[error("Audio playback error: {0}")]
    AudioPlayback(#[from] PlayError),

    #[error("Audio decoding error: {0}")]
    AudioDecode(#[from] DecoderError),

    #[error("Terminal error: {0}")]
    Terminal(std::io::Error),

    #[error("FFmpeg command failed: {0}")]
    FFmpeg(String),

    #[error("FFprobe command failed: {0}")]
    FFprobe(String),

    #[error("Video file not found: {0}")]
    VideoNotFound(PathBuf),

    #[error("Could not determine video properties (resolution, fps) for: {0}")]
    VideoMetadata(PathBuf),

    #[error("Invalid ACSV file: {0}")]
    InvalidAcsv(String),

    #[error("ACSV integrity check failed")]
    AcsvIntegrity,

    #[error("Unsupported ACSV version: {0}")]
    UnsupportedAcsvVersion(u8),

    #[error("Failed to parse integer: {0}")]
    ParseInt(#[from] ParseIntError),

    #[error("Failed to parse float: {0}")] // <-- Added Variant
    ParseFloat(#[from] ParseFloatError), // <-- Added From impl

    #[error("Failed to decode UTF-8: {0}")]
    Utf8(#[from] FromUtf8Error),

    #[error("Compression error: {0}")]
    Compression(std::io::Error),

    #[error("Decompression error: {0}")]
    Decompression(std::io::Error),

    #[error("Frame processing failed")]
    FrameProcessing,

    #[error("Could not get terminal size")]
    TerminalSize,

    #[error("User interruption")]
    Interrupted,

    #[error("Could not create output directory: {0}")]
    CreateDir(PathBuf, std::io::Error),

    #[error("Could not clean up frame directory: {0}")]
    CleanupFrames(PathBuf, std::io::Error),

    #[error("Could not get system information: {0}")]
    SystemInfo(String),
}

// Helper for converting crossterm::Result to our AppError
pub fn map_terminal_error(e: std::io::Error) -> AppError {
    AppError::Terminal(e)
}
