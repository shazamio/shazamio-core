use opus::{Channels as OpusChannels, Decoder as LibopusDecoder};
use symphonia::core::audio::{
    AsAudioBufferRef, AudioBuffer, AudioBufferRef, Layout, Signal, SignalSpec,
};
use symphonia::core::codecs::{
    CodecDescriptor, CodecParameters, Decoder, DecoderOptions, FinalizeResult, CODEC_TYPE_OPUS,
};
use symphonia::core::errors::{decode_error, unsupported_error, Error, Result};
use symphonia::core::formats::Packet;
use symphonia::core::support_codec;

// An Opus stream always decodes at 48 kHz, whatever rate the encoder was fed.
//  https://www.rfc-editor.org/rfc/rfc7845#section-3
const OPUS_SAMPLE_RATE: u32 = 48_000;

// A packet carries at most 120 ms of audio, as one frame or as up to 48 short ones,
//  so nothing decodes to more frames than this.
//  https://www.rfc-editor.org/rfc/rfc6716#section-3.2.5
const MAX_FRAMES_PER_PACKET: usize = 120 * OPUS_SAMPLE_RATE as usize / 1000;

const OPUS_HEAD_MAGIC: &[u8] = b"OpusHead";
const OPUS_HEAD_MIN_LENGTH: usize = 19;

/// The fields of the `OpusHead` identification header the decoder needs.
struct OpusHead {
    channel_count: usize,
    pre_skip: usize,
    output_gain: f32,
}

impl OpusHead {
    /// Reads the fixed part of the header, whose layout is
    /// https://www.rfc-editor.org/rfc/rfc7845#section-5.1
    fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < OPUS_HEAD_MIN_LENGTH || !data.starts_with(OPUS_HEAD_MAGIC) {
            return unsupported_error("opus: the stream carries no `OpusHead` header");
        }

        let pre_skip = u16::from_le_bytes([data[10], data[11]]);
        let output_gain_db = i16::from_le_bytes([data[16], data[17]]);

        Ok(Self {
            channel_count: usize::from(data[9]),
            pre_skip: usize::from(pre_skip),
            output_gain: 10.0f32.powf(f32::from(output_gain_db) / (20.0 * 256.0)),
        })
    }
}

/// The Opus decoder `symphonia` does not ship, wired to `libopus`.
///
/// `symphonia` demuxes Ogg Opus and hands out `CODEC_TYPE_OPUS` packets already, so
/// only the codec itself is missing: the status table lists Opus as unsupported and
/// no `symphonia-codec-opus` crate exists.
/// https://github.com/pdeljanov/Symphonia#codecs-decoders
pub struct OpusDecoder {
    decoder: LibopusDecoder,
    codec_parameters: CodecParameters,
    buffer: AudioBuffer<f32>,
    interleaved_samples: Vec<f32>,
    channel_count: usize,
    output_gain: f32,
    frames_to_skip: usize,
}

// `opus::Decoder` is `Send` but not `Sync`, and `symphonia`'s `Decoder` wants both.
//  Everything reaching the `libopus` pointer takes `&mut self`, so a shared reference
//  cannot get at it: the two `&self` methods below read the other fields only.
//  https://github.com/SpaceManiac/opus-rs/blob/31e8ba1ae8abfa31bbe37817dbf0a8ebdeffc31c/src/lib.rs#L692
unsafe impl Sync for OpusDecoder {}

