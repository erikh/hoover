use std::io::Write;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use tracing_subscriber::EnvFilter;

use hoover::config::Config;
use hoover::error::HooverError;

#[derive(Parser)]
#[command(
    name = "hoover",
    about = "Spy on yourself for good",
    long_about = "Hoover is a continuous audio transcription tool that captures microphone \
        input, transcribes it using speech-to-text, and stores timestamped daily \
        markdown logs. It supports speaker identification to isolate your voice \
        and protect the privacy of others, version-controlled output with GitHub \
        and Gitea integration, encrypted UDP streaming between machines, and an \
        MCP server for AI assistant integration."
)]
struct Cli {
    /// Path to config file
    ///
    /// Defaults to ~/.config/hoover/config.yaml if not specified.
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Enable verbose logging
    ///
    /// Sets the log level to debug for the hoover crate, showing detailed
    /// information about audio capture, transcription, and VCS operations.
    #[arg(long, short, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start recording from the microphone (foreground)
    ///
    /// Captures audio from the configured input device, transcribes it in
    /// chunks using the configured STT backend, and appends timestamped
    /// results to a daily markdown file. If speaker identification is
    /// enabled, segments are tagged with the recognized speaker name.
    /// Runs until interrupted with Ctrl+C.
    Record,

    /// Manually push the transcription repository
    ///
    /// Pushes the output directory's git repository to the configured
    /// remote. If a GitHub or Gitea token is available (from config,
    /// environment variables, or the gh CLI), it will be used to
    /// authenticate the push over HTTPS.
    Push,

    /// Manually trigger a forge action (GitHub/Gitea workflow)
    ///
    /// Dispatches a workflow run on the configured GitHub Actions or Gitea
    /// Actions workflow. Requires a token and repository to be configured
    /// or detectable from the environment.
    Trigger,

    /// Enroll a speaker voice profile
    ///
    /// Records a short audio sample and computes an ECAPA-TDNN voice
    /// embedding that is saved as a speaker profile. Once enrolled,
    /// hoover can identify this speaker during transcription and tag
    /// their segments accordingly. Speak for 10-30 seconds, then press
    /// Ctrl+C to finish enrollment.
    Enroll {
        /// Name of the speaker to enroll
        name: String,
    },

    /// Send audio to a remote hoover instance via encrypted UDP
    ///
    /// Streams audio data to a remote hoover instance over AES-256-GCM
    /// encrypted UDP. The shared key file must match on both ends.
    /// Packets are serial-numbered for ordering and replay detection.
    /// Can send from a file or read audio from stdin.
    Send {
        /// Target address (host:port)
        target: String,

        /// Audio file to send (reads from stdin if omitted)
        #[arg(long)]
        file: Option<PathBuf>,

        /// Path to the shared key file
        ///
        /// Defaults to ~/.config/hoover/udp.key if not specified.
        #[arg(long)]
        key_file: Option<PathBuf>,
    },

    /// List or manage enrolled speaker profiles
    ///
    /// Shows all enrolled speaker profiles. Use --remove to delete a
    /// speaker's profile by name.
    Speakers {
        /// Remove an enrolled speaker profile by name
        #[arg(long)]
        remove: Option<String>,
    },

    /// List available audio input devices
    ///
    /// Shows all audio input devices recognized by the system. Use --pick
    /// to interactively select one and save it to your config file, or
    /// use --set to write a device name directly.
    Devices {
        /// Write the chosen device name to the config file
        #[arg(long, conflicts_with = "pick")]
        set: Option<String>,

        /// Interactively pick a device and save it to the config file
        #[arg(long, conflicts_with = "set")]
        pick: bool,
    },

    /// Create a new configuration file
    ///
    /// Walks through an interactive setup to configure audio input,
    /// speech-to-text backend, output directory, speaker identification,
    /// and version control. Writes the result to the config file.
    Init,

    /// Start the MCP server (stdio transport)
    ///
    /// Exposes transcription data over the Model Context Protocol,
    /// allowing AI assistants to search and query your transcription
    /// history. Communicates over stdin/stdout.
    #[cfg(feature = "mcp")]
    Mcp,

    /// Generate shell completions
    ///
    /// Prints a completion script for the given shell to stdout.
    /// Source or install the output to enable tab completion.
    Completions {
        /// Shell to generate completions for (bash, zsh, fish, elvish, powershell)
        shell: Shell,
    },
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
    install_completions_if_missing();

    let cli = Cli::parse();
    init_logging(cli.verbose);

