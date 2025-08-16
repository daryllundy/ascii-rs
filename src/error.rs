use image::ImageError;
use rodio::{PlayError, decoder::DecoderError};
use std::num::{ParseFloatError, ParseIntError};
use std::path::PathBuf;
use std::string::FromUtf8Error;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("I/O error: {source}")]
    Io {
        source: std::io::Error,
        context: Option<String>,
    },

    #[error("Image processing error: {source}")]
    Image {
        source: ImageError,
        context: Option<String>,
    },

    #[error("Audio playback error: {source}")]
    AudioPlayback {
        source: PlayError,
        context: Option<String>,
    },

    #[error("Audio decoding error: {source}")]
    AudioDecode {
        source: DecoderError,
        context: Option<String>,
    },

    #[error("Terminal error: {source}")]
    Terminal {
        source: std::io::Error,
        context: Option<String>,
    },

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

    #[error("Error during cache write operation: {0}")]
    CacheWrite(String),

    #[error("Error during cache read operation: {0}")]
    CacheRead(String),

    #[error("ACSV integrity check failed")]
    AcsvIntegrity,

    #[error("Unsupported ACSV version: {0}")]
    UnsupportedAcsvVersion(u8),

    #[error("Failed to parse integer: {source}")]
    ParseInt {
        source: ParseIntError,
        context: Option<String>,
    },

    #[error("Failed to parse float: {source}")]
    #[allow(dead_code)]
    ParseFloat {
        source: ParseFloatError,
        context: Option<String>,
    },

    #[error("Failed to decode UTF-8: {source}")]
    Utf8 {
        source: FromUtf8Error,
        context: Option<String>,
    },

    #[error("Compression error: {source}")]
    Compression {
        source: std::io::Error,
        context: Option<String>,
    },

    #[error("Decompression error: {source}")]
    Decompression {
        source: std::io::Error,
        context: Option<String>,
    },

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
