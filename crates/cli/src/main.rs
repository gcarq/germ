mod commands;

use crate::commands::Command;
use anyhow::{Context, Result};
use clap::Parser;
use colored::{Color, Colorize};
use log::error;
use pkgrove_core::consts::DEFAULT_USE_PORTAGE_CONF_PATH;
use pkgrove_core::repository::set::RepoSet;
use std::io;
use std::path::Path;

/// Package management tool for Gentoo-like systems.
#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Increase verbosity
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

fn main() {
    let args = Args::parse();
    let log_level = match args.verbose {
        0 => log::LevelFilter::Info,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };
    setup_logger(log_level).expect("unable to setup logger");

    match run(args) {
        Ok(()) => (),
        Err(err) => {
            let error_cause = err
                .chain()
                .skip(1)
                .enumerate()
                .map(|(i, cause)| format!("   {i}: {cause}"))
                .collect::<Vec<_>>()
                .join("\n");
            if error_cause.is_empty() {
                error!("{err}");
            } else {
                error!("{err}\nCaused by\n{error_cause}");
            }
            std::process::exit(1);
        }
    }
}

/// Main application logic is here.
fn run(args: Args) -> Result<()> {
    let config_path = Path::new(DEFAULT_USE_PORTAGE_CONF_PATH).join("repos.conf");
    let mut repo_set =
        RepoSet::new(&config_path).with_context(|| "unable to process repos.conf")?;
    commands::execute(&args.command, &mut repo_set)?;

    repo_set.flush(false)
}

/// Sets up application logger with the given `log_level`.
fn setup_logger(log_level: log::LevelFilter) -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            let color = match record.level() {
                log::Level::Error => Color::Red,
                log::Level::Warn => Color::Yellow,
                log::Level::Info => Color::Green,
                log::Level::Debug => Color::Cyan,
                log::Level::Trace => Color::BrightCyan,
            };
            let level = record.level().to_string().color(color);
            let format = match record.level() {
                log::Level::Trace | log::Level::Debug => {
                    format_args!("[{level}] {} - {message}", record.target())
                }
                _ => format_args!("[{level}] {message}"),
            };
            out.finish(format);
        })
        .level(log_level)
        .chain(io::stdout())
        .apply()?;
    Ok(())
}
