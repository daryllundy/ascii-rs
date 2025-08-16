use log::LevelFilter;
use log4rs::{
    append::{
        console::{ConsoleAppender, Target},
        file::FileAppender,
    },
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
    filter::threshold::ThresholdFilter,
};
use std::io;

pub fn setup_logging(level: LevelFilter, file_path: &str) -> Result<(), io::Error> {
    let stderr = ConsoleAppender::builder()
        .target(Target::Stderr)
        .encoder(Box::new(PatternEncoder::new(
            "[{d(%Y-%m-%d %H:%M:%S)} {h({l})}] {m}\n",
        )))
        .build();

    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "[{d(%Y-%m-%d %H:%M:%S)} {l}] {m}\n",
        )))
        .append(false)
        .build(file_path)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let log_config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .appender(
            Appender::builder()
                .filter(Box::new(ThresholdFilter::new(level)))
                .build("stderr", Box::new(stderr)),
        )
        .build(
            Root::builder()
                .appender("logfile")
                .appender("stderr")
                .build(LevelFilter::Debug),
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    log4rs::init_config(log_config).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(())
}

#[macro_export]
macro_rules! log_app_startup {
    () => {
        log::info!("ascii-rs v{}", env!("CARGO_PKG_VERSION"));
        log::info!("by: {}", $crate::config::AUTHOR);
        log::info!("Made with sausage rolls and constant caffeine supply");
    };
}
