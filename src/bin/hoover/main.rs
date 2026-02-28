use std::io::Write;
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

    /// List available audio input devices
    Devices {
        /// Write the chosen device name to the config file
        #[arg(long, conflicts_with = "pick")]
        set: Option<String>,

        /// Interactively pick a device and save it to the config file
        #[arg(long, conflicts_with = "set")]
        pick: bool,
    },

    /// Start the MCP server (stdio transport)
    #[cfg(feature = "mcp")]
    Mcp,
}

fn load_config(cli: &Cli) -> Result<Config, HooverError> {
    let path = cli.config.clone().unwrap_or_else(Config::default_path);
    Config::load(&path)
}

fn config_path(cli: &Cli) -> PathBuf {
    cli.config.clone().unwrap_or_else(Config::default_path)
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
    match cli.command {
        Command::Devices {
            ref set,
            pick,
        } => run_devices(&cli, set.as_deref(), pick),
        _ => run_with_config(cli),
    }
}

fn list_devices() -> Result<(Vec<String>, Option<String>), HooverError> {
    let devices = hoover::audio::capture::list_input_devices()?;
    let default_name = hoover::audio::capture::default_input_device_name();
    Ok((devices, default_name))
}

fn print_device_list(devices: &[String], default_name: Option<&str>) {
    for (i, name) in devices.iter().enumerate() {
        let marker = if default_name == Some(name.as_str()) {
            " (default)"
        } else {
            ""
        };
        println!("  {}: {name}{marker}", i + 1);
    }
}

fn run_devices(cli: &Cli, set: Option<&str>, pick: bool) -> Result<(), HooverError> {
    if let Some(device_name) = set {
        let path = config_path(cli);
        Config::set_audio_device(&path, device_name)?;
        println!("Set audio device to: {device_name}");
        return Ok(());
    }

    let (devices, default_name) = list_devices()?;

    if devices.is_empty() {
        println!("No audio input devices found.");
        return Ok(());
    }

    if pick {
        println!("Available audio input devices:");
        print_device_list(&devices, default_name.as_deref());
        println!();

        print!("Select device [1-{}]: ", devices.len());
        std::io::stdout()
            .flush()
            .map_err(|e| HooverError::Other(format!("failed to flush stdout: {e}")))?;

        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| HooverError::Other(format!("failed to read input: {e}")))?;

        let choice: usize = input
            .trim()
            .parse()
            .map_err(|_| HooverError::Other("invalid selection: enter a number".to_string()))?;

        if choice < 1 || choice > devices.len() {
            return Err(HooverError::Other(format!(
                "selection out of range: pick 1-{}",
                devices.len()
            )));
        }

        let selected = &devices[choice - 1];
        let path = config_path(cli);
        Config::set_audio_device(&path, selected)?;
        println!("Set audio device to: {selected}");
    } else {
        print_device_list(&devices, default_name.as_deref());
    }

    Ok(())
}

fn run_with_config(cli: Cli) -> Result<(), HooverError> {
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
        Command::Devices { .. } => unreachable!(),
    }
}
