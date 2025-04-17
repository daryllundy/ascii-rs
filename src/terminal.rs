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

    /// Enters alternate screen, hides cursor, enables raw mode.
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

    /// Clears the screen and moves cursor to top-left.
    pub fn clear(&mut self) -> Result<(), AppError> {
        execute!(self.stdout, Clear(ClearType::All), MoveTo(0, 0)).map_err(map_terminal_error)
    }

    /// Gets terminal size (columns, rows).
    pub fn get_size() -> Result<(u16, u16), AppError> {
        size()
            .map_err(map_terminal_error)
            .map_err(|_| AppError::TerminalSize)
    }

    /// Checks for user input (Esc or Ctrl+C). Returns true if interruption is requested.
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

    /// Writes output buffer to the terminal.
    pub fn draw(&mut self, content: &str) -> Result<(), AppError> {
        execute!(self.stdout, MoveTo(0, 0))
            .and_then(|_| self.stdout.write_all(content.as_bytes()))
            .and_then(|_| self.stdout.flush())
            .map_err(map_terminal_error)
    }

    /// Waits for any key press.
    #[allow(dead_code)]
    pub fn wait_for_key(&mut self) -> Result<(), AppError> {
        loop {
            match read().map_err(map_terminal_error)? {
                Event::Key(_) => break,
                Event::Resize(_, _) => { /* Optionally handle resize */ }
                _ => {}
            }
        }
        Ok(())
    }
}

impl Drop for TerminalManager {
    /// Restores terminal state when TerminalManager goes out of scope.
    fn drop(&mut self) {
        let _ = crossterm::execute!(self.stdout, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
        // Use eprintln to avoid interfering with potential final stdout output
        eprintln!();
    }
}
