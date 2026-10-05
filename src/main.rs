//! sing-box-tui — Terminal dashboard for the sing-box VPN.
//!
//! Launch with: `sudo sing-box-tui` (launch-time elevation only; the app
//! never prompts for a password — FR-019, SC-014).
//!
//! Terminal lifecycle lives here (T004): raw mode + alternate screen with a
//! panic hook that always restores the shell, and a teardown path that
//! prints the final service state on screen before exit (T039, SC-006).
use sing_box_tui::app;

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let elevated = is_elevated();

    let mut terminal = setup_terminal().context("terminal setup failed")?;
    let result = tokio::runtime::Runtime::new()?.block_on(app::run(&mut terminal, elevated));
    restore_terminal(&mut terminal)?;

    match result {
        Ok(Some(final_state)) => println!("sing-box-tui: {final_state}"),
        Ok(None) => {}
        Err(e) => {
            eprintln!("sing-box-tui: {e:#}");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Enter raw mode + the alternate screen, installing a panic hook that
/// restores the terminal first (FR-021, SC-006).
fn setup_terminal()
-> anyhow::Result<ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let terminal = ratatui::Terminal::new(backend)?;

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        hook(info);
    }));

    Ok(terminal)
}

/// Always leave the shell prompt intact with no leftover screen content.
fn restore_terminal(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) -> anyhow::Result<()> {
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    crossterm::terminal::disable_raw_mode()?;
    terminal.show_cursor()?;
    Ok(())
}

/// Elevated privileges = euid 0 (FR-020a).
fn is_elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}
