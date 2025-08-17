use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Plays videos in the terminal using ASCII characters"
)]
pub struct CliArgs {
    #[arg(required = true)]
    pub video: PathBuf,

    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub regenerate: bool,

    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub compatibility_mode: bool,
}

pub fn parse_args() -> CliArgs {
    CliArgs::parse()
}
