use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "Plays videos in the terminal using ASCII characters", long_about = None)]
pub struct CliArgs {
    /// Path to the video file to play. If omitted, prompts the user.
    #[arg(short, long)]
    pub video: Option<PathBuf>,

    /// Force regeneration of ASCII frames even if a cache file exists.
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    pub regenerate: bool,
}

pub fn parse_args() -> CliArgs {
    CliArgs::parse()
}
