use crate::fingerprinting::ffmpeg_wrapper::{decode_with_ffmpeg, decode_with_ffmpeg_from_bytes};
use crate::fingerprinting::hanning::HANNING_WINDOW_2048_MULTIPLIERS;
use crate::fingerprinting::signature_format::{DecodedSignature, FrequencyBand, FrequencyPeak};
use chfft::RFft1D;
use rodio::source::{SeekError, UniformSourceIterator};
use rodio::{ChannelCount, Sample, SampleRate, Source};
use std::collections::HashMap;
use std::error::Error;
use std::io::{BufReader, Cursor};
use std::time::Duration;

// The fingerprint is defined over mono 16 kHz PCM; every input is resampled to
// it before anything else happens.
const TARGET_CHANNELS: ChannelCount = ChannelCount::new(1).unwrap();
const TARGET_SAMPLE_RATE: SampleRate = SampleRate::new(16000).unwrap();

// `rodio` handed out `i16` samples until 0.20 and hands out `f32` in
// [-1.0, 1.0] from 0.21 on, so the scale has to be put back. This is the
// `f32` -> `i16` conversion of `symphonia`, the decoder behind every format
// read here, so the samples reaching the fingerprint are the ones it was
// defined over.
// https://github.com/pdeljanov/Symphonia/blob/v0.5.5/symphonia-core/src/conv.rs#L606
fn to_i16(sample: Sample) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32_768.0) as i16
}

// `UniformSourceIterator` takes the length of the span it wraps from
// `current_span_len()`, so `Some(0)` wraps a `Take` of zero samples: it yields
// nothing, re-bootstraps once, reads zero again because the source never
// advanced, and ends. `None` is the only answer that makes the wrap unbounded.
// https://github.com/RustAudio/rodio/blob/v0.22.2/src/source/uniform.rs#L49-L67
//
// `symphonia`'s Vorbis decoder answers `Some(0)` for the first packet, so every
// `.ogg` file resampled to an empty signature. Non-zero spans pass through
// untouched, which leaves every other decoder exactly as it was -- measured
// against `rodio` 0.22.2 alone, decoding then resampling to mono 16 kHz:
//
//   probe16k.ogg   span=Some(0)     raw=320128   uniform=0
//   probe16k.flac  span=Some(1152)  raw=320000   uniform=320000
//   probe.mp3      span=Some(94)    raw=1764000  uniform=320034
struct NonEmptySpans<S: Source>(S);

impl<S: Source> Iterator for NonEmptySpans<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl<S: Source> Source for NonEmptySpans<S> {
    fn current_span_len(&self) -> Option<usize> {
        match self.0.current_span_len() {
            Some(0) => None,
            span => span,
        }
    }

    fn channels(&self) -> ChannelCount {
        self.0.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.0.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.0.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.0.try_seek(pos)
    }
}

// Resample to the mono 16 kHz PCM the fingerprint is defined over.
fn to_mono_16khz(source: impl Source) -> Vec<i16> {
    let uniform = UniformSourceIterator::new(
        NonEmptySpans(source),
        TARGET_CHANNELS,
        TARGET_SAMPLE_RATE,
    );
    uniform.map(to_i16).collect()
}

pub struct SignatureGenerator {
    ring_buffer_of_samples: Vec<i16>,
    reordered_ring_buffer_of_samples: Vec<f32>,
    fft_outputs: Vec<Vec<f32>>,
    spread_fft_outputs: Vec<Vec<f32>>,
    ring_buffer_of_samples_index: usize,
    fft_outputs_index: usize,
    fft_object: RFft1D<f32>,
    spread_fft_outputs_index: usize,
    num_spread_ffts_done: u32,
    signature: DecodedSignature,
}

