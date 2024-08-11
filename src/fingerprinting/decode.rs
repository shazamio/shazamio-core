use std::io::Cursor;

use symphonia::core::io::MediaSourceStream;
use symphonia::core::audio::{Channels, SampleBuffer, SignalSpec};
use symphonia::core::errors::Error;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub fn samples_from_bytes(
    bytes: Vec<u8>,
    seconds: usize,
    offset: usize
) -> Result<(SignalSpec, Vec<f32>), Error> {
    // Create the media source stream.
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());    

    let probe_result = symphonia::default::get_probe().format(&Hint::new(), mss, &FormatOptions::default(), &MetadataOptions::default())?;

    // Get the instantiated format reader.
    let mut format = probe_result.format;

    // Find the first audio track with a known (decodeable) codec.
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(Error::Unsupported("codec"))?;

    // Create a decoder for the track.
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    // Store the track identifier, it will be used to filter packets.
    let track_id = track.id;

    let mut spec: SignalSpec = SignalSpec::new(track.codec_params.sample_rate.unwrap_or(96000), Channels::FRONT_LEFT | Channels::FRONT_RIGHT);
    let mut aggregate_samples: Vec<f32> = Vec::with_capacity(12 * spec.rate as usize * 2);
    let mut sample_buf = SampleBuffer::<f32>::new(0, spec);
    loop {
        // Get the next packet from the format reader.
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(_) => break,
        };
        // If the packet does not belong to the selected track, skip it.
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {         
                spec = *audio_buf.spec();
                if sample_buf.capacity() < audio_buf.capacity() {
                    sample_buf = SampleBuffer::<f32>::new(audio_buf.capacity() as u64, spec);
                }

                sample_buf.copy_interleaved_ref(audio_buf);
                aggregate_samples.extend_from_slice(sample_buf.samples());                

                if aggregate_samples.len() >= ((seconds + offset) * spec.rate as usize * spec.channels.count()) {
                    break;
                }
            }
            Err(err) => return Err(err),
        }
    }
    
    Ok((spec, aggregate_samples))
}

