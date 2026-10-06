use opus::MSDecoder as LibopusDecoder;
use symphonia::core::audio::{
    AsGenericAudioBufferRef, AudioBuffer, AudioMut, AudioSpec, Channels, GenericAudioBufferRef,
};
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::{
    AudioCodecParameters, AudioDecoder, AudioDecoderOptions, FinalizeResult,
};
use symphonia::core::codecs::registry::{RegisterableAudioDecoder, SupportedAudioCodec};
use symphonia::core::codecs::CodecInfo;
use symphonia::core::errors::{decode_error, unsupported_error, Error, Result};
use symphonia::core::packet::PacketRef;

// An Opus stream always decodes at 48 kHz, whatever rate the encoder was fed.
//  https://www.rfc-editor.org/rfc/rfc7845#section-3
const OPUS_SAMPLE_RATE: u32 = 48_000;

// A packet carries at most 120 ms of audio: two frames of the longest 60 ms, or up
//  to 48 of the shortest, so nothing decodes to more frames than this.
//  https://www.rfc-editor.org/rfc/rfc6716#section-3.2.5
const MAX_FRAMES_PER_PACKET: usize = 120 * OPUS_SAMPLE_RATE as usize / 1000;

const OPUS_HEAD_MAGIC: &[u8] = b"OpusHead";
const OPUS_HEAD_MIN_LENGTH: usize = 19;

// Where the fields the parser reads sit in the header. The mapping table starts at
//  `STREAM_COUNT_OFFSET`, and family 0 leaves it out.
//  https://www.rfc-editor.org/rfc/rfc7845#section-5.1
const VERSION_OFFSET: usize = 8;
const CHANNEL_COUNT_OFFSET: usize = 9;
const PRE_SKIP_OFFSET: usize = 10;
const OUTPUT_GAIN_OFFSET: usize = 16;
const MAPPING_FAMILY_OFFSET: usize = 18;
const STREAM_COUNT_OFFSET: usize = 19;
const COUPLED_COUNT_OFFSET: usize = 20;
const CHANNEL_MAPPING_OFFSET: usize = 21;

// The upper nibble of the version byte is the major version, which is 0 for this
//  format: the RFC says to accept 15 or less and to assume 16 or more is a format
//  this parser does not know. https://www.rfc-editor.org/rfc/rfc7845#section-5.1
const MAX_HEADER_VERSION: u8 = 15;

// The two families that name a channel order, and the one that names none and gives
//  every channel a stream of its own. Families 2 to 254 are reserved.
//  https://www.rfc-editor.org/rfc/rfc7845#section-5.1.1
const MAPPING_FAMILY_RTP: u8 = 0;
const MAPPING_FAMILY_VORBIS: u8 = 1;
const MAPPING_FAMILY_DISCRETE: u8 = 255;

/// How many channels a mapping family codes: two for the RTP family, eight for the
/// Vorbis order, and as many as the header can count for the discrete one.
/// https://www.rfc-editor.org/rfc/rfc7845#section-5.1.1
fn max_channels_for_family(mapping_family: u8) -> usize {
    match mapping_family {
        MAPPING_FAMILY_RTP => 2,
        MAPPING_FAMILY_VORBIS => 8,
        _ => usize::from(u8::MAX),
    }
}

/// How the channels are packed into Opus streams, which is what `libopus` opens on.
struct StreamLayout {
    stream_count: u8,
    coupled_count: u8,
    channel_mapping: Vec<u8>,
}

/// The fields of the `OpusHead` identification header the decoder needs.
struct OpusHead {
    channel_count: usize,
    pre_skip: usize,
    output_gain: f32,
    stream_layout: StreamLayout,
}

impl OpusHead {
    /// Reads the header, whose layout is
    /// https://www.rfc-editor.org/rfc/rfc7845#section-5.1
    fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < OPUS_HEAD_MIN_LENGTH || !data.starts_with(OPUS_HEAD_MAGIC) {
            return unsupported_error("opus: the stream carries no `OpusHead` header");
        }

        // Nothing read the version byte, so a `.webm` declaring version 255 decoded
        //  as though it were this format while `ffmpeg` refused the same stream.
        if data[VERSION_OFFSET] > MAX_HEADER_VERSION {
            return unsupported_error("opus: the stream declares an unreadable header version");
        }

        let channel_count = usize::from(data[CHANNEL_COUNT_OFFSET]);

        if channel_count == 0 {
            return unsupported_error("opus: the stream declares an unusable channel count");
        }