impl SignatureGenerator {
    pub fn make_signature_from_bytes(bytes: Vec<u8>, segment_duration_seconds: Option<u32>) -> Result<DecodedSignature, Box<dyn Error>> {
        // Create a cursor around the byte array for decoding
        let cursor = Cursor::new(bytes.clone());

        let decoder = rodio::Decoder::new(cursor).or_else(|_decoding_error| {
            // Use the original bytes vector here
            decode_with_ffmpeg_from_bytes(&bytes)
        })?;

        let raw_pcm_samples: Vec<i16> = to_mono_16khz(decoder);

        // Process the PCM samples as in make_signature_from_buffer
        let duration_seconds = segment_duration_seconds.unwrap_or(10);
        let sample_rate = 16000;
        let segment_samples = (duration_seconds * sample_rate) as usize;

        let raw_pcm_samples_slice: &[i16] = if raw_pcm_samples.len() > segment_samples {
            let middle = raw_pcm_samples.len() / 2;
            let half_segment = segment_samples / 2;
            if middle >= half_segment && middle + half_segment <= raw_pcm_samples.len() {
                &raw_pcm_samples[middle - half_segment..middle + half_segment]
            } else {
                &raw_pcm_samples[..segment_samples]
            }
        } else {
            &raw_pcm_samples[..]
        };

        // Generate signature from buffer
        let signature =
            SignatureGenerator::make_signature_from_buffer(raw_pcm_samples_slice.to_vec());

        // Return the generated signature
        Ok(signature)
    }
    pub fn make_signature_from_file(file_path: &str, segment_duration_seconds: Option<u32>) -> Result<DecodedSignature, Box<dyn Error>> {
        // Decode the .WAV, .MP3, .OGG or .FLAC file

        let mut decoder = rodio::Decoder::new(BufReader::new(std::fs::File::open(file_path)?));

        if let Err(ref _decoding_error) = decoder {
            // Try to decode with FFMpeg, if available, in case of failure with
            // Rodio (most likely due to the use of a format unsupported by
            // Rodio, such as .WMA or .MP4/.AAC)

            if let Some(new_decoder) = decode_with_ffmpeg(file_path) {
                decoder = Ok(new_decoder);
            }
        }

        // Downsample the raw PCM samples to 16 KHz, and skip to the middle of the file
        // in order to increase recognition odds. Take N (10 default) seconds of sample.
        let duration_seconds = segment_duration_seconds.unwrap_or(10);
        let sample_rate = 16000;
        let segment_samples = (duration_seconds * sample_rate) as usize;

        let raw_pcm_samples: Vec<i16> = to_mono_16khz(decoder?);
        let slice_len = raw_pcm_samples.len().min(segment_samples);
        let mut raw_pcm_samples_slice: &[i16] = &raw_pcm_samples[..slice_len];

        if raw_pcm_samples.len() > segment_samples {
            let middle = raw_pcm_samples.len() / 2;
            raw_pcm_samples_slice = &raw_pcm_samples[middle - segment_samples/2 .. middle + segment_samples/2];
        }

        let res = SignatureGenerator::make_signature_from_buffer(raw_pcm_samples_slice.to_vec());
        Ok(res)
    }

    pub fn make_signature_from_buffer(s16_mono_16khz_buffer: Vec<i16>) -> DecodedSignature {
        let mut this = SignatureGenerator {
            ring_buffer_of_samples: vec![0i16; 2048],
            ring_buffer_of_samples_index: 0,

            reordered_ring_buffer_of_samples: vec![0.0f32; 2048],

            fft_outputs: vec![vec![0.0f32; 1025]; 256],
            fft_outputs_index: 0,

            fft_object: RFft1D::new(2048),

            spread_fft_outputs: vec![vec![0.0f32; 1025]; 256],
            spread_fft_outputs_index: 0,

            num_spread_ffts_done: 0,

            signature: DecodedSignature {
                sample_rate_hz: 16000,
                number_samples: s16_mono_16khz_buffer.len() as u32,
                frequency_band_to_sound_peaks: HashMap::new(),
            },
        };
        for chunk in s16_mono_16khz_buffer.chunks_exact(128) {
            this.do_fft(chunk);

            this.do_peak_spreading();

            this.num_spread_ffts_done += 1;

            if this.num_spread_ffts_done >= 46 {
                this.do_peak_recognition();
            }
        }

        this.signature
    }

