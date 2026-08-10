mod commands;

use crate::commands::Command;
use clap::Parser;
use colored::{Color, Colorize};
use germ_core::SysConf;
use log::error;
use std::io;
use std::num::NonZeroUsize;
use std::path::PathBuf;

/// Package management tool for Gentoo-like systems.
#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Increase verbosity
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Maximum number of ebuilds to execute concurrently [default: number of CPU cores]
    #[arg(long, value_name = "N")]
    jobs: Option<NonZeroUsize>,

    /// Root path to configuration files
    #[arg(long, value_name = "PATH", default_value = "/")]
    config_root: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let log_level = match args.verbose {
        0 => log::LevelFilter::Info,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };
    setup_logger(log_level).expect("unable to setup logger");

    let sysconf = build_sysconf(&args).into();
    match commands::execute(&args, sysconf).await {
        Ok(()) => {}
        Err(err) => handle_error(err),
    }
}

/// Builds a [`SysConf`] from the given clap `args`.
fn build_sysconf(args: &Args) -> SysConf {
    let mut sysconf = SysConf::new(args.config_root.clone());
    if let Some(jobs) = args.jobs {
        sysconf = sysconf.with_ebuild_jobs(jobs);
    }
    sysconf
}

/// Logs the error cause and stops the process with a non-zero exit code.
fn handle_error(err: anyhow::Error) -> ! {
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

/// Sets up application logger with the given `log_level`.
fn setup_logger(log_level: log::LevelFilter) -> anyhow::Result<()> {
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
