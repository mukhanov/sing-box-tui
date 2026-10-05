//! sing-box-tui — Terminal dashboard for the sing-box VPN.
//!
//! Usage:
//!   sing-box-tui [--config <path>]        run the dashboard
//!   sing-box-tui --import <vless-url> [--out <path>]
//!                                         convert a vless:// link into a
//!                                         new sing-box config and exit
//!
//! Launch with: `sudo sing-box-tui` (launch-time elevation only; the app
//! never prompts for a password — FR-019, SC-014). Terminal lifecycle lives
//! here (T004): raw mode + alternate screen with a panic hook that always
//! restores the shell, and a teardown path that prints the final service
//! state on screen before exit (T039, SC-006).
use std::path::PathBuf;

use sing_box_tui::app;
use sing_box_tui::config;
use sing_box_tui::service::process;

use anyhow::Context;

/// Parsed command line.
enum Cli {
    Run { config: Option<PathBuf> },
    Import { url: String, out: Option<PathBuf> },
    Help,
}

fn parse_cli(args: &[String]) -> Result<Cli, String> {
    let mut config: Option<PathBuf> = None;
    let mut import: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => return Ok(Cli::Help),
            "-c" | "--config" => {
                config = Some(PathBuf::from(next_value(args, &mut i, "--config")?));
            }
            "--import" => {
                import = Some(next_value(args, &mut i, "--import")?);
            }
            "--out" | "-o" => {
                out = Some(PathBuf::from(next_value(args, &mut i, "--out")?));
            }
            other => return Err(format!("unknown argument {other:?} (see --help)")),
        }
        i += 1;
    }
    match import {
        Some(url) => Ok(Cli::Import { url, out }),
        None => Ok(Cli::Run { config }),
    }
}

fn next_value(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

const USAGE: &str = "sing-box-tui — terminal dashboard for sing-box

USAGE:
    sing-box-tui [-c <config>]                       run the dashboard
    sing-box-tui --import <vless-url> [-o <path>]    convert a vless:// link
                                                    to a new config, validate,
                                                    and exit

OPTIONS:
    -c, --config <path>   config file to use (default:
                          ~/.config/sing-box/config.json of the invoking user)
        --import <url>    vless:// sharing link to convert into a config
    -o, --out <path>      target file for --import (default:
                          <config dir>/config-<label|host>.json); never
                          overwrites an existing file
    -h, --help            this help";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_cli(&args).map_err(|e| anyhow::anyhow!("{e}"))? {
        Cli::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Cli::Import { url, out } => import_vless(url, out),
        Cli::Run { config } => run_tui(config),
    }
}

/// Convert a vless:// link into a new config file, validate it, and exit.
fn import_vless(url: String, out: Option<PathBuf>) -> anyhow::Result<()> {
    let params =
        config::vless::parse_vless_url(&url).map_err(|e| anyhow::anyhow!("invalid link: {e}"))?;
    let json = serde_json::to_string_pretty(&config::vless::build_config_value(&params))?;

    // Target: explicit --out, else a new file next to the default config.
    let target = out.unwrap_or_else(|| {
        config::resolve_config_path()
            .parent()
            .map(|d| d.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
            .join(config::vless::config_file_name(&params))
    });

    // Never overwrite: create_new(2) fails atomically if the file exists.
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600) // the file carries credentials
        .open(&target)
        .with_context(|| format!("cannot create {}: file exists?", target.display()))?;
    file.write_all(json.as_bytes())?;
    file.write_all(b"\n")?;
    drop(file);

    // Validate the result so the user never meets a broken profile later.
    let bin = process::find_singbox();
    let check = tokio::runtime::Runtime::new()?.block_on(config::validate::check(&bin, &target));
    if check.ok {
        println!("written {} ({})", target.display(), check.summary());
    } else {
        println!(
            "written {} — INVALID: {}",
            target.display(),
            check.raw.trim_end()
        );
        std::process::exit(1);
    }
    println!("run with: sing-box-tui -c {}", target.display());
    Ok(())
}

fn run_tui(config: Option<PathBuf>) -> anyhow::Result<()> {
    let elevated = is_elevated();
    let config_path = config.unwrap_or_else(config::resolve_config_path);

    let mut terminal = setup_terminal().context("terminal setup failed")?;
    let result =
        tokio::runtime::Runtime::new()?.block_on(app::run(&mut terminal, elevated, config_path));
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