    fn do_fft(&mut self, s16_mono_16khz_buffer: &[i16]) {
        // Copy the 128 input s16le samples to the local ring buffer

        self.ring_buffer_of_samples
            [self.ring_buffer_of_samples_index..self.ring_buffer_of_samples_index + 128]
            .copy_from_slice(s16_mono_16khz_buffer);

        self.ring_buffer_of_samples_index += 128;
        self.ring_buffer_of_samples_index &= 2047;

        // Reorder the items (put the latest data at end) and apply Hanning window

        for (index, multiplier) in HANNING_WINDOW_2048_MULTIPLIERS.iter().enumerate() {
            self.reordered_ring_buffer_of_samples[index] = self.ring_buffer_of_samples
                [(index + self.ring_buffer_of_samples_index) & 2047]
                as f32
                * multiplier;
        }

        // Perform Fast Fourier transform
        let reordered_slice: &[f32] = self.reordered_ring_buffer_of_samples.as_ref();

        let complex_fft_results = self.fft_object.forward(reordered_slice);

        assert_eq!(complex_fft_results.len(), 1025);

        // Turn complex into reals, and put the results into a local array

        let real_fft_results = &mut self.fft_outputs[self.fft_outputs_index];

        for index in 0..=1024 {
            real_fft_results[index] = ((complex_fft_results[index].re.powi(2)
                + complex_fft_results[index].im.powi(2))
                / ((1 << 17) as f32))
                .max(0.0000000001);
        }

        self.fft_outputs_index += 1;
        self.fft_outputs_index &= 255;
    }

    fn do_peak_spreading(&mut self) {
        let real_fft_results =
            &self.fft_outputs[((self.fft_outputs_index as i32 - 1) & 255) as usize];

        let spread_fft_results = &mut self.spread_fft_outputs[self.spread_fft_outputs_index];

        // Perform frequency-domain spreading of peak values
        spread_fft_results.copy_from_slice(real_fft_results);

        for position in 0..=1022 {
            spread_fft_results[position] = spread_fft_results[position]
                .max(spread_fft_results[position + 1])
                .max(spread_fft_results[position + 2]);
        }

        let spread_fft_results_copy = spread_fft_results.clone();

        for position in 0..=1024 {
            for former_fft_number in &[1, 3, 6] {
                let former_fft_output = &mut self.spread_fft_outputs
                    [((self.spread_fft_outputs_index as i32 - *former_fft_number) & 255) as usize];

                former_fft_output[position] =
                    former_fft_output[position].max(spread_fft_results_copy[position]);
            }
        }

        self.spread_fft_outputs_index += 1;
        self.spread_fft_outputs_index &= 255;
    }

