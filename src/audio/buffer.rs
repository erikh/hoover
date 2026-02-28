use chrono::{DateTime, Utc};

const SAMPLE_RATE: u32 = 16000;

/// A chunk of 16kHz mono audio ready for STT processing.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub samples_f32: Vec<f32>,
    pub samples_i16: Vec<i16>,
    pub timestamp: DateTime<Utc>,
    pub duration_secs: f32,
}

impl AudioChunk {
    fn from_samples(samples: &[f32], timestamp: DateTime<Utc>) -> Self {
        let samples_i16: Vec<i16> = samples
            .iter()
            .map(|&s| {
                let clamped = s.clamp(-1.0, 1.0);
                (clamped * f32::from(i16::MAX)) as i16
            })
            .collect();

        let duration_secs = samples.len() as f32 / SAMPLE_RATE as f32;

        Self {
            samples_f32: samples.to_vec(),
            samples_i16,
            timestamp,
            duration_secs,
        }
    }
}

/// Accumulates 16kHz mono samples into overlapping chunks.
pub struct ChunkAccumulator {
    buffer: Vec<f32>,
    chunk_samples: usize,
    overlap_samples: usize,
    chunk_start: DateTime<Utc>,
}

impl ChunkAccumulator {
    #[must_use]
    pub fn new(chunk_duration_secs: u64, overlap_secs: u64) -> Self {
        let chunk_samples = (chunk_duration_secs as usize) * (SAMPLE_RATE as usize);
        let overlap_samples = (overlap_secs as usize) * (SAMPLE_RATE as usize);

        Self {
            buffer: Vec::with_capacity(chunk_samples),
            chunk_samples,
            overlap_samples,
            chunk_start: Utc::now(),
        }
    }

    /// Feed samples and return any complete chunks.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<AudioChunk> {
        if self.buffer.is_empty() {
            self.chunk_start = Utc::now();
        }

        self.buffer.extend_from_slice(samples);

        let mut chunks = Vec::new();
        while self.buffer.len() >= self.chunk_samples {
            let chunk_data: Vec<f32> = self.buffer[..self.chunk_samples].to_vec();
            let chunk = AudioChunk::from_samples(&chunk_data, self.chunk_start);
            chunks.push(chunk);

            // Keep overlap_samples for the next chunk
            let drain_count = self.chunk_samples - self.overlap_samples;
            self.buffer.drain(..drain_count);
            self.chunk_start = Utc::now();
        }

        chunks
    }

    /// Flush remaining samples as a final chunk (for graceful shutdown).
    pub fn flush(&mut self) -> Option<AudioChunk> {
        if self.buffer.is_empty() {
            return None;
        }

        let samples: Vec<f32> = self.buffer.drain(..).collect();
        Some(AudioChunk::from_samples(&samples, self.chunk_start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_at_correct_size() {
        let mut acc = ChunkAccumulator::new(1, 0); // 1 sec chunks, no overlap
        let samples = vec![0.0f32; SAMPLE_RATE as usize * 3]; // 3 seconds
        let chunks = acc.feed(&samples);
        assert_eq!(chunks.len(), 3);
        for chunk in &chunks {
            assert_eq!(chunk.samples_f32.len(), SAMPLE_RATE as usize);
        }
    }

    #[test]
    fn overlap_preserves_samples() {
        let mut acc = ChunkAccumulator::new(2, 1); // 2s chunks, 1s overlap
        // Feed 4 seconds (should yield 2 chunks with 1s overlap each, leaving 2s in buffer)
        let samples = vec![0.5f32; SAMPLE_RATE as usize * 4];
        let chunks = acc.feed(&samples);
        // First chunk at 2s, drains 1s, buffer has 3s. Second chunk at 2s, drains 1s, buffer has 2s.
        // Third iteration: buffer is 2s = chunk_samples, produces another chunk, drains 1s, buffer has 1s.
        assert_eq!(chunks.len(), 3);
        // Remaining in buffer should be overlap from the last chunk
        assert_eq!(acc.buffer.len(), SAMPLE_RATE as usize); // 1s of overlap
    }

    #[test]
    fn flush_returns_remainder() {
        let mut acc = ChunkAccumulator::new(2, 0);
        let samples = vec![0.1f32; SAMPLE_RATE as usize]; // 1 second (less than chunk)
        let chunks = acc.feed(&samples);
        assert!(chunks.is_empty());

        let flushed = acc.flush();
        assert!(flushed.is_some());
        let flushed = flushed.unwrap_or_else(|| panic!("expected a chunk"));
        assert_eq!(flushed.samples_f32.len(), SAMPLE_RATE as usize);
    }

    #[test]
    fn flush_empty_returns_none() {
        let mut acc = ChunkAccumulator::new(1, 0);
        assert!(acc.flush().is_none());
    }

    #[test]
    fn i16_conversion_clamps() {
        let chunk = AudioChunk::from_samples(&[1.5, -1.5, 0.0, 0.5], Utc::now());
        assert_eq!(chunk.samples_i16[0], i16::MAX);
        assert_eq!(chunk.samples_i16[1], -i16::MAX); // -1.0 * MAX
        assert_eq!(chunk.samples_i16[2], 0);
    }
}
