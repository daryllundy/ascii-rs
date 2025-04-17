use crate::error::{AppError, map_terminal_error};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{Event, KeyCode, KeyEvent, KeyModifiers, poll, read},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode, size,
    },
};
use std::io::{Stdout, Write, stdout};
use std::time::Duration;

pub struct TerminalManager {
    stdout: Stdout,
}

impl TerminalManager {
    pub fn new() -> Self {
        TerminalManager { stdout: stdout() }
    }

    pub fn setup(&mut self) -> Result<(), AppError> {
        enable_raw_mode().map_err(map_terminal_error)?;
        execute!(
            self.stdout,
            EnterAlternateScreen,
            Hide,
            Clear(ClearType::All),
            MoveTo(0, 0)
        )
        .map_err(map_terminal_error)?;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<(), AppError> {
        execute!(self.stdout, Clear(ClearType::All), MoveTo(0, 0)).map_err(map_terminal_error)
    }

    pub fn get_size() -> Result<(u16, u16), AppError> {
        size()
            .map_err(map_terminal_error)
            .map_err(|_| AppError::TerminalSize)
    }

    pub fn check_for_exit() -> Result<bool, AppError> {
        if poll(Duration::from_millis(1)).map_err(map_terminal_error)? {
            match read().map_err(map_terminal_error)? {
                Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                }) => return Ok(true),
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }) => return Ok(true),
                _ => {}
            }
        }
        Ok(false)
    }

    pub fn draw(&mut self, content: &str) -> Result<(), AppError> {
        execute!(self.stdout, MoveTo(0, 0))
            .and_then(|_| self.stdout.write_all(content.as_bytes()))
            .and_then(|_| self.stdout.flush())
            .map_err(map_terminal_error)
    }

    #[allow(dead_code)]
    pub fn wait_for_key(&mut self) -> Result<(), AppError> {
        loop {
            match read().map_err(map_terminal_error)? {
                Event::Key(_) => break,
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        Ok(())
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        let _ = crossterm::execute!(self.stdout, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        eprintln!();
    }
}
