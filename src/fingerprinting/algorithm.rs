use crate::fingerprinting::decode::MonoDecoder;
use crate::fingerprinting::hanning::HANNING_WINDOW_2048_MULTIPLIERS;
use crate::fingerprinting::resample::Resampling;
use crate::fingerprinting::signature_format::{DecodedSignature, FrequencyBand, FrequencyPeak};
use chfft::RFft1D;
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

// The rate every entry point resamples to before fingerprinting, and the rate the
//  signature declares.
pub(crate) const SAMPLE_RATE_HZ: u32 = 16000;

// What SongRec takes from the middle of a file, the reference this crate follows.
//  https://github.com/marin-m/SongRec/blob/eaa3ea3e1ce4da90e71881f9853564937db518a7/src/core/fingerprinting/algorithm.rs#L101-L107
pub(crate) const DEFAULT_SEGMENT_SECONDS: u32 = 12;

const FRAME_LENGTH: usize = 2048;
const HOP_LENGTH: usize = 128;
const BIN_COUNT: usize = FRAME_LENGTH / 2 + 1;

// Frames are kept by their index modulo this, and every lag below stays under it.
const HISTORY_LENGTH: usize = 256;

// A frame is judged this many hops after it was computed, once every later frame its
//  neighbourhood needs exists; its neighbours are read around an older reference frame.
const PEAK_LAG: usize = 45;
const REFERENCE_LAG: usize = 48;

// The neighbourhood a peak must beat. A spread value at `bin - 1` already covers bins
//  `bin - 1` to `bin + 1`, so the reference bin is the one below the candidate.
const FREQUENCY_NEIGHBOUR_DISTANCES: [usize; 4] = [2, 3, 6, 9];
const TIME_NEIGHBOUR_DISTANCES: [usize; 7] = [4, 7, 14, 21, 28, 35, 42];

// How many hops back a new frame's spread values are carried.
const SPREAD_BACK_LAGS: [usize; 3] = [1, 3, 6];

const FIRST_PEAK_BIN: usize = 10;
const LAST_PEAK_BIN: usize = 1014;
const MIN_PEAK_POWER: f32 = 1.0 / 64.0;

// `refined_bin` counts in 1/64 of a bin, and one bin is 8000 / 1024 Hz, so a
//  refined bin is 125 / 1024 Hz.
const REFINED_BIN_HZ_NUMERATOR: u32 = 125;
const REFINED_BIN_HZ_DENOMINATOR: u32 = 1024;

#[derive(Clone)]
struct Frame {
    power: [f32; BIN_COUNT],
    spread: [f32; BIN_COUNT],
}

// The slots are zeroed up front and a lag may reach one not written yet, which reads
//  as silence.
struct History {
    frames: Vec<Frame>,
    count: usize,
}

impl History {
    fn slot(&self, lag: usize) -> usize {
        (self.count + HISTORY_LENGTH - 1 - lag) % HISTORY_LENGTH
    }

    fn ago(&self, lag: usize) -> &Frame {
        &self.frames[self.slot(lag)]
    }

    fn ago_mut(&mut self, lag: usize) -> &mut Frame {
        let slot = self.slot(lag);

        &mut self.frames[slot]
    }
}

struct Spectrogram {
    fft: RFft1D<f32>,
    samples: [i16; FRAME_LENGTH],
    history: History,
    peaks: HashMap<FrequencyBand, Vec<FrequencyPeak>>,
}

impl Spectrogram {
    fn new() -> Self {
        let silence = Frame {
            power: [0.0; BIN_COUNT],
            spread: [0.0; BIN_COUNT],
        };

        Spectrogram {
            fft: RFft1D::new(FRAME_LENGTH),
            samples: [0; FRAME_LENGTH],
            history: History {
                frames: vec![silence; HISTORY_LENGTH],
                count: 0,
            },
            peaks: HashMap::new(),
        }
    }