    let result = run(cli);
    if let Err(e) = result {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

/// Auto-install shell completions for `$SHELL` if the completion file does not
/// already exist.  Runs silently — errors are ignored so that missing dirs or
/// unsupported shells never block normal operation.
fn install_completions_if_missing() {
    let Ok(shell_env) = std::env::var("SHELL") else {
        return;
    };

    let Some(home) = dirs::home_dir() else {
        return;
    };

    // Map $SHELL to a clap_complete Shell variant and a destination path.
    let (shell, path) = if shell_env.ends_with("/bash") {
        let dir = home.join(".local/share/bash-completion/completions");
        (Shell::Bash, dir.join("hoover"))
    } else if shell_env.ends_with("/zsh") {
        (Shell::Zsh, home.join(".zfunc/_hoover"))
    } else if shell_env.ends_with("/fish") {
        (Shell::Fish, home.join(".config/fish/completions/hoover.fish"))
    } else {
        return;
    };

    if path.exists() {
        return;
    }

    // Create parent directory if needed.
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }

    let mut buf = Vec::new();
    generate(shell, &mut Cli::command(), "hoover", &mut buf);

    let _ = std::fs::write(&path, buf);
}

fn run(cli: Cli) -> Result<(), HooverError> {
    match cli.command {
        Command::Devices {
            ref set,
            pick,
        } => run_devices(&cli, set.as_deref(), pick),
        Command::Init => run_init(&cli),
        Command::Completions { shell } => {
            generate(shell, &mut Cli::command(), "hoover", &mut std::io::stdout());
            Ok(())
        }
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
        Command::Speakers { remove } => run_speakers(&config, remove.as_deref()),
        Command::Devices { .. } | Command::Init | Command::Completions { .. } => unreachable!(),
    }
}

fn run_speakers(config: &Config, remove: Option<&str>) -> Result<(), HooverError> {
    let profiles_dir = Config::expand_path(&config.speaker.profiles_dir);

    if let Some(name) = remove {
        hoover::speaker::enroll::remove_profile(&profiles_dir, name)?;
        println!("Removed speaker profile: {name}");
        return Ok(());
    }

    let names = hoover::speaker::enroll::list_profiles(&profiles_dir)?;
    if names.is_empty() {
        println!("No enrolled speakers. Use `hoover enroll <name>` to add one.");
    } else {
        println!("Enrolled speakers:");
        for name in &names {
            println!("  {name}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Prompt helpers
// ---------------------------------------------------------------------------

fn prompt(msg: &str) -> Result<String, HooverError> {
    print!("{msg}");
    std::io::stdout()
        .flush()
        .map_err(|e| HooverError::Other(format!("failed to flush stdout: {e}")))?;
    let mut buf = String::new();
    std::io::stdin()
        .read_line(&mut buf)
        .map_err(|e| HooverError::Other(format!("failed to read input: {e}")))?;
    Ok(buf.trim().to_string())
}

fn prompt_default(msg: &str, default: &str) -> Result<String, HooverError> {
    let input = prompt(&format!("{msg} [{default}]: "))?;
    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input)
    }
}

fn prompt_yes_no(msg: &str, default_yes: bool) -> Result<bool, HooverError> {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    let input = prompt(&format!("{msg} [{hint}]: "))?;
    if input.is_empty() {
        return Ok(default_yes);
    }
    match input.to_lowercase().as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Ok(default_yes),
    }
}

fn prompt_choice(msg: &str, options: &[&str]) -> Result<usize, HooverError> {
    println!("{msg}");
    for (i, opt) in options.iter().enumerate() {
        println!("  {}: {opt}", i + 1);
    }
    let input = prompt(&format!("Select [1-{}]: ", options.len()))?;
    let choice: usize = input
        .parse()
        .map_err(|_| HooverError::Other("invalid selection: enter a number".to_string()))?;
    if choice < 1 || choice > options.len() {
        return Err(HooverError::Other(format!(
            "selection out of range: pick 1-{}",
            options.len()
        )));
    }
    Ok(choice - 1)
}

// ---------------------------------------------------------------------------
// YAML builder helper
// ---------------------------------------------------------------------------

fn yaml_section<'a>(
    root: &'a mut serde_yaml_ng::Mapping,
    key: &str,
) -> Result<&'a mut serde_yaml_ng::Mapping, HooverError> {
    let k = serde_yaml_ng::Value::String(key.to_string());
    root.entry(k)
        .or_insert_with(|| {
            serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new())
        })
        .as_mapping_mut()
        .ok_or_else(|| HooverError::Config(format!("{key} section is not a mapping")))
}

