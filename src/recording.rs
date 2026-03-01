use tokio::sync::mpsc;

use crate::audio::buffer::AudioChunk;
use crate::config::Config;
use crate::error::Result;
use crate::output::markdown::MarkdownWriter;
use crate::stt;

/// Main recording loop: capture audio -> STT -> markdown output.
#[allow(clippy::too_many_lines)]
pub async fn run_recording(config: Config) -> Result<()> {
    tracing::info!("starting recording with {} backend", config.stt.backend);

    let (chunk_tx, mut chunk_rx) = mpsc::channel::<AudioChunk>(32);

    // Start audio capture pipeline
    let capture = crate::audio::start_audio_pipeline(&config.audio, chunk_tx.clone())?;
    capture.start()?;
    tracing::info!("audio capture started");

    // Optionally start UDP server
    let cancel_tx = if config.udp.enabled {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let udp_chunk_tx = chunk_tx.clone();
        let udp_config = config.udp.clone();

        tokio::spawn(async move {
            match crate::net::server::UdpServer::bind(&udp_config, udp_chunk_tx).await {
                Ok(mut server) => {
                    if let Err(e) = server.run(cancel_rx).await {
                        tracing::error!("UDP server error: {e}");
                    }
                }
                Err(e) => {
                    tracing::error!("failed to start UDP server: {e}");
                }
            }
        });

        Some(cancel_tx)
    } else {
        None
    };

    // Drop our copy of chunk_tx so the channel closes when audio pipeline stops
    drop(chunk_tx);

    // Create STT engine (runs in a dedicated thread for blocking operations)
    let (stt_tx, mut stt_rx) = mpsc::channel::<AudioChunk>(16);
    let (result_tx, mut result_rx) =
        mpsc::channel::<(Vec<crate::stt::TranscriptionSegment>, Option<String>)>(16);

    let stt_config = config.stt.clone();
    let speaker_config = config.speaker.clone();
    std::thread::spawn(move || {
        let mut engine = match stt::create_engine(&stt_config) {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("failed to create STT engine: {e}");
                return;
            }
        };

        tracing::info!("STT engine '{}' initialized", engine.name());

        // Initialize speaker identifier alongside STT
        let mut speaker_id = if speaker_config.enabled {
            match crate::speaker::identify::SpeakerIdentifier::new(&speaker_config) {
                Ok(id) => Some(id),
                Err(e) => {
                    tracing::warn!("speaker identification disabled: {e}");
                    None
                }
            }
        } else {
            None
        };

        while let Some(chunk) = stt_rx.blocking_recv() {
            let speaker_name = speaker_id.as_mut().and_then(|id| {
                match id.identify(&chunk.samples_f32) {
                    Ok(Some(m)) => m.name,
                    Ok(None) => None, // filter_unknown suppressed this chunk
                    Err(e) => {
                        tracing::warn!("speaker identification error: {e}");
                        None
                    }
                }
            });

            match engine.transcribe(&chunk) {
                Ok(segments) => {
                    if result_tx.blocking_send((segments, speaker_name)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("transcription error: {e}");
                }
            }
        }

        // Flush any pending speaker profile updates before exiting
        if let Some(ref id) = speaker_id {
            id.flush();
        }

        tracing::debug!("STT thread exiting");
    });

    // Initialize output writer
    let mut writer = MarkdownWriter::new(&config.output)?;

    // Set up Ctrl+C handler
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("received Ctrl+C, shutting down...");
        let _ = shutdown_tx.send(());
    });

    // Main processing loop
    loop {
        tokio::select! {
            Some(chunk) = chunk_rx.recv() => {
                if stt_tx.send(chunk).await.is_err() {
                    tracing::error!("STT channel closed");
                    break;
                }
            }
            Some((segments, speaker)) = result_rx.recv() => {
                for segment in &segments {
                    if let Err(e) = writer.write_segment(segment, speaker.as_deref()) {
                        tracing::error!("output error: {e}");
                    }
                }

                // Auto-commit if configured
                if let Err(e) = crate::vcs::auto_commit(&config) {
                    tracing::debug!("auto-commit skipped: {e}");
                }
            }
            _ = &mut shutdown_rx => {
                tracing::info!("shutting down gracefully");
                break;
            }
        }
    }

    // Shutdown: stop capture and cancel UDP server
    if let Some(cancel_tx) = cancel_tx {
        let _ = cancel_tx.send(true);
    }

    // Drop capture to close the audio channel, which causes the audio pipeline
    // thread to flush its accumulator and exit.
    drop(capture);

    // Drain any remaining audio chunks and forward them to the STT engine.
    while let Some(chunk) = chunk_rx.recv().await {
        if stt_tx.send(chunk).await.is_err() {
            break;
        }
    }

    // Drop stt_tx so the STT thread sees the channel close and exits after
    // finishing its current work.
    drop(stt_tx);

    // Drain all remaining transcription results.
    while let Some((segments, speaker)) = result_rx.recv().await {
        for segment in &segments {
            if let Err(e) = writer.write_segment(segment, speaker.as_deref()) {
                tracing::error!("output error: {e}");
            }
        }
    }

    // Final commit and push
    if let Err(e) = crate::vcs::auto_commit(&config) {
        tracing::debug!("final commit: {e}");
    }
    if let Err(e) = crate::vcs::auto_push(&config) {
        tracing::debug!("final push: {e}");
    }

    tracing::info!("recording stopped");
    Ok(())
}