    fn push_hop(&mut self, hop: &[i16]) {
        self.samples.copy_within(HOP_LENGTH.., 0);
        self.samples[FRAME_LENGTH - HOP_LENGTH..].copy_from_slice(hop);

        let power = self.power_spectrum();

        let mut spread = power;
        for (bin, triple) in power.windows(3).enumerate() {
            spread[bin] = triple[0].max(triple[1]).max(triple[2]);
        }

        self.history.count += 1;

        for lag in SPREAD_BACK_LAGS {
            let earlier = &mut self.history.ago_mut(lag).spread;

            for (earlier_value, value) in earlier.iter_mut().zip(&spread) {
                *earlier_value = earlier_value.max(*value);
            }
        }

        *self.history.ago_mut(0) = Frame { power, spread };

        if self.history.count > PEAK_LAG {
            pick_peaks(&self.history, &mut self.peaks);
        }
    }

    fn power_spectrum(&mut self) -> [f32; BIN_COUNT] {
        let windowed: Vec<f32> = self
            .samples
            .iter()
            .zip(HANNING_WINDOW_2048_MULTIPLIERS)
            .map(|(&sample, weight)| sample as f32 * weight)
            .collect();

        let mut power = [0.0; BIN_COUNT];
        for (value, coefficient) in power.iter_mut().zip(self.fft.forward(&windowed)) {
            *value = (coefficient.norm_sqr() / 131072.0).max(1e-10);
        }

        power
    }
}

fn pick_peaks(history: &History, peaks: &mut HashMap<FrequencyBand, Vec<FrequencyPeak>>) {
    let frame_index = (history.count - 1 - PEAK_LAG) as u32;
    let candidate = &history.ago(PEAK_LAG).power;
    let reference = &history.ago(REFERENCE_LAG).spread;

    let time_neighbours: Vec<&[f32; BIN_COUNT]> = TIME_NEIGHBOUR_DISTANCES
        .iter()
        .flat_map(|distance| [REFERENCE_LAG - distance, REFERENCE_LAG + distance])
        .map(|lag| &history.ago(lag).spread)
        .collect();

    for bin in FIRST_PEAK_BIN..=LAST_PEAK_BIN {
        let power = candidate[bin];
        let below = bin - 1;

        if power < MIN_PEAK_POWER || power < reference[below] {
            continue;
        }

        let beats_frequency_neighbours = FREQUENCY_NEIGHBOUR_DISTANCES.iter().all(|distance| {
            power > reference[below - distance] && power > reference[below + distance]
        });
        let beats_time_neighbours = time_neighbours.iter().all(|frame| power > frame[below]);

        if !beats_frequency_neighbours || !beats_time_neighbours {
            continue;
        }

        let magnitude = log_magnitude(power);
        let refined_bin = refine_bin(
            bin,
            log_magnitude(candidate[bin - 1]),
            magnitude,
            log_magnitude(candidate[bin + 1]),
        );
        let frequency_hz =
            u32::from(refined_bin) * REFINED_BIN_HZ_NUMERATOR / REFINED_BIN_HZ_DENOMINATOR;

        let Some(band) = FrequencyBand::containing(frequency_hz) else {
            continue;
        };

        peaks.entry(band).or_default().push(FrequencyPeak {
            fft_pass_number: frame_index,
            peak_magnitude: magnitude as u16,
            corrected_peak_frequency_bin: refined_bin,
        });
    }
}

// The scale the signature format stores a peak's magnitude in.
fn log_magnitude(power: f32) -> f32 {
    power.ln().max(1.0 / 64.0) * 1477.3 + 6144.0
}

// Fits a parabola through the peak and its two neighbours and returns its vertex, in
//  1/64 of a bin.
fn refine_bin(bin: usize, below: f32, peak: f32, above: f32) -> u16 {
    let curvature = peak * 2.0 - below - above;
    let vertex_offset = (above - below) * 32.0 / curvature;

    (bin as i32 * 64 + vertex_offset as i32) as u16
}

// Decoded, mixed down and resampled a packet at a time, so nothing longer than one
//  packet of the source is ever held beside the 16 kHz result.
fn pcm_samples(mut decoder: MonoDecoder) -> Result<Vec<i16>, Box<dyn Error>> {
    let mut resampling = Resampling::new(decoder.spec().rate())?;
    let mut mono_frames = Vec::new();

    while decoder.next_chunk(&mut mono_frames)? {
        resampling.push(&mono_frames)?;
    }

    resampling.finish()
}