        let pre_skip = u16::from_le_bytes([data[PRE_SKIP_OFFSET], data[PRE_SKIP_OFFSET + 1]]);
        let output_gain_db =
            i16::from_le_bytes([data[OUTPUT_GAIN_OFFSET], data[OUTPUT_GAIN_OFFSET + 1]]);

        Ok(Self {
            channel_count,
            pre_skip: usize::from(pre_skip),
            output_gain: 10.0f32.powf(f32::from(output_gain_db) / (20.0 * 256.0)),
            stream_layout: Self::parse_stream_layout(data, channel_count)?,
        })
    }

    /// Reads the mapping table, whose layout is
    /// https://www.rfc-editor.org/rfc/rfc7845#section-5.1.1
    fn parse_stream_layout(data: &[u8], channel_count: usize) -> Result<StreamLayout> {
        let mapping_family = data[MAPPING_FAMILY_OFFSET];

        // A reserved family is not to be decoded at all, and family 2 and 3 ambisonics
        //  put a demixing matrix where the others put stream indices, so the table below
        //  would be matrix coefficients. https://www.rfc-editor.org/rfc/rfc8486#section-5.2
        if !matches!(
            mapping_family,
            MAPPING_FAMILY_RTP | MAPPING_FAMILY_VORBIS | MAPPING_FAMILY_DISCRETE
        ) {
            return unsupported_error("opus: the stream declares a mapping family we cannot read");
        }

        if channel_count > max_channels_for_family(mapping_family) {
            return unsupported_error("opus: the mapping family codes fewer channels than that");
        }

        // Family 0 codes no table: one stream, coupled for stereo, channels in order.
        //  `libopus` wants the array either way, so the implied one is written out.
        if mapping_family == MAPPING_FAMILY_RTP {
            return Ok(StreamLayout {
                stream_count: 1,
                coupled_count: u8::from(channel_count == 2),
                channel_mapping: (0..channel_count as u8).collect(),
            });
        }

        let table_end = CHANNEL_MAPPING_OFFSET + channel_count;

        if data.len() < table_end {
            return decode_error("opus: the `OpusHead` mapping table is cut short");
        }

        Ok(StreamLayout {
            stream_count: data[STREAM_COUNT_OFFSET],
            coupled_count: data[COUPLED_COUNT_OFFSET],
            channel_mapping: data[CHANNEL_MAPPING_OFFSET..table_end].to_vec(),
        })
    }
}

/// The Opus decoder `symphonia` does not ship, wired to `libopus`.
///
/// `symphonia` demuxes Ogg Opus and hands out `CODEC_ID_OPUS` packets already, so
/// only the codec itself is missing: the status table lists Opus as unsupported and
/// no `symphonia-codec-opus` crate exists.
/// https://github.com/pdeljanov/Symphonia#codecs-decoders
pub struct OpusDecoder {
    decoder: LibopusDecoder,
    codec_parameters: AudioCodecParameters,
    buffer: AudioBuffer<f32>,
    interleaved_samples: Vec<f32>,
    channel_count: usize,
    output_gain: f32,
    frames_to_skip: usize,
}

// `opus::MSDecoder` is `Send` but not `Sync`, and `symphonia`'s `AudioDecoder` wants
//  both. Everything reaching the `libopus` pointer takes `&mut self`, so a shared
//  reference cannot get at it: the three `&self` methods below read the other fields only.
//  https://github.com/SpaceManiac/opus-rs/blob/31e8ba1ae8abfa31bbe37817dbf0a8ebdeffc31c/src/lib.rs#L1203
unsafe impl Sync for OpusDecoder {}

