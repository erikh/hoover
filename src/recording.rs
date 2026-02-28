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
        mpsc::channel::<(Vec<crate::stt::TranscriptionSegment>, AudioChunk)>(16);

    let stt_config = config.stt.clone();
    std::thread::spawn(move || {
        let mut engine = match stt::create_engine(&stt_config) {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("failed to create STT engine: {e}");
                return;
            }
        };

        tracing::info!("STT engine '{}' initialized", engine.name());

        while let Some(chunk) = stt_rx.blocking_recv() {
            match engine.transcribe(&chunk) {
                Ok(segments) => {
                    if result_tx.blocking_send((segments, chunk)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("transcription error: {e}");
                }
            }
        }

        tracing::debug!("STT thread exiting");
    });

    // Initialize output writer
    let mut writer = MarkdownWriter::new(&config.output)?;

    // Initialize speaker identifier if enabled
    #[cfg(any())]
    // Speaker ID is complex — for now, we skip it in the hot path
    // and just pass None for speaker name. A full implementation would
    // run the identifier in the STT thread.
    let _speaker_id = if config.speaker.enabled {
        // Would initialize SpeakerIdentifier here
        None::<()>
    } else {
        None
    };

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
            Some((segments, _chunk)) = result_rx.recv() => {
                for segment in &segments {
                    if let Err(e) = writer.write_segment(segment, None) {
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

    // Cleanup
    capture.pause()?;

    if let Some(cancel_tx) = cancel_tx {
        let _ = cancel_tx.send(true);
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