// A file no longer than the requested segment is fingerprinted whole; a longer one
//  is cut from its middle, where recognition odds are best.
fn middle_segment(samples: &[i16], segment_duration_seconds: Option<u32>) -> &[i16] {
    let duration_seconds = segment_duration_seconds.unwrap_or(DEFAULT_SEGMENT_SECONDS);

    // The product leaves `u32` at 268436 seconds, and `usize` is 32 bits wide on
    //  the `i686` wheel, so both are too narrow for a duration callers may pass:
    //  268436 selected 544 ms of an 8 s file instead of the whole of it.
    let segment_samples =
        (duration_seconds as u64 * SAMPLE_RATE_HZ as u64).min(usize::MAX as u64) as usize;

    if samples.len() <= segment_samples {
        return samples;
    }

    let middle = samples.len() / 2;
    let half_segment = segment_samples / 2;

    &samples[middle - half_segment..middle + half_segment]
}

pub fn fingerprint_bytes(
    bytes: Vec<u8>,
    segment_duration_seconds: Option<u32>,
) -> Result<DecodedSignature, Box<dyn Error>> {
    fingerprint_decoded(MonoDecoder::from_bytes(bytes)?, segment_duration_seconds)
}

pub fn fingerprint_file(
    file_path: &Path,
    segment_duration_seconds: Option<u32>,
) -> Result<DecodedSignature, Box<dyn Error>> {
    // Read through as it decodes rather than with `fs::read`, which held the whole
    //  file: 30 minutes of stereo FLAC is 230 MB of bytes before a sample is decoded.
    fingerprint_decoded(MonoDecoder::from_file(file_path)?, segment_duration_seconds)
}

fn fingerprint_decoded(
    decoder: MonoDecoder,
    segment_duration_seconds: Option<u32>,
) -> Result<DecodedSignature, Box<dyn Error>> {
    let samples = pcm_samples(decoder)?;

    Ok(fingerprint_samples(middle_segment(
        &samples,
        segment_duration_seconds,
    )))
}