impl OpusDecoder {
    fn try_new(params: &AudioCodecParameters) -> Result<Self> {
        // The header is the only description both containers carry. Ogg fills
        //  `params.channels` from it, Matroska leaves it `None` and a stream out of
        //  `.webm` then failed with "declares no channel layout".
        let Some(extra_data) = params.extra_data.as_deref() else {
            return unsupported_error("opus: the stream carries no `OpusHead` header");
        };

        let head = OpusHead::parse(extra_data)?;

        // Every stream goes through the multistream decoder, mono and stereo included:
        //  family 0 is the one-stream case of the same API, so the wider layouts cost
        //  no second decoding path. A `.opus` with 6 channels used to be refused here.
        let decoder = LibopusDecoder::new(
            OPUS_SAMPLE_RATE,
            head.stream_layout.stream_count,
            head.stream_layout.coupled_count,
            &head.stream_layout.channel_mapping,
        )
        .map_err(|_| Error::Unsupported("opus: libopus refused the stream"))?;

        // Only the count is read downstream, where every channel is averaged into mono,
        //  so the channels are counted rather than given the positions the Opus channel
        //  order assigns them.
        let channels = Channels::Discrete(head.channel_count as u16);
        let spec = AudioSpec::new(OPUS_SAMPLE_RATE, channels);

        Ok(Self {
            decoder,
            codec_parameters: params.clone(),
            buffer: AudioBuffer::new(spec, MAX_FRAMES_PER_PACKET),
            interleaved_samples: vec![0.0; MAX_FRAMES_PER_PACKET * head.channel_count],
            channel_count: head.channel_count,
            output_gain: head.output_gain,
            // The encoder's own warm-up, which `OpusHead` names and the spec says to
            //  discard. https://www.rfc-editor.org/rfc/rfc7845#section-4.2
            frames_to_skip: head.pre_skip,
        })
    }
}

impl AudioDecoder for OpusDecoder {
    fn reset(&mut self) {
        // A failure here leaves the previous state in place, which decodes the next
        //  packet with stale history rather than not at all. The trait cannot report it.
        let _ = self.decoder.reset_state();
    }

    fn codec_info(&self) -> &CodecInfo {
        &OPUS_CODECS[0].info
    }

    fn codec_params(&self) -> &AudioCodecParameters {
        &self.codec_parameters
    }