// ---------------------------------------------------------------------------
// hoover init
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn run_init(cli: &Cli) -> Result<(), HooverError> {
    use serde_yaml_ng::{Mapping, Value};

    let path = config_path(cli);

    // 1. Config path check
    if path.exists() {
        let overwrite = prompt_yes_no(
            &format!("Config file already exists at {}. Overwrite?", path.display()),
            false,
        )?;
        if !overwrite {
            println!("Aborted.");
            return Ok(());
        }
    }

    let mut root = Mapping::new();

    // 2. Audio device
    println!();
    let pick_device = prompt_yes_no("Pick an audio input device?", true)?;
    if pick_device {
        let (devices, default_name) = list_devices()?;
        if devices.is_empty() {
            println!("No audio input devices found, skipping.");
        } else {
            println!("Available audio input devices:");
            print_device_list(&devices, default_name.as_deref());
            println!();
            let input = prompt(&format!(
                "Select device [1-{}] (Enter to skip): ",
                devices.len()
            ))?;
            if let Ok(choice) = input.parse::<usize>()
                && choice >= 1
                && choice <= devices.len()
            {
                let audio = yaml_section(&mut root, "audio")?;
                audio.insert(
                    Value::String("device".to_string()),
                    Value::String(devices[choice - 1].clone()),
                );
            }
        }
    }

    // 3. STT backend
    println!();
    let backend_idx = prompt_choice(
        "Speech-to-text backend:",
        &["Whisper (default)", "Vosk", "OpenAI"],
    )?;
    match backend_idx {
        0 => {
            // Whisper — only write non-default model size
            let model = prompt_default(
                "Whisper model size (tiny/base/small/medium/large)",
                "base",
            )?;
            if model != "base" {
                let stt = yaml_section(&mut root, "stt")?;
                stt.insert(
                    Value::String("whisper_model_size".to_string()),
                    Value::String(model),
                );
            }
        }
        1 => {
            // Vosk
            let stt = yaml_section(&mut root, "stt")?;
            stt.insert(
                Value::String("backend".to_string()),
                Value::String("vosk".to_string()),
            );
            let model_path = prompt("Vosk model path: ")?;
            if !model_path.is_empty() {
                stt.insert(
                    Value::String("model_path".to_string()),
                    Value::String(model_path),
                );
            }
        }
        2 => {
            // OpenAI
            let stt = yaml_section(&mut root, "stt")?;
            stt.insert(
                Value::String("backend".to_string()),
                Value::String("openai".to_string()),
            );
            let api_key = prompt("OpenAI API key: ")?;
            if !api_key.is_empty() {
                stt.insert(
                    Value::String("openai_api_key".to_string()),
                    Value::String(api_key),
                );
            }
        }
        _ => unreachable!(),
    }

    // 4. Language
    println!();
    let lang = prompt_default("Language", "en")?;
    if lang != "en" {
        let stt = yaml_section(&mut root, "stt")?;
        stt.insert(
            Value::String("language".to_string()),
            Value::String(lang),
        );
    }

    // 5. Output directory
    println!();
    let default_out = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hoover")
        .to_string_lossy()
        .to_string();
    let out_dir = prompt_default("Output directory", &default_out)?;
    if out_dir != default_out {
        let output = yaml_section(&mut root, "output")?;
        output.insert(
            Value::String("directory".to_string()),
            Value::String(out_dir),
        );
    }

    // 6. Speaker identification
    println!();
    let speaker_enabled = prompt_yes_no("Enable speaker identification?", false)?;
    if speaker_enabled {
        let speaker = yaml_section(&mut root, "speaker")?;
        speaker.insert(
            Value::String("enabled".to_string()),
            Value::Bool(true),
        );
        let filter = prompt_yes_no(
            "Filter out unrecognized speakers?",
            false,
        )?;
        if filter {
            speaker.insert(
                Value::String("filter_unknown".to_string()),
                Value::Bool(true),
            );
        }
    }

    // 7. VCS
    println!();
    let vcs_enabled = prompt_yes_no("Enable version control (git)?", false)?;
    if vcs_enabled {
        let vcs = yaml_section(&mut root, "vcs")?;
        vcs.insert(
            Value::String("enabled".to_string()),
            Value::Bool(true),
        );
        let auto_commit = prompt_yes_no("Auto-commit after each recording chunk?", false)?;
        if auto_commit {
            vcs.insert(
                Value::String("auto_commit".to_string()),
                Value::Bool(true),
            );
        }
        let auto_push = prompt_yes_no("Auto-push after commits?", false)?;
        if auto_push {
            vcs.insert(
                Value::String("auto_push".to_string()),
                Value::Bool(true),
            );
        }
    }

    // 8. Write config
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            HooverError::Config(format!(
                "failed to create config directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    let yaml = serde_yaml_ng::to_string(&Value::Mapping(root)).map_err(|e| {
        HooverError::Config(format!("failed to serialize config: {e}"))
    })?;

    std::fs::write(&path, &yaml).map_err(|e| {
        HooverError::Config(format!(
            "failed to write config file {}: {e}",
            path.display()
        ))
    })?;

    // 9. Summary
    println!();
    println!("Config written to {}", path.display());
    println!("Run `hoover record` to start transcribing.");

    Ok(())
}
