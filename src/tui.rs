use std::io::{self, Stdout};
use std::panic;
use std::sync::Once;

use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};

pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        install_panic_hook();

        enable_raw_mode()?;
        let raw_enabled = true;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = rollback_terminal(raw_enabled, false, false);
            return Err(error);
        }

        let alt_screen_enabled = true;
        if let Err(error) = execute!(stdout, Hide) {
            let _ = rollback_terminal(raw_enabled, alt_screen_enabled, false);
            return Err(error);
        }

        let cursor_hidden = true;

        let backend = CrosstermBackend::new(stdout);
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = rollback_terminal(raw_enabled, alt_screen_enabled, cursor_hidden);
                return Err(error);
            }
        };

        Ok(Self { terminal })
    }

    pub fn draw<F>(&mut self, renderer: F) -> io::Result<()>
    where
        F: FnOnce(&mut Frame<'_>),
    {
        self.terminal.draw(renderer).map(|_| ())
    }

    pub fn size(&self) -> io::Result<(u16, u16)> {
        self.terminal.size().map(|size| (size.width, size.height))
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = restore_terminal();
    }
}

fn restore_terminal() -> io::Result<()> {
    rollback_terminal(true, true, true)
}

fn rollback_terminal(
    raw_enabled: bool,
    alt_screen_enabled: bool,
    cursor_hidden: bool,
) -> io::Result<()> {
    let mut first_error: Option<io::Error> = None;
    let mut stdout = io::stdout();

    if cursor_hidden {
        capture_error(execute!(stdout, Show), &mut first_error);
    }

    if alt_screen_enabled {
        capture_error(execute!(stdout, LeaveAlternateScreen), &mut first_error);
    }

    if raw_enabled {
        capture_error(disable_raw_mode(), &mut first_error);
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn capture_error(result: io::Result<()>, first_error: &mut Option<io::Error>) {
    if let Err(error) = result
        && first_error.is_none()
    {
        *first_error = Some(error);
    }
}

fn install_panic_hook() {
    static ONCE: Once = Once::new();

    ONCE.call_once(|| {
        let previous_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic_info| {
            let _ = restore_terminal();
            previous_hook(panic_info);
        }));
    });
}