    fn decode_ref(&mut self, packet: &PacketRef<'_>) -> Result<GenericAudioBufferRef<'_>> {
        self.buffer.clear();

        let Ok(decoded_frames) =
            self.decoder
                .decode_float(packet.data, &mut self.interleaved_samples, false)
        else {
            return decode_error("opus: libopus rejected the packet");
        };

        let decoded_samples = &self.interleaved_samples[..decoded_frames * self.channel_count];

        self.buffer.render_uninit(Some(decoded_frames));
        self.buffer.copy_from_slice_interleaved(&decoded_samples);

        let output_gain = self.output_gain;
        self.buffer.apply(|sample| sample * output_gain);

        // The reader trims only what the container signals: Ogg reports the end padding
        //  and leaves the pre-skip to the track delay, Matroska reports neither. Taking the
        //  larger of the two leading counts drops each frame once whichever answers.
        //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-format-ogg/src/mappings/opus.rs#L129
        let leading = self
            .frames_to_skip
            .max(packet.trim_start.get() as usize)
            .min(decoded_frames);
        self.frames_to_skip = self.frames_to_skip.saturating_sub(leading);

        let trailing = (packet.trim_end.get() as usize).min(decoded_frames - leading);
        self.buffer.trim(leading, trailing);

        Ok(self.buffer.as_generic_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> GenericAudioBufferRef<'_> {
        self.buffer.as_generic_audio_buffer_ref()
    }
}

const OPUS_CODECS: [SupportedAudioCodec; 1] = [SupportedAudioCodec {
    id: CODEC_ID_OPUS,
    info: CodecInfo {
        short_name: "opus",
        long_name: "Opus",
        profiles: &[],
    },
}];

impl RegisterableAudioDecoder for OpusDecoder {
    fn try_registry_new(
        params: &AudioCodecParameters,
        _options: &AudioDecoderOptions,
    ) -> Result<Box<dyn AudioDecoder>> {
        Ok(Box::new(OpusDecoder::try_new(params)?))
    }

    fn supported_codecs() -> &'static [SupportedAudioCodec] {
        &OPUS_CODECS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opus::{Application, Channels as OpusChannels, Encoder, MSEncoder};
    use symphonia::core::audio::Audio;
    use symphonia::core::packet::Packet;
    use symphonia::core::units::{Duration, Timestamp};

    // 20 ms, the frame size the encoder is asked for below. Any Opus frame size works;
    //  this one is the default and leaves 648 frames once the pre-skip is dropped.
    const TONE_FRAMES: usize = 960;

    const TONE_AMPLITUDE: f32 = 0.25;

    // What `opus_head` writes into every header a test builds, so a decode of one
    //  20 ms packet keeps 648 frames.
    const PRE_SKIP_FRAMES: usize = 312;

    /// An `OpusHead` of mapping family 255, which is the only one that codes more
    /// than 8 channels. Every channel gets its own uncoupled stream.
    fn opus_head_with_mapping_table(channel_count: u8) -> Box<[u8]> {
        const COUPLED_STREAM_COUNT: u8 = 0;

        // `opus_head` ends on a family 0 byte, which is the one a table replaces.
        let mut header = Vec::from(&opus_head(channel_count, 0)[..]);
        header.pop();

        header.push(MAPPING_FAMILY_DISCRETE);
        header.push(channel_count);
        header.push(COUPLED_STREAM_COUNT);
        header.extend(0..channel_count);

        header.into_boxed_slice()
    }

    /// The fixed 19 bytes of an `OpusHead`, with the two fields a test varies.
    fn opus_head(channel_count: u8, output_gain_db: i16) -> Box<[u8]> {
        let mut header = Vec::from(OPUS_HEAD_MAGIC);

        header.push(1);
        header.push(channel_count);
        header.extend_from_slice(&(PRE_SKIP_FRAMES as u16).to_le_bytes());
        header.extend_from_slice(&OPUS_SAMPLE_RATE.to_le_bytes());

        header.extend_from_slice(&output_gain_db.to_le_bytes());
        header.push(0);

        header.into_boxed_slice()
    }

    #[test]
    fn the_header_supplies_what_a_container_leaves_out() {
        // Matroska describes an Opus track with neither a channel count nor a
        //  pre-skip, so a stream out of `.webm` used to fail on the first of them.
        let mut codec_parameters = AudioCodecParameters::new();
        codec_parameters
            .for_codec(CODEC_ID_OPUS)
            .with_extra_data(opus_head(2, 0));

        let decoder = OpusDecoder::try_new(&codec_parameters).unwrap();

        assert_eq!(decoder.channel_count, 2);
        assert_eq!(decoder.frames_to_skip, PRE_SKIP_FRAMES);
    }

    #[test]
    fn the_output_gain_is_read_as_q7_8_decibels() {
        // 1536 is 6 dB, so the factor is 10 raised to 6 over 20.
        let head = OpusHead::parse(&opus_head(2, 1536)).unwrap();

        assert!(
            (head.output_gain - 1.995_262).abs() < 1e-5,
            "{}",
            head.output_gain
        );
    }

    // An Opus frame is at most 1275 bytes, and a multistream packet adds a table of
    //  contents and a self-delimiting length per stream on top of that.
    //  https://www.rfc-editor.org/rfc/rfc6716#section-3.2.1
    const MAX_PACKET_BYTES_PER_STREAM: usize = 1300;

    /// One 20 ms frame of the same tone in every channel, interleaved.
    fn tone(channel_count: usize) -> Vec<f32> {
        let mut samples = vec![0f32; TONE_FRAMES * channel_count];

        for (index, sample) in samples.iter_mut().enumerate() {
            let frame = (index / channel_count) as f32;
            *sample = TONE_AMPLITUDE
                * (std::f32::consts::TAU * 1000.0 * frame / OPUS_SAMPLE_RATE as f32).sin();
        }

        samples
    }

    /// One stereo packet holding a tone, so a decode of it has something to measure.
    fn tone_packet() -> Vec<u8> {
        let mut encoder =
            Encoder::new(OPUS_SAMPLE_RATE, OpusChannels::Stereo, Application::Audio).unwrap();

        let mut encoded = vec![0u8; MAX_PACKET_BYTES_PER_STREAM];
        let written = encoder.encode_float(&tone(2), &mut encoded).unwrap();
        encoded.truncate(written);

        encoded
    }

    /// The same packet for a layout of one uncoupled stream per channel, which is
    /// what `opus_head_with_mapping_table` describes.
    fn multistream_tone_packet(channel_count: u8) -> Vec<u8> {
        let channel_mapping: Vec<u8> = (0..channel_count).collect();

        let mut encoder = MSEncoder::new(
            OPUS_SAMPLE_RATE,
            channel_count,
            0,
            &channel_mapping,
            Application::Audio,
        )
        .unwrap();

        encoder
            .encode_vec_float(
                &tone(usize::from(channel_count)),
                MAX_PACKET_BYTES_PER_STREAM * usize::from(channel_count),
            )
            .unwrap()
    }

    /// One packet of `TONE_FRAMES` frames carrying `data`.
    fn tone_frames_packet(data: Vec<u8>) -> Packet {
        Packet::new(
            0,
            Timestamp::new(0),
            Duration::new(TONE_FRAMES as u64),
            data,
        )
    }

    /// The loudest sample the decoder produces for that packet under a given gain.
    fn decoded_peak(output_gain_db: i16) -> f32 {
        let mut codec_parameters = AudioCodecParameters::new();
        codec_parameters
            .for_codec(CODEC_ID_OPUS)
            .with_extra_data(opus_head(2, output_gain_db));

        let mut decoder = OpusDecoder::try_new(&codec_parameters).unwrap();

        decoder.decode(&tone_frames_packet(tone_packet())).unwrap();

        decoder
            .buffer
            .plane(0)
            .unwrap()
            .iter()
            .fold(0f32, |peak, sample| peak.max(sample.abs()))
    }

    #[test]
    fn the_output_gain_reaches_the_samples() {
        // The gain is a field of the header that nothing else carries, so a decoder
        //  reading only the container ignored it and returned the stream 6 dB quiet.
        let plain = decoded_peak(0);
        let amplified = decoded_peak(1536);

        assert!(
            (amplified / plain - 1.995_262).abs() < 1e-3,
            "{plain} against {amplified}"
        );
    }

    fn decoder_for_channels(channel_count: u8) -> Result<OpusDecoder> {
        let mut codec_parameters = AudioCodecParameters::new();
        codec_parameters
            .for_codec(CODEC_ID_OPUS)
            .with_extra_data(opus_head_with_mapping_table(channel_count));

        OpusDecoder::try_new(&codec_parameters)
    }

    #[test]
    fn the_widest_stream_the_header_can_count_decodes() {
        // `symphonia` 0.5 counted channels in a 26-position mask, so a 27-channel stream
        //  was refused. A discrete count takes every channel the header can name.
        let channel_count = u8::MAX;
        let mut decoder = decoder_for_channels(channel_count).unwrap();

        let decoded = decoder
            .decode(&tone_frames_packet(multistream_tone_packet(channel_count)))
            .unwrap();

        assert_eq!(
            decoded.spec().channels().count(),
            usize::from(channel_count)
        );
        assert_eq!(decoded.frames(), TONE_FRAMES - PRE_SKIP_FRAMES);
    }

    #[test]
    fn a_stream_with_no_header_is_refused() {
        let mut codec_parameters = AudioCodecParameters::new();
        codec_parameters.for_codec(CODEC_ID_OPUS);

        let Err(error) = OpusDecoder::try_new(&codec_parameters) else {
            panic!("a stream with no `OpusHead` built a decoder");
        };

        assert!(matches!(error, Error::Unsupported(_)), "{error}");
    }

    #[test]
    fn a_header_version_the_parser_does_not_know_is_refused() {
        // Nothing read the version byte, so a stream declaring a format this parser
        //  has never seen decoded as though it were an ordinary one.
        let mut header = Vec::from(&opus_head(2, 0)[..]);

        header[VERSION_OFFSET] = MAX_HEADER_VERSION;
        assert!(OpusHead::parse(&header).is_ok());

        header[VERSION_OFFSET] = MAX_HEADER_VERSION + 1;

        let Err(error) = OpusHead::parse(&header) else {
            panic!("a header of an unknown major version parsed");
        };

        assert!(matches!(error, Error::Unsupported(_)), "{error}");
    }

    #[test]
    fn a_family_coding_fewer_channels_than_the_header_names_is_refused() {
        // Family 0 codes mono and stereo only. A third channel reached `libopus` as
        //  a mapping of one stream onto three channels, which it refused itself.
        let Err(error) = OpusHead::parse(&opus_head(3, 0)) else {
            panic!("a family 0 header of three channels parsed");
        };

        assert!(matches!(error, Error::Unsupported(_)), "{error}");
    }

    #[test]
    fn a_mapping_family_the_parser_does_not_read_is_refused() {
        // Every one of these decoded as though it were family 255: two ambisonic
        //  families whose table is a demixing matrix, and two reserved numbers whose
        //  meaning is not written yet. Family 2 cannot code two channels at all.
        const AMBISONIC_AND_RESERVED: [u8; 4] = [2, 3, 4, 254];

        for mapping_family in AMBISONIC_AND_RESERVED {
            let mut header = Vec::from(&opus_head_with_mapping_table(2)[..]);
            header[MAPPING_FAMILY_OFFSET] = mapping_family;

            let Err(error) = OpusHead::parse(&header) else {
                panic!("a header of family {mapping_family} parsed as a channel mapping");
            };

            assert!(matches!(error, Error::Unsupported(_)), "{error}");
        }
    }
}
