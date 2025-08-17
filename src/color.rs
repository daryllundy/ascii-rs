pub fn rgb_to_ansi256(r: u8, g: u8, b: u8) -> u8 {
    // If the color is a shade of gray
    if r == g && g == b {
        if r < 8 {
            16 // black
        } else if r > 248 {
            231 // white
        } else {
            // gray index
            232 + ((r as f32 - 8.0) / 10.0).round() as u8
        }
    } else {
        // Not a gray, so map to the 6x6x6 color cube
        let r_idx = (r as f32 / 255.0 * 5.0).round() as u8;
        let g_idx = (g as f32 / 255.0 * 5.0).round() as u8;
        let b_idx = (b as f32 / 255.0 * 5.0).round() as u8;
        16 + 36 * r_idx + 6 * g_idx + b_idx
    }
}
