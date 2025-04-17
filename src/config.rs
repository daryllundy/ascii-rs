use lazy_static::lazy_static;

pub const AUTHOR: &str = "minhcrafters";

const ASCII_STR: &str = " .:,;'_\"^<>-!~=)(|j?}{}][ti+l7v1%yrfcJ32uIC$zwo96sgnaT5qpkyVOL40&mG8*xhedbZUSAPQFDXWK#RNEHBM@";

lazy_static! {
    pub static ref ASCII_CHARS: Vec<char> = ASCII_STR.chars().collect();
}

pub const CHAR_ASPECT_RATIO: f32 = 2.0;

pub const ACSV_VERSION: u8 = 1;
pub const ACSV_MAGIC: &[u8; 4] = b"ACSV";

pub const ZSTD_COMPRESSION_LEVEL: i32 = 12;

pub const METRICS_UPDATE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
