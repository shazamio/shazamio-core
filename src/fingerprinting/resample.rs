use crate::fingerprinting::decode::DecodedAudio;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters};
use std::error::Error;

/// The sample rate the fingerprinting algorithm is defined against.
const TARGET_RATE: u32 = 16_000;

// The ratio is fixed for a whole clip, so the resampler is never asked to adjust it.
const MAX_RATIO_RELATIVE: f64 = 1.0;

// Input frames per internal call. `process_all` drives the loop, so this only sizes the
//  resampler's own buffers.
const CHUNK_FRAMES: usize = 1024;

pub fn resample(decoded_audio: DecodedAudio) -> Result<Vec<i16>, Box<dyn Error>> {
    let channel_count = decoded_audio.spec.channels.count();
    let frame_count = decoded_audio.samples.len() / channel_count;

    let mut mono_samples = vec![0f32; frame_count];
    for (index, sample) in decoded_audio.samples.iter().enumerate() {
        mono_samples[index / channel_count] += sample / channel_count as f32;
    }

    // The defaults keep the filter this used to build by hand and differ in two fields:
    //  the cutoff follows `sinc_len` and the window instead of being pinned at 0.95, and
    //  `oversampling_factor` is 128 rather than 160. Neither old number was explained.
    let mut resampler = Async::<f32>::new_sinc(
        f64::from(TARGET_RATE) / f64::from(decoded_audio.spec.rate),
        MAX_RATIO_RELATIVE,
        &SincInterpolationParameters::default(),
        CHUNK_FRAMES,
        1,
        FixedAsync::Input,
    )?;

    // `process_all` trims the sinc filter's startup delay and the trailing padding, which a
    //  single `process` call leaves in: the clip would come out shifted by `output_delay`
    //  frames of silence and cut short at the end.
    let input = InterleavedSlice::new(&mono_samples, 1, frame_count)?;
    let resampled = resampler.process_all(&input, frame_count, None)?;

    Ok(resampled
        .take_data()
        .iter()
        .map(|&sample| (sample * i16::MAX as f32) as i16)
        .collect())
}
