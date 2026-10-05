use std::fs::File;
use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;

use symphonia::core::audio::AudioSpec;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;

use crate::fingerprinting::opus_decoder::OpusDecoder;

/// Every codec `symphonia` enables, plus the Opus decoder it does not ship.
fn codec_registry() -> &'static CodecRegistry {
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();

    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_audio_decoder::<OpusDecoder>();
        registry
    })
}

/// The track a packet has to belong to, and the decoder that reads it.
struct TrackDecoder {
    track_id: u32,
    decoder: Box<dyn AudioDecoder>,
}

/// Picks the first audio track this build has a decoder for, and builds that decoder.
fn decoder_for(format: &dyn FormatReader) -> Result<TrackDecoder, Error> {
    // A codec `symphonia` can name is not one it can decode: AC-3 has an id and no
    //  decoder, so taking the first known codec refused AC-3 followed by FLAC with
    //  `core (codec): unsupported audio codec` instead of reading the FLAC.
    //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-core/src/formats/mod.rs#L602-L617
    let Some((track_id, codec_parameters)) = format.tracks().iter().find_map(|track| {
        let codec_parameters = track.codec_params.as_ref()?.audio()?;
        codec_registry().get_audio_decoder(codec_parameters.codec)?;
        Some((track.id, codec_parameters))
    }) else {
        return Err(Error::Unsupported(
            "the stream carries no track with a codec this build can decode",
        ));
    };

    let decoder =
        codec_registry().make_audio_decoder(codec_parameters, &AudioDecoderOptions::default())?;

    Ok(TrackDecoder { track_id, decoder })
}

/// Reads the packets of one track, decoded and mixed down to mono.
struct PacketDecoder {
    format: Box<dyn FormatReader>,
    track_decoder: TrackDecoder,
    interleaved_samples: Vec<f32>,
}

impl PacketDecoder {
    fn new(source: Box<dyn MediaSource>) -> Result<Self, Error> {
        let media_source = MediaSourceStream::new(source, Default::default());

        // `symphonia` reports a stream it cannot recognise by naming its own probe:
        //  `unsupported feature: core (probe): no suitable format reader found`. A caller
        //  can act on none of that, and this is the only place that knows the failure
        //  means nothing here could read the stream at all.
        //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-core/src/formats/probe.rs#L597
        let format = symphonia::default::get_probe()
            .probe(
                &Hint::new(),
                media_source,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|error| match error {
                Error::Unsupported(_) => {
                    Error::Unsupported("no reader in this build recognises the stream")
                }
                other => other,
            })?;

        let track_decoder = decoder_for(format.as_ref())?;

        Ok(PacketDecoder {
            format,
            track_decoder,
            interleaved_samples: Vec::new(),
        })
    }

    /// Fills `mono_frames` with the next packet and reports the spec it decoded under,
    /// or `None` once the stream ends.
    fn next(&mut self, mono_frames: &mut Vec<f32>) -> Result<Option<AudioSpec>, Error> {
        mono_frames.clear();

        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(None),

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
            if packet.track_id != self.track_decoder.track_id {
                continue;
            }

            let audio_buffer = self.track_decoder.decoder.decode(&packet)?;
            let spec = audio_buffer.spec().clone();
            let channel_count = spec.channels().count();

            audio_buffer.copy_to_vec_interleaved(&mut self.interleaved_samples);

            // Mixing down here rather than after the whole file is what keeps the
            //  interleaved samples of a long recording from being held at all.
            mono_frames.resize(self.interleaved_samples.len() / channel_count, 0f32);

            for (index, sample) in self.interleaved_samples.iter().enumerate() {
                mono_frames[index / channel_count] += sample / channel_count as f32;
            }

            return Ok(Some(spec));
        }
    }
}

/// Decodes a stream one packet at a time, mixed down to mono at the rate it declares.
pub struct MonoDecoder {
    packets: PacketDecoder,
    spec: AudioSpec,
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
    pub fn spec(&self) -> &AudioSpec {
        &self.spec
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
