# ascii-rs

A Rust command line tool that plays videos as _coloured_ ASCII art directly in your terminal with minimal performance overhead.

## Steps to run

-   If not already, install Rust [via rustup](https://rustup.rs) and FFmpeg (ffmpeg must be on your system PATH).
-   Put a video file somewhere on your machine (e.g., videos/sample.mp4).
-   From the project root, run in your terminal:
    -   Debug: `cargo run -- <path-to-video>`
    -   Release: `cargo run --release -- <path-to-video>`
-   Use Ctrl+C to stop playback at any time.

## Screenshots

![sync_test.png](img/sync_test.png)

![steve.png](img/steve.png)

And everyone's favourite:

![bad_apple.png](img/bad_apple.png)

## Notes

-   Tested on Windows Terminal (Powershell), not sure about other terminals.
-   Larger terminals look better; a minimum of ~100x80 is recommended.
-   A cache file is created to speed up subsequent runs of the same video.
-   Add --regenerate to force rebuilding the ASCII cache for that video.
