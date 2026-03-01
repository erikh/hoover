# hoover

Spy on yourself for good. Hoover is a continuous audio transcription tool that
captures microphone input, transcribes it using speech-to-text, and stores
timestamped daily markdown logs.

## Features

- **Audio capture** -- records from your microphone using CPAL, with
  configurable chunk duration and overlap for continuous transcription.
- **Multiple STT backends** -- supports Whisper (local, default), Vosk (local),
  and OpenAI Whisper API (remote). Compile only what you need with feature
  flags.
- **Speaker identification** -- enroll speaker voice profiles using ECAPA-TDNN
  embeddings (ONNX), then automatically tag transcription segments with speaker
  names. Use this to isolate your own voice and protect the privacy of others --
  enable `filter_unknown` to drop segments that don't match an enrolled profile,
  ensuring only your speech is recorded.
- **Daily markdown output** -- transcriptions are written to date-stamped
  markdown files (`YYYY-MM-DD.md`) with timestamps and optional speaker labels.
  Overlapping segments are deduplicated.
- **Version control** -- automatically commits and pushes transcription files to
  a git repository. Supports triggering GitHub Actions or Gitea workflows on
  push.
- **Encrypted UDP streaming** -- send audio between machines over AES-256-GCM
  encrypted UDP with serial-number-based ordering. The firewall integration
  automatically blocks sources that fail decryption, protecting you from
  impersonation by ensuring only trusted senders can feed audio into your
  transcription pipeline.
- **MCP server** -- exposes transcription data as an MCP (Model Context
  Protocol) tool server over stdio, allowing AI assistants to search and query
  your transcription history.

## Build

```sh
# Default (whisper backend)
cargo build

# All features
cargo build --features "whisper,vosk,openai,github,gitea,mcp"
```

### Feature flags

| Feature   | Description                                    |
|-----------|------------------------------------------------|
| `whisper` | Local Whisper STT via whisper-rs (default)      |
| `vosk`    | Local Vosk STT (requires libvosk system library)|
| `openai`  | OpenAI Whisper API backend                      |
| `github`  | GitHub Actions workflow trigger on push          |
| `gitea`   | Gitea workflow trigger on push                   |
| `mcp`     | MCP server for AI assistant integration          |

## Usage

```sh
# Interactive first-time setup
hoover init

# List audio input devices, or pick/set one
hoover devices
hoover devices --pick
hoover devices --set "My Microphone"

# Start recording (foreground, Ctrl+C to stop)
hoover record

# Enroll a speaker voice profile
hoover enroll "Alice"

# Push transcription repo
hoover push

# Trigger a forge workflow
hoover trigger

# Send audio to a remote hoover instance
hoover send <host:port> [--file audio.wav] [--key-file key.bin]

# Start MCP server (requires mcp feature)
hoover mcp
```

### Global options

- `--config <path>` -- path to config file (default: `~/.config/hoover/config.yaml`)
- `--verbose` / `-v` -- enable debug logging

### Getting started

Run `hoover init` to walk through an interactive setup wizard. It will prompt
you for audio device, STT backend, output directory, speaker identification,
and version control settings, then write the config file. After that, run
`hoover record` to start transcribing.

## Configuration

Hoover is configured via a YAML file (default `~/.config/hoover/config.yaml`).
All sections are optional with sensible defaults. See
[`config.example.yaml`](config.example.yaml) for a fully commented example.

```yaml
audio:
  device: "My Microphone"    # omit for system default
  chunk_duration_secs: 30
  overlap_secs: 5

stt:
  backend: whisper           # whisper | vosk | openai
  language: en
  whisper_model_size: base
  # model_path: /path/to/model  # required for vosk
  # openai_api_key: sk-...      # required for openai

speaker:
  enabled: true              # enabled by default
  profiles_dir: ~/.local/share/hoover/speakers
  min_confidence: 0.7
  filter_unknown: false      # drop segments from unrecognized speakers
  # model_path: /path/to/custom_model.onnx  # omit to auto-download default

output:
  directory: ~/hoover
  timestamps: true

vcs:
  enabled: false
  auto_commit: false
  auto_push: false
  remote: origin
  # github:
  #   token: ghp_xxx
  #   owner: erikh
  #   repo: hoover
  #   workflow: ci.yml
  # gitea:
  #   url: https://gitea.example.com
  #   token: xxx
  #   owner: erikh
  #   repo: hoover

udp:
  enabled: false
  bind: "0.0.0.0:9700"
  key_file: ~/.config/hoover/udp.key
  backlog: 1000
  firewall:
    enabled: false
    backend: firewalld       # firewalld | nftables
    block_duration_secs: 3600

mcp:
  enabled: false
```

## Recording behavior

When `hoover record` is running, audio is captured in configurable chunks
(default 30 seconds with 5 seconds of overlap) and sent to the STT engine.
Non-speech audio such as keyboard tapping, mouse clicks, and other mechanical
sounds is automatically filtered out using Whisper's no-speech probability
detection. Common Whisper hallucinations from background noise (e.g.
`[MUSIC]`, `(keyboard clicking)`, phantom "Thank you" segments) are also
suppressed.

Speaker identification is enabled by default. Each audio chunk is run through
the ECAPA-TDNN embedding model alongside transcription, and the closest
enrolled speaker name is attached to the output. If no profiles have been
enrolled yet, segments are written without a speaker tag. Set
`speaker.enabled: false` in the config to disable it.

When you press Ctrl+C, hoover performs a graceful shutdown: it flushes any
buffered audio through the STT engine, writes all remaining transcription
segments to the markdown file, and then runs the final git commit and push
(if configured). No in-flight audio is lost.

### Output format

Daily transcription files use `HH:MM` headings to group segments by minute.
Segments within the same minute appear under a single heading. When speaker
identification is active, the speaker name is shown as a bold prefix:

```markdown
# Friday, February 28, 2026

## 14:30

Hello, this is some transcribed text.

More text in the same minute, no duplicate heading.

## 14:31

**Alice:** Speaker-tagged text when identification is enabled.

Untagged text when the speaker is unknown.
```

## Speaker enrollment

Hoover uses an ECAPA-TDNN ONNX model for speaker embeddings. The default model
is auto-downloaded on first use. To use a custom ONNX model, set
`speaker.model_path` in your config. The input tensor rank (2 or 3) is detected
automatically, so any compatible ONNX speaker embedding model works.

```sh
hoover enroll "Alice"
# Speak for 10-30 seconds, then press Ctrl+C
```

Profiles are saved as `.bin` files in the speaker profiles directory.

## Authentication

GitHub and Gitea push/trigger operations require an access token. Hoover
resolves tokens in this order:

1. `vcs.github.token` (or `vcs.gitea.token`) in the config file
2. `GITHUB_TOKEN` environment variable (GitHub only)
3. `GH_TOKEN` environment variable (GitHub only)
4. `gh auth token` CLI fallback (GitHub only)
5. `GITEA_TOKEN` environment variable (Gitea only)

Owner and repo are resolved from the config, or detected automatically from
the git remote URL in the output directory.

## MCP server

When compiled with the `mcp` feature, `hoover mcp` starts an MCP server on
stdio with the following tools:

- `search_transcriptions` -- full-text search across all transcription files
  with optional date range filtering
- `get_day` -- retrieve the full transcription for a specific date
- `list_dates` -- list all available transcription dates
- `get_date_range` -- retrieve transcriptions for a date range
- `get_summary` -- summary statistics (number of days, entries, date range)
- `get_speakers` -- list enrolled speaker profiles

## License

AGPL-3.0-or-later