// A trailing part of a hop is dropped, as the frames advance a whole hop at a time.
pub fn fingerprint_samples(samples: &[i16]) -> DecodedSignature {
    let mut spectrogram = Spectrogram::new();

    for hop in samples.chunks_exact(HOP_LENGTH) {
        spectrogram.push_hop(hop);
    }

    DecodedSignature {
        sample_rate_hz: SAMPLE_RATE_HZ,
        number_samples: samples.len() as u32,
        frequency_band_to_sound_peaks: spectrogram.peaks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprinting::{probe_path, DATA_DIRECTORY};
    use symphonia::core::errors::Error as SymphoniaError;

    // The pinned URIs are the ones `tests/` uses. Reading them here rather than
    //  restating the expected bytes keeps one copy of each golden.
    //
    //  A golden URI holds on Linux only, so every test comparing one is ignored
    //  elsewhere. The module docstring of `tests/test_recognizer.py` has the reason
    //  and the CI runs that showed it.
    //
    //  `UPDATE_EXPECT=1` rewrites the file instead of failing; `just regenerate` sets it.
    fn assert_golden_uri(signature: &DecodedSignature, name: &str) {
        expect_test::expect_file![format!("{DATA_DIRECTORY}/{name}.uri")]
            .assert_eq(&format!("{}\n", signature.encode_to_uri().unwrap()));
    }

    struct ProbeShape {
        frames: usize,
        channels: usize,
        rate: u32,
    }

    // The same decode the entry points drive, so a test here cannot prove something they
    //  do not do. Mixing down keeps one value per frame, so the count is the frame count.
    fn decode(mut decoder: MonoDecoder) -> Result<ProbeShape, SymphoniaError> {
        let mut mono_frames = Vec::new();
        let mut frames = 0;

        while decoder.next_chunk(&mut mono_frames)? {
            frames += mono_frames.len();
        }

        Ok(ProbeShape {
            frames,
            channels: decoder.spec().channels().count(),
            rate: decoder.spec().rate(),
        })
    }

    // The same read-and-decode the path entry point does.
    fn decode_probe(name: &str) -> Result<ProbeShape, SymphoniaError> {
        decode(MonoDecoder::from_file(&probe_path(name))?)
    }

    // What the bytes entry point does instead, for a stream no file holds.
    fn decode_bytes(bytes: Vec<u8>) -> Result<ProbeShape, SymphoniaError> {
        decode(MonoDecoder::from_bytes(bytes)?)
    }

    #[test]
    fn a_probe_decodes_to_the_length_of_its_source() {
        const SOURCE_FRAMES: usize = 8 * 44100;

        for name in ["probe.flac", "probe.mp3", "probe.ogg"] {
            let probe = decode_probe(name).unwrap();

            assert_eq!(probe.frames, SOURCE_FRAMES, "{name}");
        }
    }

    #[test]
    fn an_opus_stream_decodes_to_the_length_of_its_source() {
        // Opus always decodes at 48 kHz whatever the encoder was fed, so the frame
        //  count is against that rate rather than against the source's 44.1 kHz.
        const OPUS_RATE: usize = 48_000;

        let probe = decode_probe("probe.opus").unwrap();

        assert_eq!(probe.rate as usize, OPUS_RATE);
        assert_eq!(probe.frames, 8 * OPUS_RATE);
    }

    #[test]
    fn a_chained_ogg_stream_decodes_every_link() {
        // Two Ogg streams in one file, which is what a concatenation produces. The
        //  reader stops between them and asks for a new decoder; read as the end of
        //  the file, the second link went missing and nothing reported it.
        let mut chained = std::fs::read(probe_path("probe.opus")).unwrap();
        chained.extend_from_slice(&chained.clone());

        let probe = decode_bytes(chained).unwrap();

        assert_eq!(probe.frames, 2 * 8 * 48_000);
    }

    #[test]
    fn an_opus_stream_in_matroska_decodes() {
        // Matroska describes an Opus track with no channel count, and the decoder used
        //  to refuse it with "declares no channel layout". Its 648 frames of end
        //  padding survive because the demuxer reads `DiscardPadding` and drops it.
        //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-format-mkv/src/segment.rs#L1187
        let probe = decode_probe("matroska.webm").unwrap();

        assert_eq!(probe.channels, 2);
        assert_eq!(probe.frames, 8 * 48_000 + 648);
    }

    #[test]
    fn a_track_this_build_cannot_decode_is_passed_over() {
        // AC-3 comes first, and taking the first track whose codec has a name refused
        //  the file with `core (codec): unsupported audio codec` instead of reading
        //  the FLAC track behind it.
        let probe = decode_probe("ac3_then_flac.mka").unwrap();

        assert_eq!(probe.frames, 48_000);
    }

    #[test]
    fn a_surround_opus_stream_decodes() {
        // Above two channels `OpusHead` carries a mapping table and `libopus` decodes
        //  the stream only through its multistream API. The single-stream decoder used
        //  to refuse this file with "only mono and stereo streams are supported".
        let probe = decode_probe("surround.opus").unwrap();

        assert_eq!(probe.channels, 6);
        assert_eq!(probe.frames, 8 * 48_000);
    }

    #[test]
    fn a_chained_stream_that_changes_format_is_refused() {
        // The links of a chained file need not agree on rate or channel count, and
        //  samples of two shapes cannot share one buffer. Appended regardless, these
        //  two came out as 10666 ms of 16 s of audio and reported success.
        let mut chained = std::fs::read(probe_path("probe.opus")).unwrap();
        chained.extend_from_slice(&std::fs::read(probe_path("surround.opus")).unwrap());

        let Err(error) = decode_bytes(chained) else {
            panic!("a stream that changes format decoded");
        };

        assert!(matches!(error, SymphoniaError::Unsupported(_)), "{error}");
    }

    #[test]
    fn a_link_that_decodes_into_a_wider_buffer_decodes() {
        // The sample buffer used to be sized once, so a link needing a wider one panicked
        //  in `copy_interleaved_ref`: 2304 samples into a buffer of 2048. How the fixture
        //  chains its links: `tests/data/generate.sh`.
        let probe = decode_probe("chained_capacity.ogg").unwrap();

        assert_eq!(probe.channels, 2);
        assert_eq!(probe.frames, 32_000);
    }

    #[test]
    fn an_mp4_container_decodes() {
        // 1504 frames longer than the source: AAC pads the front, and the edit list
        //  saying by how much is parsed into the track and never read again. Same gap
        //  before the decoder was swapped, so it is not a regression to fix here.
        //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-format-isomp4/src/atoms/trak.rs#L37
        let probe = decode_probe("probe.m4a").unwrap();

        assert_eq!(probe.frames, 8 * 44100 + 1504);
    }

    #[test]
    fn every_other_enabled_codec_decodes() {
        // Every source is half a second of mono at 44.1 kHz, 22050 frames, so each
        //  count below past that is the codec's own rounding, and `ffmpeg` decodes
        //  every one of these files to the same count. How they are encoded:
        //  `tests/data/generate.sh`.
        const SOURCE_FRAMES: usize = 22_050;

        for (name, frames) in [
            ("pcm_f32.wav", SOURCE_FRAMES),
            ("pcm_s16.aiff", SOURCE_FRAMES),
            ("pcm_s24.caf", SOURCE_FRAMES),
            ("alac.m4a", SOURCE_FRAMES),
            // ADPCM fills its last block: 11 of 2041 frames for IMA, 11 of 2036 for
            //  Microsoft. The `fact` chunk saying 22050 is ignored.
            //  https://github.com/FFmpeg/FFmpeg/blob/894da5ca7d742e4429ffb2af534fcda0103ef593/libavcodec/adpcmenc.c#L132-L147
            ("adpcm_ima.wav", 11 * 2041),
            ("adpcm_ms.wav", 11 * 2036),
            // Whole frames of 1152.
            ("mp2.mp2", 20 * 1152),
            // Whole frames of 1024 after 1024 frames of encoder delay, which ADTS has
            //  no field to carry.
            //  https://github.com/FFmpeg/FFmpeg/blob/894da5ca7d742e4429ffb2af534fcda0103ef593/libavcodec/aacenc.c#L1192
            ("aac.aac", 23 * 1024),
        ] {
            let probe = decode_probe(name).unwrap();

            assert_eq!(
                (probe.frames, probe.channels, probe.rate),
                (frames, 1, 44_100),
                "{name}"
            );
        }
    }

    fn first_half_of(name: &str) -> Vec<u8> {
        let mut bytes = std::fs::read(probe_path(name)).unwrap();
        bytes.truncate(bytes.len() / 2);

        bytes
    }

    #[test]
    fn a_recording_cut_short_decodes_what_was_read() {
        for name in [
            "aac.aac",
            "adpcm_ima.wav",
            "adpcm_ms.wav",
            "chord.flac",
            "pcm_f32.wav",
            "probe.flac",
            "probe.ogg",
            "probe.opus",
            "surround.opus",
        ] {
            let whole = decode_probe(name).unwrap().frames;
            let cut = decode_bytes(first_half_of(name)).unwrap().frames;

            assert!(0 < cut && cut < whole, "{name}: {cut} of {whole} frames");
        }
    }

    #[test]
    fn a_recording_read_past_its_audio_on_open_is_refused_cut_short() {
        // AIFF and CAF skip over the audio to the chunks after it, and MP4 and Matroska
        //  look for the index `ffmpeg` writes last, so the cut fails before any packet.
        //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-format-riff/src/aiff/mod.rs#L138-L142
        for name in [
            "alac.m4a",
            "matroska.webm",
            "pcm_s16.aiff",
            "pcm_s24.caf",
            "probe.m4a",
        ] {
            assert!(decode_bytes(first_half_of(name)).is_err(), "{name}");
        }
    }

    #[test]
    fn a_wav_streamed_with_its_sizes_unset_decodes_whole() {
        // What `ffmpeg -f wav -` writes: both RIFF sizes `0xFFFFFFFF`, since a pipe
        //  cannot seek back to fill them in.
        for name in ["pcm_f32.wav", "adpcm_ms.wav"] {
            let mut bytes = std::fs::read(probe_path(name)).unwrap();
            let data = bytes.windows(4).position(|tag| tag == b"data").unwrap();

            for size in [4, data + 4] {
                bytes[size..size + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            }

            assert_eq!(
                decode_bytes(bytes).unwrap().frames,
                decode_probe(name).unwrap().frames,
                "{name}"
            );
        }
    }

    #[test]
    fn a_stream_cut_before_its_audio_is_refused() {
        let wav = std::fs::read(probe_path("pcm_f32.wav")).unwrap();
        let data = wav.windows(4).position(|tag| tag == b"data").unwrap();

        let Err(error) = decode_bytes(wav[..data + 4].to_vec()) else {
            panic!("a stream cut inside its header decoded");
        };
        assert!(
            matches!(&error, SymphoniaError::IoError(io_error) if io_error.kind() == std::io::ErrorKind::UnexpectedEof),
            "{error}"
        );

        let Err(error) = decode_bytes(wav[..data + 8].to_vec()) else {
            panic!("a stream with no audio decoded");
        };
        assert_eq!(
            error.to_string(),
            "malformed stream: the stream carries no decodable audio"
        );
    }

    #[test]
    fn an_mpeg_audio_stream_cut_short_decodes_what_is_left() {
        // A run of MPEG frames ends wherever the last whole frame does, so a cut file
        //  reads as a shorter one. The MP3 header counts its frames, but without one the
        //  reader estimates the count from the bitrate, so it cannot be held to it.
        //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-bundle-mp3/src/demuxer.rs#L464-L469
        for (name, frames) in [("probe.mp3", 175_151), ("mp2.mp2", 10 * 1152)] {
            assert_eq!(
                decode_bytes(first_half_of(name)).unwrap().frames,
                frames,
                "{name}"
            );
        }
    }

    #[test]
    fn a_chain_cut_inside_its_second_link_loses_that_link() {
        // The reader hits the end while opening the second link and judges it by the
        //  first, which has read its last page, so it reports a clean end. A fix upstream
        //  fails this test, as what was read of the second link then decodes too.
        //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-format-ogg/src/demuxer.rs#L94
        //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-format-ogg/src/demuxer.rs#L148-L152
        const FIRST_LINK_FRAMES: usize = 16_000;

        let probe = decode_bytes(first_half_of("chained_capacity.ogg")).unwrap();

        assert_eq!(probe.frames, FIRST_LINK_FRAMES);
    }

    #[test]
    #[cfg_attr(not(target_os = "linux"), ignore = "the golden URI is pinned on Linux")]
    fn the_whole_pipeline_reproduces_the_golden_uri() {
        let signature = fingerprint_file(&probe_path("probe.flac"), None).unwrap();

        assert_golden_uri(&signature, "probe.flac");
    }

    #[test]
    fn the_whole_pipeline_peaks_in_every_band() {
        // The golden above pins the bytes and runs on Linux alone. This holds
        //  everywhere: peaks reached every band `FrequencyBand::containing`
        //  defines, not only some of them.
        let signature = fingerprint_file(&probe_path("probe.flac"), None).unwrap();

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
    #[cfg_attr(not(target_os = "linux"), ignore = "the golden URI is pinned on Linux")]
    fn the_chord_probe_reproduces_its_golden_uri() {
        let signature = fingerprint_file(&probe_path("chord.flac"), None).unwrap();

        assert_golden_uri(&signature, "chord.flac");
    }

    #[test]
    fn the_chord_probe_fills_the_top_band() {
        // `probe.flac` barely reaches the 3500 to 5500 Hz band, so it puts 2 peaks
        //  there against 32 here. `tests/data/generate.sh` says why, and why that
        //  makes this the probe a decoding or resampling change is judged on.
        let signature = fingerprint_file(&probe_path("chord.flac"), None).unwrap();

        let top_band = &signature.frequency_band_to_sound_peaks[&FrequencyBand::_3500_5500];

        assert!(
            top_band.len() > 20,
            "top band holds {} peaks",
            top_band.len()
        );
    }

    #[test]
    fn a_segment_shorter_than_the_file_is_cut_from_the_middle() {
        let from_file = fingerprint_file(&probe_path("probe.flac"), Some(4)).unwrap();
        let from_bytes =
            fingerprint_bytes(std::fs::read(probe_path("probe.flac")).unwrap(), Some(4)).unwrap();

        assert_eq!(from_file.number_samples, 4 * 16000);

        // Both entry points cut the same window, so they fingerprint the same audio.
        assert_eq!(
            from_bytes.encode_to_uri().unwrap(),
            from_file.encode_to_uri().unwrap(),
        );
    }

    // `probe.flac` is lossless, so its audio is the `aevalsrc` expression in
    //  `generate.sh`, restated here and mixed down to mono.
    fn probe_source_sample(time_seconds: f64) -> f64 {
        use std::f64::consts::PI;

        let left = 0.30 * (2.0 * PI * (300.0 + 180.0 * time_seconds) * time_seconds).sin()
            + 0.22 * (2.0 * PI * 1237.0 * time_seconds).sin()
            + 0.16 * (2.0 * PI * 3001.0 * time_seconds).sin();
        let right = 0.28 * (2.0 * PI * (450.0 + 240.0 * time_seconds) * time_seconds).sin()
            + 0.20 * (2.0 * PI * 1601.0 * time_seconds).sin()
            + 0.14 * (2.0 * PI * 2699.0 * time_seconds).sin();

        (left + right) / 2.0
    }

    // The RMS difference between the segment and the source it should hold from
    //  `first_sample` on, with the source read `shift` samples later.
    fn rms_error_against_source(segment: &[i16], first_sample: usize, shift: f64) -> f64 {
        let squared_error: f64 = segment
            .iter()
            .enumerate()
            .map(|(index, &sample)| {
                let time_seconds =
                    (first_sample as f64 + index as f64 + shift) / f64::from(SAMPLE_RATE_HZ);
                let resampled = f64::from(sample) / f64::from(i16::MAX);

                (resampled - probe_source_sample(time_seconds)).powi(2)
            })
            .sum();

        (squared_error / segment.len() as f64).sqrt()
    }

    #[test]
    fn the_middle_window_holds_the_middle_of_the_source() {
        // 4 s out of the 8 s probe spans 2 s to 6 s.
        const SEGMENT_SECONDS: u32 = 4;
        const FIRST_SAMPLE: usize = 2 * SAMPLE_RATE_HZ as usize;

        let decoder = MonoDecoder::from_file(&probe_path("probe.flac")).unwrap();
        let samples = pcm_samples(decoder).unwrap();
        let segment = middle_segment(&samples, Some(SEGMENT_SECONDS));

        // The resampled audio leads the source by about 0.6 of a sample, so the best
        //  alignment is searched for in tenths. A window off by one whole sample moves
        //  it out of 0 to 1, and a wrong decode or downmix leaves no alignment close.
        let (best_shift, best_error) = (-20..=20)
            .map(|tenths| {
                let shift = f64::from(tenths) / 10.0;

                (
                    shift,
                    rms_error_against_source(segment, FIRST_SAMPLE, shift),
                )
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .unwrap();

        assert!(best_error < 0.01, "{best_error} at a shift of {best_shift}");
        assert!((0.0..1.0).contains(&best_shift), "{best_shift}");
    }

    #[test]
    fn a_duration_at_or_above_the_file_length_selects_the_whole_file() {
        // 8 s at 16 kHz, which is `probe.flac` resampled and not cut.
        const WHOLE_FILE_SAMPLES: u32 = 8 * 16000;

        // 268436 is where the product left `u32`: it selected 544 ms of this file and
        //  reported success, and 268435456 selected nothing at all.
        for duration in [10, 268_436, 268_435_456, u32::MAX] {
            let from_file = fingerprint_file(&probe_path("probe.flac"), Some(duration)).unwrap();
            let from_bytes = fingerprint_bytes(
                std::fs::read(probe_path("probe.flac")).unwrap(),
                Some(duration),
            )
            .unwrap();

            assert_eq!(from_file.number_samples, WHOLE_FILE_SAMPLES, "{duration}");
            assert_eq!(from_bytes.number_samples, WHOLE_FILE_SAMPLES, "{duration}");
        }
    }

    #[test]
    fn the_bytes_of_a_file_fingerprint_the_same_as_its_path() {
        let probe = std::fs::read(probe_path("probe.flac")).unwrap();

        let from_bytes = fingerprint_bytes(probe, None).unwrap();
        let from_file = fingerprint_file(&probe_path("probe.flac"), None).unwrap();

        // The path form is what the golden above pins, so comparing the two carries
        //  the same guarantee on Linux and still runs everywhere else.
        assert_eq!(
            from_bytes.encode_to_uri().unwrap(),
            from_file.encode_to_uri().unwrap(),
        );
    }

    #[test]
    fn input_no_decoder_understands_is_an_error() {
        let not_audio = fingerprint_bytes(b"not audio".to_vec(), None);
        let script = Path::new(DATA_DIRECTORY).join("generate.sh");
        let not_a_sound_file = fingerprint_file(&script, None);

        assert!(not_audio.is_err());
        assert!(not_a_sound_file.is_err());
    }
}
