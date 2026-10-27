use std::fs::File;
use std::io::{Cursor, ErrorKind};
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
use symphonia::core::packet::Packet;

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

fn is_unexpected_eof(error: &Error) -> bool {
    matches!(error, Error::IoError(error) if error.kind() == ErrorKind::UnexpectedEof)
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

        PacketDecoder::from_format(format)
    }

    fn from_format(format: Box<dyn FormatReader>) -> Result<Self, Error> {
        let track_decoder = decoder_for(format.as_ref())?;

        Ok(PacketDecoder {
            format,
            track_decoder,
            interleaved_samples: Vec::new(),
        })
    }

    fn next_packet(&mut self) -> Result<Option<Packet>, Error> {
        loop {
            let packet = self.format.next_packet();

            // A stream written to a pipe cannot seek back to fill in its length, so
            //  `ffmpeg -f wav -` declares `0xFFFFFFFF` and the reader runs out first.
            //  Refused, it raised `unexpected end of file`; Shazam needs only seconds,
            //  so what was read is fingerprinted, as MPEG audio always was. A frame
            //  whose length field is damaged runs out the same way, and is cut short too.
            //  https://github.com/FFmpeg/FFmpeg/blob/894da5ca7d742e4429ffb2af534fcda0103ef593/libavformat/wavenc.c#L313
            if matches!(&packet, Err(error) if is_unexpected_eof(error)) {
                return Ok(None);
            }

            if !matches!(packet, Err(Error::ResetRequired)) {
                return packet;
            }

            // A chained Ogg file opens a second logical stream, and the reader asks for
            //  a new decoder rather than for the read to stop. Read as the end, a
            //  four-second chained file decoded to 2013 ms and reported success.
            //  https://www.rfc-editor.org/rfc/rfc7845#section-2
            self.track_decoder = decoder_for(self.format.as_ref())?;
        }
    }

    fn end_of_stream_or(&mut self, error: Error) -> Result<Option<AudioSpec>, Error> {
        if matches!(error, Error::IoError(_)) && self.next_packet()?.is_none() {
            return Ok(None);
        }

        Err(error)
    }

    /// Fills `mono_frames` with the next packet and reports the spec it decoded under,
    /// or `None` once the stream ends.
    fn next(&mut self, mono_frames: &mut Vec<f32>) -> Result<Option<AudioSpec>, Error> {
        mono_frames.clear();

        loop {
            let Some(packet) = self.next_packet()? else {
                return Ok(None);
            };

            // If the packet does not belong to the selected track, skip it.
            if packet.track_id != self.track_decoder.track_id {
                continue;
            }

            // The reader hands over what was left as a short last packet: ADPCM refused
            //  it with `buffer underrun`, FLAC with `unexpected end of bitstream`, whose
            //  kind is `Other`. A decoder reads only the packet, so any I/O error means it
            //  ran short; mid-stream, that still means a damaged packet.
            //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-core/src/io/mod.rs#L345-L347
            //  https://github.com/pdeljanov/Symphonia/blob/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee/symphonia-core/src/io/bit.rs#L14-L16
            if let Err(error) = self.track_decoder.decoder.decode(&packet) {
                return self.end_of_stream_or(error);
            }

            let audio_buffer = self.track_decoder.decoder.last_decoded();
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

#[cfg(test)]
mod tests {
    use super::*;

    use symphonia::core::formats::{FormatInfo, MediaInfo, SeekMode, SeekTo, SeekedTo, Track};
    use symphonia::core::meta::Metadata;

    use crate::fingerprinting::probe_path;

    fn probe(name: &str) -> Box<dyn FormatReader> {
        let file = File::open(probe_path(name)).unwrap();

        symphonia::default::get_probe()
            .probe(
                &Hint::new(),
                MediaSourceStream::new(Box::new(file), Default::default()),
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .unwrap()
    }

    /// Halves the third packet, where a cut only ever shortens the last one.
    struct HalvesThirdPacket {
        format: Box<dyn FormatReader>,
        packets_read: usize,
    }

    impl FormatReader for HalvesThirdPacket {
        fn format_info(&self) -> &FormatInfo {
            self.format.format_info()
        }

        fn media_info(&self) -> &MediaInfo {
            self.format.media_info()
        }

        fn metadata(&mut self) -> Metadata<'_> {
            self.format.metadata()
        }

        fn seek(&mut self, mode: SeekMode, to: SeekTo) -> Result<SeekedTo, Error> {
            self.format.seek(mode, to)
        }

        fn tracks(&self) -> &[Track] {
            self.format.tracks()
        }

        fn next_packet(&mut self) -> Result<Option<Packet>, Error> {
            let Some(mut packet) = self.format.next_packet()? else {
                return Ok(None);
            };

            self.packets_read += 1;
            if self.packets_read == 3 {
                packet.data = packet.data[..packet.data.len() / 2].into();
            }

            Ok(Some(packet))
        }

        fn into_inner<'s>(self: Box<Self>) -> MediaSourceStream<'s> {
            self.format.into_inner()
        }
    }

    #[test]
    fn a_short_packet_mid_stream_is_refused() {
        let mut packets = PacketDecoder::from_format(Box::new(HalvesThirdPacket {
            format: probe("adpcm_ms.wav"),
            packets_read: 0,
        }))
        .unwrap();
        let mut mono_frames = Vec::new();

        let error = loop {
            match packets.next(&mut mono_frames) {
                Ok(Some(_)) => continue,
                Ok(None) => panic!("the stream ended at the short packet"),
                Err(error) => break error,
            }
        };

        assert!(is_unexpected_eof(&error), "{error}");
    }

    /// Halves the last packet, as a cut inside it does.
    struct HalvesLastPacket {
        format: Box<dyn FormatReader>,
        next: Option<Packet>,
    }

    impl FormatReader for HalvesLastPacket {
        fn format_info(&self) -> &FormatInfo {
            self.format.format_info()
        }

        fn media_info(&self) -> &MediaInfo {
            self.format.media_info()
        }

        fn metadata(&mut self) -> Metadata<'_> {
            self.format.metadata()
        }

        fn seek(&mut self, mode: SeekMode, to: SeekTo) -> Result<SeekedTo, Error> {
            self.format.seek(mode, to)
        }

        fn tracks(&self) -> &[Track] {
            self.format.tracks()
        }

        fn next_packet(&mut self) -> Result<Option<Packet>, Error> {
            let Some(mut packet) = self
                .next
                .take()
                .map_or_else(|| self.format.next_packet(), |packet| Ok(Some(packet)))?
            else {
                return Ok(None);
            };

            self.next = self.format.next_packet()?;
            if self.next.is_none() {
                packet.data = packet.data[..packet.data.len() / 2].into();
            }

            Ok(Some(packet))
        }

        fn into_inner<'s>(self: Box<Self>) -> MediaSourceStream<'s> {
            self.format.into_inner()
        }
    }

    #[test]
    fn a_flac_frame_cut_short_ends_the_stream() {
        let mut packets = PacketDecoder::from_format(Box::new(HalvesLastPacket {
            format: probe("chord.flac"),
            next: None,
        }))
        .unwrap();
        let mut mono_frames = Vec::new();
        let mut frames = 0;

        while packets.next(&mut mono_frames).unwrap().is_some() {
            frames += mono_frames.len();
        }

        assert!(frames > 0);
    }

    fn decode_to_end(source: Box<dyn MediaSource>) -> Result<usize, Error> {
        let mut decoder = MonoDecoder::new(source)?;
        let mut mono_frames = Vec::new();
        let mut frames = 0;

        while decoder.next_chunk(&mut mono_frames)? {
            frames += mono_frames.len();
        }

        Ok(frames)
    }

    #[test]
    fn a_damaged_last_packet_is_refused() {
        let mut wav = std::fs::read(probe_path("adpcm_ms.wav")).unwrap();

        let fmt = wav.windows(4).position(|tag| tag == b"fmt ").unwrap();
        let data = wav.windows(4).position(|tag| tag == b"data").unwrap();
        let block_align = u16::from_le_bytes([wav[fmt + 20], wav[fmt + 21]]) as usize;
        let data_size = u32::from_le_bytes(wav[data + 4..data + 8].try_into().unwrap()) as usize;

        let last_block = data + 8 + (data_size - 1) / block_align * block_align;
        wav[last_block] = 0xFF;

        let error = decode_to_end(Box::new(Cursor::new(wav))).unwrap_err();

        assert!(!is_unexpected_eof(&error), "{error}");
    }

    /// A pipe that breaks after `limit` bytes. Seekable, it failed in the probe instead.
    struct BreaksPastLimit {
        bytes: Cursor<Vec<u8>>,
        limit: u64,
    }

    impl std::io::Read for BreaksPastLimit {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.bytes.position() >= self.limit {
                return Err(std::io::Error::other("the pipe broke"));
            }

            self.bytes.read(buffer)
        }
    }

    impl std::io::Seek for BreaksPastLimit {
        fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
            self.bytes.seek(position)
        }
    }

    impl MediaSource for BreaksPastLimit {
        fn is_seekable(&self) -> bool {
            false
        }

        fn byte_len(&self) -> Option<u64> {
            None
        }
    }

    #[test]
    fn a_read_error_mid_stream_is_refused() {
        let wav = std::fs::read(probe_path("pcm_f32.wav")).unwrap();
        let limit = wav.len() as u64 / 2;

        let error = decode_to_end(Box::new(BreaksPastLimit {
            bytes: Cursor::new(wav),
            limit,
        }))
        .unwrap_err();

        assert_eq!(error.to_string(), "the pipe broke");
    }
}
