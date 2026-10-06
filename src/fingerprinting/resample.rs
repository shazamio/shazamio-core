use crate::fingerprinting::algorithm::SAMPLE_RATE_HZ;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters};
use std::error::Error;

// The ratio is fixed for a whole clip, so the resampler is never asked to adjust it.
const MAX_RATIO_RELATIVE: f64 = 1.0;

// Input frames the resampler takes per call.
const CHUNK_FRAMES: usize = 1024;

// The stream is mixed down to mono before it is resampled.
const CHANNEL_COUNT: usize = 1;

/// Resamples a mono stream to 16 kHz as it arrives, holding only the 16 kHz output.
///
/// This is `rubato`'s own `process_all_into_buffer` unrolled over a stream: the chunking,
/// the delay trim and the final flush, so the output is the one a single whole-clip call
/// produces. The tests below hold the two equal.
/// https://github.com/HEnquist/rubato/blob/eb9190855133fd40d54b17f8d17065ef98ab4db0/src/lib.rs#L323-L395
pub struct Resampling {
    resampler: Async<f32>,
    staged_frames: Vec<f32>,
    chunk_output: Vec<f32>,
    output: Vec<i16>,
    input_frame_count: usize,
    frames_to_trim: usize,
}

// The defaults keep the filter this used to build by hand and differ in two fields: the
//  cutoff follows `sinc_len` and the window instead of being pinned at 0.95, and
//  `oversampling_factor` is 128 rather than 160. Neither old number was explained.
fn sinc_resampler(source_rate: u32) -> Result<Async<f32>, Box<dyn Error>> {
    Ok(Async::<f32>::new_sinc(
        f64::from(SAMPLE_RATE_HZ) / f64::from(source_rate),
        MAX_RATIO_RELATIVE,
        &SincInterpolationParameters::default(),
        CHUNK_FRAMES,
        CHANNEL_COUNT,
        FixedAsync::Input,
    )?)
}

impl Resampling {
    pub fn new(source_rate: u32) -> Result<Self, Box<dyn Error>> {
        let resampler = sinc_resampler(source_rate)?;
        let chunk_output = vec![0f32; resampler.output_frames_max()];
        let frames_to_trim = resampler.output_delay();

        Ok(Resampling {
            resampler,
            staged_frames: Vec::new(),
            chunk_output,
            output: Vec::new(),
            input_frame_count: 0,
            frames_to_trim,
        })
    }

    /// Takes the next frames of the stream, resampling every whole chunk they complete.
    pub fn push(&mut self, mono_frames: &[f32]) -> Result<(), Box<dyn Error>> {
        self.staged_frames.extend_from_slice(mono_frames);
        self.input_frame_count += mono_frames.len();

        // Strictly greater, because `rubato` takes a full chunk only while more than one
        //  is left and hands whatever remains to the partial call `finish` makes.
        while self.staged_frames.len() > CHUNK_FRAMES {
            let consumed_frames = self.resample_chunk(None)?;

            self.staged_frames.drain(..consumed_frames);
        }

        Ok(())
    }

    /// Resamples what is staged, flushes the filter, and returns the whole 16 kHz stream.
    pub fn finish(mut self) -> Result<Vec<i16>, Box<dyn Error>> {
        // The clip is as long as the ratio says, and the flush below pumps silence until
        //  the output reaches that length.
        let output_frame_count =
            (self.resampler.resample_ratio() * self.input_frame_count as f64).ceil() as usize;

        if !self.staged_frames.is_empty() {
            self.resample_chunk(Some(self.staged_frames.len()))?;
            self.staged_frames.clear();
        }

        while self.output.len() < output_frame_count {
            self.resample_chunk(Some(0))?;
        }

        self.output.truncate(output_frame_count);

        Ok(self.output)
    }

