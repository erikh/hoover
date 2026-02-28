use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use hoover::config::Config;
use hoover::error::HooverError;

#[derive(Parser)]
#[command(name = "hoover", about = "spy on yourself for good")]
struct Cli {
    /// Path to config file
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(long, short, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start recording from the microphone (foreground)
    Record,

    /// Manually push the transcription repository
    Push,

    /// Manually trigger a forge action (GitHub/Gitea workflow)
    Trigger,

    /// Enroll a speaker voice profile
    Enroll {
        /// Name of the speaker to enroll
        name: String,
    },

    /// Send audio to a remote hoover instance via encrypted UDP
    Send {
        /// Target address (host:port)
        target: String,

        /// Audio file to send (reads from stdin if omitted)
        #[arg(long)]
        file: Option<PathBuf>,

        /// Path to the shared key file
        #[arg(long)]
        key_file: Option<PathBuf>,
    },

    /// Start the MCP server (stdio transport)
    #[cfg(feature = "mcp")]
    Mcp,
}

fn load_config(cli: &Cli) -> Result<Config, HooverError> {
    let path = cli.config.clone().unwrap_or_else(Config::default_path);
    Config::load(&path)
}

fn init_logging(verbose: bool) {
    let filter = if verbose {
        EnvFilter::new("hoover=debug,info")
    } else {
        EnvFilter::new("hoover=info,warn")
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn main() {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    let result = run(cli);
    if let Err(e) = result {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), HooverError> {
    let config = load_config(&cli)?;

    match cli.command {
        Command::Record => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(hoover::recording::run_recording(config))
        }
        Command::Push => hoover::vcs::push(&config),
        Command::Trigger => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(hoover::vcs::trigger(&config))
        }
        Command::Enroll { name } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(hoover::speaker::enroll::run_enrollment(&config, &name))
        }
        Command::Send {
            target,
            file,
            key_file,
        } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(hoover::net::client::run_sender(
                &config,
                &target,
                file.as_deref(),
                key_file.as_deref(),
            ))
        }
        #[cfg(feature = "mcp")]
        Command::Mcp => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(hoover::mcp::run_mcp_server(config))
        }
    }
}
