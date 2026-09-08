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
/// the delay trim and the final flush are the steps it takes, in its order, so the output
/// is the one a single whole-clip call produces.
/// https://github.com/HEnquist/rubato/blob/6b72d0f9d8843c6623c818751730764aefcd0525/src/lib.rs#L323
pub struct Resampling {
    resampler: Async<f32>,
    staged_frames: Vec<f32>,
    chunk_output: Vec<f32>,
    output: Vec<i16>,
    input_frame_count: usize,
    frames_to_trim: usize,
}

impl Resampling {
    pub fn new(source_rate: u32) -> Result<Self, Box<dyn Error>> {
        // The defaults keep the filter this used to build by hand and differ in two fields:
        //  the cutoff follows `sinc_len` and the window instead of being pinned at 0.95, and
        //  `oversampling_factor` is 128 rather than 160. Neither old number was explained.
        let resampler = Async::<f32>::new_sinc(
            f64::from(SAMPLE_RATE_HZ) / f64::from(source_rate),
            MAX_RATIO_RELATIVE,
            &SincInterpolationParameters::default(),
            CHUNK_FRAMES,
            CHANNEL_COUNT,
            FixedAsync::Input,
        )?;

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
            self.trim_startup_delay();
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

        Ok(consumed_frames)
    }

    // The sinc filter answers with silence until it has seen enough input, and `rubato`
    //  drops that silence from the front of the output once the output has outgrown it.
    fn trim_startup_delay(&mut self) {
        if self.frames_to_trim == 0 || self.output.len() <= self.frames_to_trim {
            return;
        }

        // It moves `frames_to_trim` frames rather than all it holds, so those frames
        //  arrive twice and nothing after them shifts. Trimmed properly, the first 46
        //  frames of `probe.flac` changed and the golden URI no longer matched.
        //  https://github.com/HEnquist/rubato/blob/6b72d0f9d8843c6623c818751730764aefcd0525/src/lib.rs#L363
        let duplicated_frames = self.frames_to_trim..2 * self.frames_to_trim;

        self.output.copy_within(duplicated_frames, 0);
        self.output
            .truncate(self.output.len() - self.frames_to_trim);
        self.frames_to_trim = 0;
    }
}
