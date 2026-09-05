use std::io::Cursor;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub fn samples_from_bytes(
    bytes: Vec<u8>,
    seconds: usize,
    offset: usize,
) -> Result<(SignalSpec, Vec<f32>), Error> {
    let media_source = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());

    let probe_result = symphonia::default::get_probe().format(
        &Hint::new(),
        media_source,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let mut format = probe_result.format;

    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(Error::Unsupported("codec"))?;

    let track_id = track.id;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    // The spec comes from the packets rather than from the container, because the
    //  decoder is the authority on what it produced. Nothing is assumed before the
    //  first one arrives, so a stream that decodes to nothing is an error below.
    let mut spec: Option<SignalSpec> = None;
    let mut sample_buffer: Option<SampleBuffer<f32>> = None;
    let mut aggregate_samples: Vec<f32> = Vec::new();

    // `next_packet` reports the end of the stream as an error rather than as `None`,
    //  so every error ends the read. https://docs.rs/symphonia-core/0.5.5/symphonia_core/formats/trait.FormatReader.html#tymethod.next_packet
    while let Ok(packet) = format.next_packet() {
        // If the packet does not belong to the selected track, skip it.
        if packet.track_id() != track_id {
            continue;
        }

        let audio_buffer = decoder.decode(&packet)?;
        let packet_spec = *audio_buffer.spec();

        if sample_buffer
            .as_ref()
            .is_none_or(|buffer| buffer.capacity() < audio_buffer.capacity())
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

        // `seconds` is `usize::MAX` when the whole file is wanted, so the product has
        //  to saturate: a plain multiply panics with `attempt to multiply with overflow`.
        let sample_limit = seconds
            .saturating_add(offset)
            .saturating_mul(packet_spec.rate as usize)
            .saturating_mul(packet_spec.channels.count());
        if aggregate_samples.len() >= sample_limit {
            break;
        }
    }

    let Some(spec) = spec else {
        return Err(Error::DecodeError("the stream carries no decodable audio"));
    };

    Ok((spec, aggregate_samples))
}
