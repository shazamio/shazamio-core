use std::fs::File;
use std::io::{Cursor, ErrorKind};
use std::path::Path;
use std::sync::OnceLock;

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{CodecRegistry, Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSource, MediaSourceStream};
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

/// The track a packet has to belong to, and the decoder that reads it.
struct TrackDecoder {
    track_id: u32,
    decoder: Box<dyn Decoder>,
}

/// Picks the first track carrying audio and builds a decoder for it.
fn decoder_for(format: &dyn FormatReader) -> Result<TrackDecoder, Error> {
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(Error::Unsupported("codec"))?;

    let decoder = codec_registry().make(&track.codec_params, &DecoderOptions::default())?;

    Ok(TrackDecoder {
        track_id: track.id,
        decoder,
    })
}

/// Reads the packets of one track, decoded and mixed down to mono.
struct PacketDecoder {
    format: Box<dyn FormatReader>,
    track_decoder: TrackDecoder,
    sample_buffer: Option<SampleBuffer<f32>>,
}

impl PacketDecoder {
    fn new(source: Box<dyn MediaSource>) -> Result<Self, Error> {
        let media_source = MediaSourceStream::new(source, Default::default());

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

        let format = probe_result.format;
        let track_decoder = decoder_for(format.as_ref())?;

        Ok(PacketDecoder {
            format,
            track_decoder,
            sample_buffer: None,
        })
    }

    /// Fills `mono_frames` with the next packet and reports the spec it decoded under,
    /// or `None` once the stream ends.
    fn next(&mut self, mono_frames: &mut Vec<f32>) -> Result<Option<SignalSpec>, Error> {
        mono_frames.clear();

        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,

                // `next_packet` reports the end of the stream as an `UnexpectedEof` read
                //  error rather than as `None`, and every other error is real.
                //  https://docs.rs/symphonia-core/0.5.5/symphonia_core/formats/trait.FormatReader.html#tymethod.next_packet
                Err(Error::IoError(er)) if er.kind() == ErrorKind::UnexpectedEof => {
                    return Ok(None)
                }

                // A chained Ogg file opens a second logical stream, and the reader asks
                //  for a new decoder rather than for the read to stop. Read as the end, a
                //  four-second chained file decoded to 2013 ms and reported success.
                //  https://www.rfc-editor.org/rfc/rfc7845#section-2
                Err(Error::ResetRequired) => {
                    self.track_decoder = decoder_for(self.format.as_ref())?;
                    continue;
                }

                Err(er) => return Err(er),
            };

            // If the packet does not belong to the selected track, skip it.
            if packet.track_id() != self.track_decoder.track_id {
                continue;
            }

            let audio_buffer = self.track_decoder.decoder.decode(&packet)?;
            let spec = *audio_buffer.spec();
            let channel_count = spec.channels.count();

            // `SampleBuffer::capacity` counts samples and `AudioBufferRef::capacity`
            //  frames, so comparing them raw let a stereo packet reuse a buffer half the
            //  size it needed, and `copy_interleaved_ref` panicked on its own assertion.
            let required_samples = audio_buffer.capacity() * channel_count;

            if self
                .sample_buffer
                .as_ref()
                .is_none_or(|buffer| buffer.capacity() < required_samples)
            {
                self.sample_buffer = Some(SampleBuffer::new(audio_buffer.capacity() as u64, spec));
            }

            if let Some(buffer) = self.sample_buffer.as_mut() {
                buffer.copy_interleaved_ref(audio_buffer);

                // Mixing down here rather than after the whole file is what keeps the
                //  interleaved samples of a long recording from being held at all.
                mono_frames.resize(buffer.samples().len() / channel_count, 0f32);

                for (index, sample) in buffer.samples().iter().enumerate() {
                    mono_frames[index / channel_count] += sample / channel_count as f32;
                }
            }

            return Ok(Some(spec));
        }
    }
}

/// Decodes a stream one packet at a time, mixed down to mono at the rate it declares.
pub struct MonoDecoder {
    packets: PacketDecoder,
    spec: SignalSpec,
    first_chunk: Option<Vec<f32>>,
}

impl MonoDecoder {
    /// Reads a stream already in memory, as the bytes entry point hands it over.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error> {
        MonoDecoder::new(Box::new(Cursor::new(bytes)))
    }

    /// Reads a file from disk, so a long recording is never held as bytes either.
    pub fn from_file(file_path: &Path) -> Result<Self, Error> {
        MonoDecoder::new(Box::new(File::open(file_path)?))
    }

    fn new(source: Box<dyn MediaSource>) -> Result<Self, Error> {
        let mut packets = PacketDecoder::new(source)?;
        let mut first_chunk = Vec::new();

        // The spec comes from the packets rather than from the container, because the
        //  decoder is the authority on what it produced. Nothing is assumed before the
        //  first one arrives, so a stream that decodes to nothing is an error here.
        let Some(spec) = packets.next(&mut first_chunk)? else {
            return Err(Error::DecodeError("the stream carries no decodable audio"));
        };

        Ok(MonoDecoder {
            packets,
            spec,
            first_chunk: Some(first_chunk),
        })
    }

    /// The spec every packet of the stream decodes under.
    pub fn spec(&self) -> SignalSpec {
        self.spec
    }

    /// Takes the next packet of the stream, and reports whether one arrived.
    pub fn next_chunk(&mut self, mono_frames: &mut Vec<f32>) -> Result<bool, Error> {
        if let Some(first_chunk) = self.first_chunk.take() {
            *mono_frames = first_chunk;
            return Ok(true);
        }

        let Some(spec) = self.packets.next(mono_frames)? else {
            return Ok(false);
        };

        // A chained stream may open its next link at another rate or channel count, and
        //  frames of two shapes cannot share one stream. Accepted regardless, a 2 s mono
        //  link followed by a 2 s stereo one came out as 3000 ms of a 4000 ms file.
        if spec != self.spec {
            return Err(Error::Unsupported(
                "the stream changes format part-way through",
            ));
        }

        Ok(true)
    }
}