    fn do_peak_recognition(&mut self) {
        // Note: when substracting an array index, casting to signed is needed
        // to avoid underflow panics at runtime.

        let fft_minus_46 = &self.fft_outputs[((self.fft_outputs_index as i32 - 46) & 255) as usize];
        let fft_minus_49 =
            &self.spread_fft_outputs[((self.spread_fft_outputs_index as i32 - 49) & 255) as usize];

        for bin_position in 10..=1014 {
            // Ensure that the bin is large enough to be a peak

            if fft_minus_46[bin_position] >= 1.0 / 64.0
                && fft_minus_46[bin_position] >= fft_minus_49[bin_position - 1]
            {
                // Ensure that it is frequency-domain local minimum

                let mut max_neighbor_in_fft_minus_49: f32 = 0.0;

                for neighbor_offset in &[-10, -7, -4, -3, 1, 2, 5, 8] {
                    max_neighbor_in_fft_minus_49 = max_neighbor_in_fft_minus_49
                        .max(fft_minus_49[(bin_position as i32 + *neighbor_offset) as usize]);
                }

                if fft_minus_46[bin_position] > max_neighbor_in_fft_minus_49 {
                    // Ensure that it is a time-domain local minimum

                    let mut max_neighbor_in_other_adjacent_ffts = max_neighbor_in_fft_minus_49;

                    for other_offset in &[
                        -53, -45, 165, 172, 179, 186, 193, 200, 214, 221, 228, 235, 242, 249,
                    ] {
                        let other_fft = &self.spread_fft_outputs[((self.spread_fft_outputs_index
                            as i32
                            + other_offset)
                            & 255)
                            as usize];

                        max_neighbor_in_other_adjacent_ffts =
                            max_neighbor_in_other_adjacent_ffts.max(other_fft[bin_position - 1]);
                    }

                    if fft_minus_46[bin_position] > max_neighbor_in_other_adjacent_ffts {
                        // This is a peak, store the peak

                        let fft_pass_number = self.num_spread_ffts_done - 46;

                        let peak_magnitude: f32 =
                            fft_minus_46[bin_position].ln().max(1.0 / 64.0) * 1477.3 + 6144.0;
                        let peak_magnitude_before: f32 =
                            fft_minus_46[bin_position - 1].ln().max(1.0 / 64.0) * 1477.3 + 6144.0;
                        let peak_magnitude_after: f32 =
                            fft_minus_46[bin_position + 1].ln().max(1.0 / 64.0) * 1477.3 + 6144.0;

                        let peak_variation_1: f32 =
                            peak_magnitude * 2.0 - peak_magnitude_before - peak_magnitude_after;
                        let peak_variation_2: f32 = (peak_magnitude_after - peak_magnitude_before)
                            * 32.0
                            / peak_variation_1;

                        let corrected_peak_frequency_bin: u16 = (
                            (bin_position as i32 * 64) + (peak_variation_2 as i32)
                        ) as u16;

                        assert!(peak_variation_1 >= 0.0);

                        // Convert back a FFT bin to a frequency, given a 16 KHz sample
                        // rate, 1024 useful bins and the multiplication by 64 made before
                        // storing the information

                        let frequency_hz: f32 =
                            corrected_peak_frequency_bin as f32 * (16000.0 / 2.0 / 1024.0 / 64.0);

                        // Ignore peaks outside the 250 Hz-5.5 KHz range, store them into
                        // a lookup table that will be used to generate the binary fingerprint
                        // otherwise

                        let frequency_band = match frequency_hz as i32 {
                            250..=519 => FrequencyBand::_250_520,
                            520..=1449 => FrequencyBand::_520_1450,
                            1450..=3499 => FrequencyBand::_1450_3500,
                            3500..=5500 => FrequencyBand::_3500_5500,
                            _ => {
                                continue;
                            }
                        };

                        self.signature.frequency_band_to_sound_peaks
                            .entry(frequency_band)
                            .or_default();

                        self.signature
                            .frequency_band_to_sound_peaks
                            .get_mut(&frequency_band)
                            .unwrap()
                            .push(FrequencyPeak {
                                fft_pass_number,
                                peak_magnitude: peak_magnitude as u16,
                                corrected_peak_frequency_bin,
                            });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The probe and its pinned URI are the ones `tests/` uses; `tests/data/generate.sh`
    //  regenerates the audio. Reading them here rather than restating the expected
    //  bytes keeps one copy of the golden.
    const DATA_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");

    fn probe_path() -> String {
        format!("{DATA_DIRECTORY}/probe.flac")
    }

    fn golden_uri() -> String {
        std::fs::read_to_string(format!("{DATA_DIRECTORY}/probe.flac.uri"))
            .unwrap()
            .trim()
            .to_string()
    }

    // Yields nothing: every test here asks about `current_span_len`, which the
    //  resampler reads before it takes a single sample.
    struct FixedSpan {
        span_length: Option<usize>,
    }

    impl Iterator for FixedSpan {
        type Item = Sample;

        fn next(&mut self) -> Option<Self::Item> {
            None
        }
    }

    impl Source for FixedSpan {
        fn current_span_len(&self) -> Option<usize> {
            self.span_length
        }

        fn channels(&self) -> ChannelCount {
            TARGET_CHANNELS
        }

        fn sample_rate(&self) -> SampleRate {
            TARGET_SAMPLE_RATE
        }

        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    #[test]
    fn a_zero_length_span_is_reported_as_unbounded() {
        let mut source = NonEmptySpans(FixedSpan {
            span_length: Some(0),
        });

        assert_eq!(source.current_span_len(), None);
        assert_eq!(source.next(), None);
    }

    #[test]
    fn every_other_span_passes_through_untouched() {
        for span_length in [Some(1), Some(94), Some(1152), None] {
            let source = NonEmptySpans(FixedSpan { span_length });

            assert_eq!(source.current_span_len(), span_length);
        }
    }

    #[test]
    fn the_wrapper_reports_the_channels_and_rate_of_what_it_wraps() {
        let source = NonEmptySpans(FixedSpan {
            span_length: Some(1),
        });

        assert_eq!(source.channels(), TARGET_CHANNELS);
        assert_eq!(source.sample_rate(), TARGET_SAMPLE_RATE);
        assert_eq!(source.total_duration(), None);
    }

    #[test]
    fn the_whole_pipeline_reproduces_the_golden_uri() {
        let signature = SignatureGenerator::make_signature_from_file(&probe_path(), None).unwrap();

        // The probe is 8 s, so the default 10 s segment takes all of it and the peaks
        //  below are every peak the file has.
        assert_eq!(signature.number_samples, 128013);
        assert_eq!(signature.encode_to_uri().unwrap(), golden_uri());

        // Peaks landed in every band, so `do_peak_recognition` ran its whole match.
        let mut bands: Vec<_> = signature.frequency_band_to_sound_peaks.keys().collect();
        bands.sort();
        assert_eq!(
            bands,
            vec![
                &FrequencyBand::_250_520,
                &FrequencyBand::_520_1450,
                &FrequencyBand::_1450_3500,
                &FrequencyBand::_3500_5500,
            ],
        );
    }

    #[test]
    fn a_segment_shorter_than_the_file_is_cut_from_the_middle() {
        let from_file =
            SignatureGenerator::make_signature_from_file(&probe_path(), Some(4)).unwrap();
        let from_bytes = SignatureGenerator::make_signature_from_bytes(
            std::fs::read(probe_path()).unwrap(),
            Some(4),
        )
        .unwrap();

        assert_eq!(from_file.number_samples, 4 * 16000);

        // Both entry points cut the same window, so they fingerprint the same audio.
        assert_eq!(
            from_bytes.encode_to_uri().unwrap(),
            from_file.encode_to_uri().unwrap(),
        );
    }

    #[test]
    fn the_bytes_of_a_file_fingerprint_the_same_as_its_path() {
        let probe = std::fs::read(probe_path()).unwrap();

        let from_bytes = SignatureGenerator::make_signature_from_bytes(probe, None).unwrap();

        assert_eq!(from_bytes.encode_to_uri().unwrap(), golden_uri());
    }

    // Both entry points fall back to `ffmpeg` when `rodio` cannot read the input, and
    //  `ffmpeg` cannot read this either -- present or not, the answer is an error and
    //  never a panic or an empty signature.
    #[test]
    fn input_no_decoder_understands_is_an_error() {
        let not_audio = SignatureGenerator::make_signature_from_bytes(b"not audio".to_vec(), None);
        let script = format!("{DATA_DIRECTORY}/generate.sh");
        let not_a_sound_file = SignatureGenerator::make_signature_from_file(&script, None);

        assert!(not_audio.is_err());
        assert!(not_a_sound_file.is_err());
    }

    #[test]
    fn a_seek_is_handed_to_the_wrapped_source() {
        let mut source = NonEmptySpans(FixedSpan {
            span_length: Some(1),
        });

        // `FixedSpan` does not override `try_seek`, so this is `Source`'s own refusal
        //  arriving through the wrapper rather than being answered by it.
        assert!(source.try_seek(Duration::ZERO).is_err());
    }

    #[test]
    fn samples_are_scaled_to_i16_and_saturate_instead_of_wrapping() {
        assert_eq!(to_i16(0.0), 0);
        assert_eq!(to_i16(-1.0), -32768);

        // `1.0 * 32_768.0` is one past `i16::MAX`; a float-to-int cast saturates, so
        //  full scale comes out as the largest sample rather than the smallest.
        assert_eq!(to_i16(1.0), 32767);

        // Anything beyond full scale is clamped first, so it lands on the same value.
        assert_eq!(to_i16(4.0), 32767);
        assert_eq!(to_i16(-4.0), -32768);
    }
}
