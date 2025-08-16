use std::path::Path;

// Get file stem from a path as a string
pub fn get_file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("test")
        .to_string()
}