impl Decoder for OpusDecoder {
    fn try_new(params: &CodecParameters, _options: &DecoderOptions) -> Result<Self> {
        // The header is the only description both containers carry. Ogg fills
        //  `params.channels` and `params.delay` from it, Matroska leaves both `None`
        //  and a stream out of `.webm` then failed with "declares no channel layout".
        let Some(extra_data) = params.extra_data.as_deref() else {
            return unsupported_error("opus: the stream carries no `OpusHead` header");
        };

        let head = OpusHead::parse(extra_data)?;

        // `libopus` decodes mono and stereo directly; anything wider is a multistream
        //  layout needing the mapping table from `OpusHead` and a different decoder.
        //  Music files are never that, so the case is refused rather than guessed at.
        let (opus_channels, layout) = match head.channel_count {
            1 => (OpusChannels::Mono, Layout::Mono),
            2 => (OpusChannels::Stereo, Layout::Stereo),
            _ => return unsupported_error("opus: only mono and stereo streams are supported"),
        };

        let decoder = LibopusDecoder::new(OPUS_SAMPLE_RATE, opus_channels)
            .map_err(|_| Error::Unsupported("opus: libopus refused the stream"))?;

        let spec = SignalSpec::new_with_layout(OPUS_SAMPLE_RATE, layout);

        Ok(Self {
            decoder,
            codec_parameters: params.clone(),
            buffer: AudioBuffer::new(MAX_FRAMES_PER_PACKET as u64, spec),
            interleaved_samples: vec![0.0; MAX_FRAMES_PER_PACKET * head.channel_count],
            channel_count: head.channel_count,
            output_gain: head.output_gain,
            // The encoder's own warm-up, which `OpusHead` names and the spec says to
            //  discard. https://www.rfc-editor.org/rfc/rfc7845#section-4.2
            frames_to_skip: head.pre_skip,
        })
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[support_codec!(CODEC_TYPE_OPUS, "opus", "Opus")]
    }

    fn reset(&mut self) {
        // A failure here leaves the previous state in place, which decodes the next
        //  packet with stale history rather than not at all. The trait cannot report it.
        let _ = self.decoder.reset_state();
    }

    fn codec_params(&self) -> &CodecParameters {
        &self.codec_parameters
    }

    fn decode(&mut self, packet: &Packet) -> Result<AudioBufferRef<'_>> {
        self.buffer.clear();

        let Ok(decoded_frames) =
            self.decoder
                .decode_float(&packet.data, &mut self.interleaved_samples, false)
        else {
            return decode_error("opus: libopus rejected the packet");
        };

        // The reader trims only what the container signals: Ogg reports the end padding
        //  and leaves the pre-skip to the header, Matroska reports neither. Taking the
        //  larger of the two leading counts drops each frame once whichever answers.
        let leading = self
            .frames_to_skip
            .max(packet.trim_start() as usize)
            .min(decoded_frames);
        self.frames_to_skip = self.frames_to_skip.saturating_sub(leading);

        let trailing = (packet.trim_end() as usize).min(decoded_frames - leading);
        let kept_frames = decoded_frames - leading - trailing;

        self.buffer.render_reserved(Some(kept_frames));

        for channel_index in 0..self.channel_count {
            let channel = self.buffer.chan_mut(channel_index);

            for (frame_index, sample) in channel.iter_mut().enumerate() {
                let offset = (leading + frame_index) * self.channel_count + channel_index;
                *sample = self.interleaved_samples[offset] * self.output_gain;
            }
        }

        Ok(self.buffer.as_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.buffer.as_audio_buffer_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed 19 bytes of an `OpusHead`, with the two fields a test varies.
    fn opus_head(channel_count: u8, output_gain_db: i16) -> Box<[u8]> {
        let mut header = Vec::from(OPUS_HEAD_MAGIC);

        header.push(1);
        header.push(channel_count);
        header.extend_from_slice(&312u16.to_le_bytes());
        header.extend_from_slice(&OPUS_SAMPLE_RATE.to_le_bytes());

        header.extend_from_slice(&output_gain_db.to_le_bytes());
        header.push(0);

        header.into_boxed_slice()
    }

    #[test]
    fn the_header_supplies_what_a_container_leaves_out() {
        // Matroska describes an Opus track with neither a channel count nor a
        //  pre-skip, so a stream out of `.webm` used to fail on the first of them.
        let mut codec_parameters = CodecParameters::new();
        codec_parameters
            .for_codec(CODEC_TYPE_OPUS)
            .with_extra_data(opus_head(2, 0));

        let decoder = OpusDecoder::try_new(&codec_parameters, &DecoderOptions::default()).unwrap();

        assert_eq!(decoder.channel_count, 2);
        assert_eq!(decoder.frames_to_skip, 312);
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

    #[test]
    fn a_stream_with_no_header_is_refused() {
        let mut codec_parameters = CodecParameters::new();
        codec_parameters.for_codec(CODEC_TYPE_OPUS);

        let Err(error) = OpusDecoder::try_new(&codec_parameters, &DecoderOptions::default()) else {
            panic!("a stream with no `OpusHead` built a decoder");
        };

        assert!(matches!(error, Error::Unsupported(_)), "{error}");
    }
}
