# hoover

Spy on yourself for good. Hoover is a continuous audio transcription tool that
captures microphone input, transcribes it using speech-to-text, and stores
timestamped daily markdown logs.

## Features

- **Audio capture** -- records from your microphone using CPAL, with
  configurable chunk duration and overlap for continuous transcription.
- **Multiple STT backends** -- supports Whisper (local, default), Vosk (local),
  and OpenAI Whisper API (remote). All backends are always compiled in.
- **GPU acceleration** -- NVIDIA CUDA (default) and AMD ROCm are supported as
  compile-time features, with a runtime `gpu` toggle in the config.
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
# Default (NVIDIA CUDA GPU acceleration)
cargo build

# AMD ROCm GPU acceleration
cargo build --no-default-features --features rocm

# CPU only (no GPU)
cargo build --no-default-features
```

### Feature flags

All STT backends (Whisper, Vosk, OpenAI) and integrations (GitHub, Gitea, MCP)
are always compiled in. The only feature flags control GPU acceleration:

| Feature | Description                                         |
|---------|-----------------------------------------------------|
| `cuda`  | NVIDIA CUDA GPU acceleration (default)               |
| `rocm`  | AMD ROCm/HIP GPU acceleration                        |
| `nogpu` | Explicitly disable GPU (same as `--no-default-features`) |

`cuda` and `rocm` are mutually exclusive. To switch from the default CUDA to
ROCm, use `--no-default-features --features rocm`.

### System dependencies

- **libvosk** -- required for the Vosk STT backend (e.g. `pacman -S vosk-api`
  on Arch/Manjaro)
- **CUDA toolkit** -- required when building with the `cuda` feature
- **ROCm/HIP** -- required when building with the `rocm` feature

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

# List enrolled speakers
hoover speakers

# Remove a speaker profile
hoover speakers --remove "Alice"

# Push transcription repo
hoover push

# Trigger a forge workflow
hoover trigger

# Send audio to a remote hoover instance
hoover send <host:port> [--file audio.wav] [--key-file key.bin]

# Start MCP server
hoover mcp

# Generate shell completions
hoover completions bash
hoover completions zsh
hoover completions fish
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
  chunk_duration_secs: 60
  overlap_secs: 5

stt:
  backend: whisper           # whisper | vosk | openai
  language: en
  whisper_model_size: small
  gpu: true                  # use GPU acceleration when available
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
(default 60 seconds with 5 seconds of overlap) and sent to the STT engine.
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

## Speaker identification

Speaker identification is enabled by default and runs alongside transcription.
Each audio chunk is processed through an ECAPA-TDNN embedding model to produce a
voice fingerprint, which is compared against enrolled speaker profiles. The
closest match above the confidence threshold is attached to the transcription
output as a speaker label.

When multiple speakers are enrolled, hoover can be used to transcribe meetings
and group conversations -- each segment is automatically tagged with the
speaker's name, producing a readable transcript of who said what.

### Embedding model

Hoover uses an ECAPA-TDNN ONNX model (WeSpeaker) for speaker embeddings. The
default model is auto-downloaded from HuggingFace on first use and cached at
`~/.local/share/hoover/models/speaker_embedding.onnx`. To use a custom ONNX
model, set `speaker.model_path` in your config. The input tensor rank (2 or 3)
is detected automatically, so any compatible ONNX speaker embedding model works.

Audio is converted to 80-dimensional log Mel filterbank features (Kaldi-compatible
defaults: 16 kHz, 25 ms window, 10 ms hop) before being fed to the model.

### Enrolling a speaker

To enroll a speaker, run the `enroll` command and speak for 10--30 seconds:

```sh
hoover enroll "Alice"
# Speak naturally for 10-30 seconds, then press Ctrl+C to finish
```

During enrollment, hoover records audio from your configured microphone, splits
it into 3-second segments, extracts an embedding from each segment, and averages
them to create a stable voice profile. At least 3 seconds of audio is required;
longer recordings produce more robust profiles. The profile is saved as a `.bin`
file in the profiles directory (default `~/.local/share/hoover/speakers/`).

To re-enroll a speaker (e.g. to improve recognition), simply run `hoover enroll`
again with the same name. The new profile overwrites the old one.

### Managing speaker profiles

List all enrolled speakers:

```sh
hoover speakers
```

Remove a speaker profile:

```sh
hoover speakers --remove "Alice"
```

### Continuous training

Speaker profiles are automatically refined during recording. When a speaker is
identified with confidence above the threshold, the stored embedding is updated
using an exponential moving average (EMA) with a blending factor of 0.05. This
means the profile slowly adapts to the speaker's voice over time, improving
accuracy as more speech is recorded.

Updated profiles are saved to disk every 10 successful identifications, and any
pending updates are flushed on graceful shutdown (Ctrl+C). This continuous
training happens transparently -- no manual re-enrollment is needed after the
initial setup.

### Privacy filtering

Set `speaker.filter_unknown: true` to drop any audio segment that does not match
an enrolled speaker profile. This ensures that only enrolled voices appear in the
transcription output, protecting the privacy of bystanders and other speakers
whose voices you have not enrolled.

### Configuration reference

| Option              | Default                              | Description                                                   |
|---------------------|--------------------------------------|---------------------------------------------------------------|
| `enabled`           | `true`                               | Enable or disable speaker identification                      |
| `profiles_dir`      | `~/.local/share/hoover/speakers`     | Directory where `.bin` profile files are stored                |
| `min_confidence`    | `0.7`                                | Cosine similarity threshold for a positive speaker match       |
| `filter_unknown`    | `false`                              | Drop segments that don't match any enrolled speaker            |
| `model_path`        | *(auto-download)*                    | Path to a custom ONNX speaker embedding model                 |

### How identification works

1. Each audio chunk (default 60 seconds) is processed through the ECAPA-TDNN
   model to extract a 512-dimensional embedding vector.
2. The embedding is compared against all enrolled profiles using cosine
   similarity.
3. If the highest similarity score is above `min_confidence`, the segment is
   tagged with that speaker's name and the profile is refined via EMA.
4. If no profile exceeds the threshold and `filter_unknown` is `false`, the
   segment is written without a speaker tag.
5. If no profile exceeds the threshold and `filter_unknown` is `true`, the
   segment is silently discarded.

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

`hoover mcp` starts an MCP server on stdio with the following tools:

- `search_transcriptions` -- full-text search across all transcription files
  with optional date range filtering
- `get_day` -- retrieve the full transcription for a specific date
- `list_dates` -- list all available transcription dates
- `get_date_range` -- retrieve transcriptions for a date range
- `get_summary` -- summary statistics (number of days, entries, date range)
- `get_speakers` -- list enrolled speaker profiles

## License

AGPL-3.0-or-later
