use crate::error::AppError;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{Event, KeyCode, KeyEvent, KeyModifiers, poll, read},
    execute,
    style::Print,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, SetSize, disable_raw_mode,
        enable_raw_mode, size,
    },
};
use log::{debug, error, info};
use std::{
    io::{Stdout, Write, stdout},
    time::Duration,
};

pub struct TerminalManager {
    stdout: Stdout,
    original_size: Option<(u16, u16)>,
    previous_frame: String,
}

impl TerminalManager {
    pub fn new() -> Self {
        TerminalManager {
            stdout: stdout(),
            original_size: None,
            previous_frame: String::new(),
        }
    }

    pub fn setup(&mut self) -> Result<(), AppError> {
        info!("Setting up terminal");

        self.original_size = Some(Self::get_size().unwrap());

        enable_raw_mode().map_err(|e| {
            error!("Failed to enable raw mode: {}", e);
            AppError::Terminal {
                source: e,
                context: Some("enable_raw_mode".to_string()),
            }
        })?;

        execute!(
            self.stdout,
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All),
            MoveTo(0, 0)
        )
        .map_err(|e| {
            error!("Failed to initialize terminal: {}", e);
            AppError::Terminal {
                source: e,
                context: Some("terminal initialization".to_string()),
            }
        })?;

        debug!("Terminal setup complete");
        Ok(())
    }

    pub fn clear(&mut self) -> Result<(), AppError> {
        execute!(self.stdout, Clear(ClearType::All), MoveTo(0, 0)).map_err(|e| {
            error!("Failed to clear terminal: {}", e);
            AppError::Terminal {
                source: e,
                context: Some("clear screen".to_string()),
            }
        })
    }

    pub fn get_size() -> Result<(u16, u16), AppError> {
        size()
            .map_err(|e| {
                error!("Failed to get terminal size: {}", e);
                AppError::Terminal {
                    source: e,
                    context: Some("get terminal size".to_string()),
                }
            })
            .map_err(|_| {
                error!("Invalid terminal size");
                AppError::TerminalSize
            })
    }

    pub fn check_for_exit() -> Result<bool, AppError> {
        if poll(Duration::from_millis(1)).map_err(|e| {
            error!("Failed to poll terminal events: {}", e);
            AppError::Terminal {
                source: e,
                context: Some("poll terminal events".to_string()),
            }
        })? {
            match read().map_err(|e| {
                error!("Failed to read from terminal: {}", e);
                AppError::Terminal {
                    source: e,
                    context: Some("read terminal input".to_string()),
                }
            })? {
                Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                }) => {
                    debug!("Escape key pressed, exiting");
                    return Ok(true);
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }) => {
                    debug!("Ctrl+C pressed, exiting");
                    return Ok(true);
                }
                _ => {}
            }
        }
        Ok(false)
    }

    pub fn draw(&mut self, content: &str) -> Result<(), AppError> {
        let old_lines = self.previous_frame.lines().collect::<Vec<_>>();
        let new_lines = content.lines().collect::<Vec<_>>();

        for (y, new_line) in new_lines.iter().enumerate() {
            if y >= old_lines.len() || old_lines[y] != *new_line {
                execute!(self.stdout, MoveTo(0, y as u16), Print(new_line)).map_err(|e| {
                    AppError::Io {
                        source: e,
                        context: Some("Failed to write to terminal".to_string()),
                    }
                })?;
            }
        }

        if new_lines.len() < old_lines.len() {
            for y in new_lines.len()..old_lines.len() {
                execute!(
                    self.stdout,
                    MoveTo(0, y as u16),
                    Clear(ClearType::CurrentLine)
                )
                .map_err(|e| AppError::Io {
                    source: e,
                    context: Some("Failed to clear line".to_string()),
                })?;
            }
        }

        self.previous_frame = content.to_string();

        self.stdout.flush().map_err(|e| AppError::Io {
            source: e,
            context: Some("Failed to flush stdout".to_string()),
        })
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        debug!("Dropping TerminalManager, restoring terminal state");

        if let Some((cols, rows)) = self.original_size {
            if let Err(e) = execute!(self.stdout, SetSize(cols, rows)) {
                error!("Failed to restore terminal size: {}", e);
            }
        }

        if let Err(e) = disable_raw_mode() {
            error!("Failed to disable raw mode: {}", e);
        }
        if let Err(e) = execute!(&mut self.stdout, Show, LeaveAlternateScreen) {
            error!("Failed to restore terminal state: {}", e);
        }

        if let Err(e) = self.stdout.flush() {
            error!("Failed to flush stdout: {}", e);
        }

        debug!("Terminal cleanup complete");
    }
}
