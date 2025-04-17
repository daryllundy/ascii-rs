use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "Plays videos in the terminal using ASCII characters", long_about = None)]
pub struct CliArgs {
    #[arg(short, long)]
    pub video: Option<PathBuf>,

    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    pub regenerate: bool,
}

pub fn parse_args() -> CliArgs {
    CliArgs::parse()
}