    // One call into the resampler over the staged frames, with what it produced appended
    //  to the output. Reports how many input frames it took.
    fn resample_chunk(&mut self, partial_frames: Option<usize>) -> Result<usize, Box<dyn Error>> {
        let indexing = Indexing {
            input_offset: 0,
            output_offset: 0,
            partial_len: partial_frames,
            active_channels_mask: None,
        };

        let staged_frame_count = self.staged_frames.len();
        let chunk_frame_count = self.chunk_output.len();

        let input = InterleavedSlice::new(&self.staged_frames, CHANNEL_COUNT, staged_frame_count)?;
        let mut output =
            InterleavedSlice::new_mut(&mut self.chunk_output, CHANNEL_COUNT, chunk_frame_count)?;

        let (consumed_frames, produced_frames) =
            self.resampler
                .process_into_buffer(&input, &mut output, Some(&indexing))?;

        self.output.extend(
            self.chunk_output[..produced_frames]
                .iter()
                .map(|&frame| (frame * i16::MAX as f32) as i16),
        );
        self.trim_startup_delay();

        Ok(consumed_frames)
    }

    // The sinc filter answers with silence until it has seen enough input, and `rubato`
    //  drops that silence from the front of the output.
    fn trim_startup_delay(&mut self) {
        let trimmed_frames = self.frames_to_trim.min(self.output.len());

        self.output.drain(..trimmed_frames);
        self.frames_to_trim -= trimmed_frames;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // What `rubato` produces for the whole clip in one call, which the stream claims to
    //  reproduce.
    fn resample_whole_clip(source_rate: u32, frames: &[f32]) -> Vec<i16> {
        let mut resampler = sinc_resampler(source_rate).unwrap();
        let mut output = vec![0f32; resampler.process_all_needed_output_len(frames.len())];

        let input = InterleavedSlice::new(frames, CHANNEL_COUNT, frames.len()).unwrap();
        let output_frame_count = output.len();
        let mut output_buffer =
            InterleavedSlice::new_mut(&mut output, CHANNEL_COUNT, output_frame_count).unwrap();

        let (_, produced_frames) = resampler
            .process_all_into_buffer(&input, &mut output_buffer, frames.len(), None)
            .unwrap();

        output[..produced_frames]
            .iter()
            .map(|&frame| (frame * i16::MAX as f32) as i16)
            .collect()
    }

    fn resample_stream(source_rate: u32, frames: &[f32], piece_frames: usize) -> Vec<i16> {
        let mut resampling = Resampling::new(source_rate).unwrap();

        for piece in frames.chunks(piece_frames) {
            resampling.push(piece).unwrap();
        }

        resampling.finish().unwrap()
    }

    fn sweep(source_rate: u32, frame_count: usize) -> Vec<f32> {
        (0..frame_count)
            .map(|index| {
                let time = index as f32 / source_rate as f32;
                0.5 * (2.0 * std::f32::consts::PI * (300.0 + 900.0 * time) * time).sin()
            })
            .collect()
    }

    // Pieces of 777 frames, so chunk boundaries never line up with what the decoder
    //  hands over. Before the delay trim followed `rubato` 5.0.1, the first 46 output
    //  frames at 44.1 kHz were duplicated and everything after them came 46 frames late.
    //  A whole number of chunks leaves a full one for `finish`, whose output falls short
    //  of the clip, so only that length reaches the flush.
    #[test]
    fn the_stream_resamples_as_the_whole_clip_does() {
        for source_rate in [44_100, 48_000] {
            for frame_count in [3 * source_rate as usize, 128 * CHUNK_FRAMES] {
                let frames = sweep(source_rate, frame_count);

                assert_eq!(
                    resample_stream(source_rate, &frames, 777),
                    resample_whole_clip(source_rate, &frames),
                    "{frame_count} frames at {source_rate} Hz"
                );
            }
        }
    }

    // Shorter than one chunk, the clip never reached the trim in `push`, so it began with
    //  the filter's leading silence and lost as many frames off its end.
    #[test]
    fn a_clip_shorter_than_one_chunk_resamples_as_the_whole_clip_does() {
        for frame_count in [1, 64, CHUNK_FRAMES / 2] {
            let frames = sweep(44_100, frame_count);

            assert_eq!(
                resample_stream(44_100, &frames, frame_count),
                resample_whole_clip(44_100, &frames),
                "{frame_count} frames"
            );
        }
    }
}
