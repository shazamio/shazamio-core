use opus::{Channels as OpusChannels, Decoder as LibopusDecoder};
use symphonia::core::audio::{AsAudioBufferRef, AudioBuffer, AudioBufferRef, Signal, SignalSpec};
use symphonia::core::codecs::{
    CodecDescriptor, CodecParameters, Decoder, DecoderOptions, FinalizeResult, CODEC_TYPE_OPUS,
};
use symphonia::core::errors::{decode_error, unsupported_error, Error, Result};
use symphonia::core::formats::Packet;
use symphonia::core::support_codec;

// An Opus stream always decodes at 48 kHz, whatever rate the encoder was fed.
//  https://www.rfc-editor.org/rfc/rfc7845#section-3
const OPUS_SAMPLE_RATE: u32 = 48_000;

// The longest frame Opus defines is 120 ms, and a packet holds at most one frame
//  duration's worth of audio per channel, so nothing decodes to more than this.
//  https://www.rfc-editor.org/rfc/rfc6716#section-2
const MAX_FRAMES_PER_PACKET: usize = 120 * OPUS_SAMPLE_RATE as usize / 1000;

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
    frames_to_skip: usize,
}

// `opus::Decoder` is `Send` but not `Sync`, and `symphonia`'s `Decoder` wants both.
//  Everything reaching the `libopus` pointer takes `&mut self`, so a shared reference
//  cannot get at it: the two `&self` methods below read the other fields only.
//  https://github.com/SpaceManiac/opus-rs/blob/master/src/lib.rs
unsafe impl Sync for OpusDecoder {}

impl Decoder for OpusDecoder {
    fn try_new(params: &CodecParameters, _options: &DecoderOptions) -> Result<Self> {
        let Some(channels) = params.channels else {
            return unsupported_error("opus: the stream declares no channel layout");
        };

        // `libopus` decodes mono and stereo directly; anything wider is a multistream
        //  layout needing the mapping table from `OpusHead` and a different decoder.
        //  Music files are never that, so the case is refused rather than guessed at.
        let channel_count = channels.count();
        let opus_channels = match channel_count {
            1 => OpusChannels::Mono,
            2 => OpusChannels::Stereo,
            _ => return unsupported_error("opus: only mono and stereo streams are supported"),
        };

        let decoder = LibopusDecoder::new(OPUS_SAMPLE_RATE, opus_channels)
            .map_err(|_| Error::Unsupported("opus: libopus refused the stream"))?;

        let spec = SignalSpec::new(OPUS_SAMPLE_RATE, channels);

        Ok(Self {
            decoder,
            codec_parameters: params.clone(),
            buffer: AudioBuffer::new(MAX_FRAMES_PER_PACKET as u64, spec),
            interleaved_samples: vec![0.0; MAX_FRAMES_PER_PACKET * channel_count],
            channel_count,
            // The encoder's own warm-up, which `OpusHead` names and the spec says to
            //  discard. https://www.rfc-editor.org/rfc/rfc7845#section-4.2
            frames_to_skip: params.delay.unwrap_or(0) as usize,
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

        let skipped_frames = self.frames_to_skip.min(decoded_frames);
        self.frames_to_skip -= skipped_frames;
        let kept_frames = decoded_frames - skipped_frames;

        self.buffer.render_reserved(Some(kept_frames));

        for channel_index in 0..self.channel_count {
            let channel = self.buffer.chan_mut(channel_index);

            for (frame_index, sample) in channel.iter_mut().enumerate() {
                let offset = (skipped_frames + frame_index) * self.channel_count + channel_index;
                *sample = self.interleaved_samples[offset];
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
