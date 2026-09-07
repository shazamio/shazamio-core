use std::io::{Cursor, ErrorKind};
use std::sync::OnceLock;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{CodecRegistry, Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::fingerprinting::opus_decoder::OpusDecoder;

/// Every codec `symphonia` enables, plus the Opus decoder it does not ship.
fn codec_registry() -> &'static CodecRegistry {
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();

    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_all::<OpusDecoder>();
        registry
    })
}

/// Picks the first track carrying audio and builds a decoder for it.
fn decoder_for(format: &dyn FormatReader) -> Result<(u32, Box<dyn Decoder>), Error> {
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(Error::Unsupported("codec"))?;

    let decoder = codec_registry().make(&track.codec_params, &DecoderOptions::default())?;

    Ok((track.id, decoder))
}

pub fn samples_from_bytes(bytes: Vec<u8>) -> Result<(SignalSpec, Vec<f32>), Error> {
    let media_source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());

    // A lossy encoder pads the stream it writes, and the padding is silence the
    //  container describes rather than audio. Left off, `probe.opus` decoded to
    //  8013 ms of an 8000 ms source and `probe.mp3` to 8045 ms of the same.
    //  https://docs.rs/symphonia-core/0.5.5/symphonia_core/formats/struct.FormatOptions.html#structfield.enable_gapless
    let format_options = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };

    let probe_result = symphonia::default::get_probe().format(
        &Hint::new(),
        media_source,
        &format_options,
        &MetadataOptions::default(),
    )?;

    let mut format = probe_result.format;
    let (mut track_id, mut decoder) = decoder_for(format.as_ref())?;

    // The spec comes from the packets rather than from the container, because the
    //  decoder is the authority on what it produced. Nothing is assumed before the
    //  first one arrives, so a stream that decodes to nothing is an error below.
    let mut spec: Option<SignalSpec> = None;
    let mut sample_buffer: Option<SampleBuffer<f32>> = None;
    let mut aggregate_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,

            // `next_packet` reports the end of the stream as an `UnexpectedEof` read
            //  error rather than as `None`, and every other error is real.
            //  https://docs.rs/symphonia-core/0.5.5/symphonia_core/formats/trait.FormatReader.html#tymethod.next_packet
            Err(Error::IoError(er)) if er.kind() == ErrorKind::UnexpectedEof => break,

            // A chained Ogg file opens a second logical stream, and the reader asks
            //  for a new decoder rather than for the read to stop. Read as the end, a
            //  four-second chained file decoded to 2013 ms and reported success.
            //  https://www.rfc-editor.org/rfc/rfc7845#section-2
            Err(Error::ResetRequired) => {
                (track_id, decoder) = decoder_for(format.as_ref())?;
                continue;
            }

            Err(er) => return Err(er),
        };

        // If the packet does not belong to the selected track, skip it.
        if packet.track_id() != track_id {
            continue;
        }

        let audio_buffer = decoder.decode(&packet)?;
        let packet_spec = *audio_buffer.spec();

        // `SampleBuffer::capacity` counts samples and `AudioBufferRef::capacity`
        //  frames, so comparing them raw let a stereo packet reuse a buffer half the
        //  size it needed, and `copy_interleaved_ref` panicked on its own assertion.
        let required_samples = audio_buffer.capacity() * packet_spec.channels.count();

        if sample_buffer
            .as_ref()
            .is_none_or(|buffer| buffer.capacity() < required_samples)
        {
            sample_buffer = Some(SampleBuffer::new(
                audio_buffer.capacity() as u64,
                packet_spec,
            ));
        }

        if let Some(buffer) = sample_buffer.as_mut() {
            buffer.copy_interleaved_ref(audio_buffer);
            aggregate_samples.extend_from_slice(buffer.samples());
        }

        spec = Some(packet_spec);
    }

    let Some(spec) = spec else {
        return Err(Error::DecodeError("the stream carries no decodable audio"));
    };

    Ok((spec, aggregate_samples))
}
