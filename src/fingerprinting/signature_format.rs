use base64::engine::general_purpose;
use base64::Engine;
use std::collections::HashMap;

const DATA_URI_PREFIX: &str = "data:audio/vnd.shazam.sig;base64,";

const HEADER_LENGTH: usize = 48;
const HEADER_MAGIC: u32 = 0xcafe_2580;
const HEADER_SECOND_MAGIC: u32 = 0x9411_9c00;
const HEADER_FIXED_VALUE: u32 = 0x007c_0000;
const SAMPLE_RATE_ID_SHIFT: u32 = 27;
const BODY_MAGIC: u32 = 0x4000_0000;
const BAND_MARKER_BASE: u32 = 0x6003_0040;

// A frame delta this large does not fit its byte, so the frame index is written whole.
const FRAME_ESCAPE: u8 = 0xff;

pub struct FrequencyPeak {
    pub frame: u32,
    pub magnitude: u16,
    // In 1/64 of an FFT bin.
    pub refined_bin: u16,
}

#[derive(Hash, Eq, PartialEq, PartialOrd, Ord, Debug, Clone, Copy)]
pub enum FrequencyBand {
    Hz250To520 = 0,
    Hz520To1450 = 1,
    Hz1450To3500 = 2,
    Hz3500To5500 = 3,
}

impl FrequencyBand {
    pub fn containing(frequency_hz: u32) -> Option<Self> {
        match frequency_hz {
            250..=519 => Some(FrequencyBand::Hz250To520),
            520..=1449 => Some(FrequencyBand::Hz520To1450),
            1450..=3499 => Some(FrequencyBand::Hz1450To3500),
            3500..=5500 => Some(FrequencyBand::Hz3500To5500),
            _ => None,
        }
    }
}

pub struct Fingerprint {
    pub sample_rate_hz: u32,
    pub sample_count: u32,
    pub peaks_by_band: HashMap<FrequencyBand, Vec<FrequencyPeak>>,
}

fn sample_rate_id(sample_rate_hz: u32) -> u32 {
    match sample_rate_hz {
        8000 => 1,
        11025 => 2,
        16000 => 3,
        32000 => 4,
        44100 => 5,
        48000 => 6,
        _ => panic!("Invalid sample rate {sample_rate_hz} for a signature"),
    }
}

fn encode_peaks(peaks: &[FrequencyPeak]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(peaks.len() * 5);
    let mut previous_frame = 0;

    for peak in peaks {
        let mut delta = peak
            .frame
            .checked_sub(previous_frame)
            .expect("the peaks of a band are in frame order");

        if delta >= u32::from(FRAME_ESCAPE) {
            encoded.push(FRAME_ESCAPE);
            encoded.extend_from_slice(&peak.frame.to_le_bytes());
            delta = 0;
        }

        encoded.push(delta as u8);
        encoded.extend_from_slice(&peak.magnitude.to_le_bytes());
        encoded.extend_from_slice(&peak.refined_bin.to_le_bytes());

        previous_frame = peak.frame;
    }

    encoded
}

impl Fingerprint {
    // Computed in `f32` and truncated, as every release has reported it; exact
    //  integer division moves 4016 samples at 16 kHz from 250 ms to 251 ms.
    pub fn duration_ms(&self) -> u32 {
        (self.sample_count as f32 / self.sample_rate_hz as f32 * 1000.0) as u32
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bands: Vec<_> = self.peaks_by_band.iter().collect();
        bands.sort_by_key(|(band, _)| **band);

        let mut body = Vec::new();
        for (band, peaks) in bands {
            let encoded = encode_peaks(peaks);

            body.extend_from_slice(&(BAND_MARKER_BASE + *band as u32).to_le_bytes());
            body.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
            body.extend_from_slice(&encoded);
            body.resize(body.len().next_multiple_of(4), 0);
        }

        // The body opens with its own magic and length, and both lengths count them.
        let body_length = (body.len() + 8) as u32;

        // The format pads the sample count by 240 ms; `* 24 / 100` is exact at every
        //  rate `sample_rate_id` accepts.
        let padded_sample_count = self.sample_count + self.sample_rate_hz * 24 / 100;

        let words = [
            HEADER_MAGIC,
            0,
            body_length,
            HEADER_SECOND_MAGIC,
            0,
            0,
            0,
            sample_rate_id(self.sample_rate_hz) << SAMPLE_RATE_ID_SHIFT,
            0,
            0,
            padded_sample_count,
            HEADER_FIXED_VALUE,
            BODY_MAGIC,
            body_length,
        ];

        let mut signature = Vec::with_capacity(HEADER_LENGTH + body_length as usize);
        for word in words {
            signature.extend_from_slice(&word.to_le_bytes());
        }
        signature.extend_from_slice(&body);

        let checksum = crc32fast::hash(&signature[8..]);
        signature[4..8].copy_from_slice(&checksum.to_le_bytes());

        signature
    }

    pub fn to_data_uri(&self) -> String {
        format!(
            "{DATA_URI_PREFIX}{}",
            general_purpose::STANDARD.encode(self.encode())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Offsets into the 48-byte header, in the order `encode` writes it.
    const MAGIC_OFFSET: usize = 0;
    const CHECKSUM_OFFSET: usize = 4;
    const BODY_LENGTH_OFFSET: usize = 8;
    const SECOND_MAGIC_OFFSET: usize = 12;

    const SAMPLE_RATE_ID_OFFSET: usize = 28;
    const PADDED_SAMPLE_COUNT_OFFSET: usize = 40;
    const FIXED_VALUE_OFFSET: usize = 44;

    // The body repeats its own length one `u32` in, and the bands start after that.
    const BODY_LENGTH_COPY_OFFSET: usize = HEADER_LENGTH + 4;
    const FIRST_BAND_OFFSET: usize = HEADER_LENGTH + 8;

    const SAMPLE_RATE_HZ: u32 = 16000;

    fn read_u32(encoded: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(encoded[offset..offset + 4].try_into().unwrap())
    }

    fn peak(frame: u32, magnitude: u16, refined_bin: u16) -> FrequencyPeak {
        FrequencyPeak {
            frame,
            magnitude,
            refined_bin,
        }
    }

    fn fingerprint_of(bands: Vec<(FrequencyBand, Vec<FrequencyPeak>)>) -> Fingerprint {
        Fingerprint {
            sample_rate_hz: SAMPLE_RATE_HZ,
            sample_count: 128000,
            peaks_by_band: bands.into_iter().collect(),
        }
    }

    // Walks the band list the way a reader of the format has to: a marker, a length,
    //  that many bytes of peaks, then padding up to the next multiple of four.
    fn band_markers(encoded: &[u8]) -> Vec<u32> {
        let mut markers = vec![];
        let mut offset = FIRST_BAND_OFFSET;

        while offset < encoded.len() {
            markers.push(read_u32(encoded, offset));

            let peaks_length = read_u32(encoded, offset + 4) as usize;
            offset += 8 + peaks_length + (4 - peaks_length % 4) % 4;
        }

        markers
    }

    #[test]
    fn the_header_carries_the_two_magic_values_and_the_sample_rate_id() {
        let encoded = fingerprint_of(vec![]).encode();

        assert_eq!(read_u32(&encoded, MAGIC_OFFSET), 0xcafe2580);
        assert_eq!(read_u32(&encoded, SECOND_MAGIC_OFFSET), 0x94119c00);
        // 16 kHz is sample-rate id 3, and the field holds it shifted left by 27.
        assert_eq!(read_u32(&encoded, SAMPLE_RATE_ID_OFFSET), 3 << 27);
        assert_eq!(read_u32(&encoded, FIXED_VALUE_OFFSET), (15 << 19) + 0x40000);
    }

    #[test]
    fn the_sample_count_is_padded_by_240_ms_worth_of_samples() {
        let encoded = fingerprint_of(vec![]).encode();

        assert_eq!(
            read_u32(&encoded, PADDED_SAMPLE_COUNT_OFFSET),
            128000 + 3840
        );
    }

    #[test]
    fn every_supported_sample_rate_has_its_own_id_and_padding() {
        for (sample_rate_hz, sample_rate_id, padding) in [
            (8000, 1, 1920),
            (11025, 2, 2646),
            (16000, 3, 3840),
            (32000, 4, 7680),
            (44100, 5, 10584),
            (48000, 6, 11520),
        ] {
            let encoded = Fingerprint {
                sample_rate_hz,
                sample_count: 0,
                peaks_by_band: HashMap::new(),
            }
            .encode();

            assert_eq!(
                read_u32(&encoded, SAMPLE_RATE_ID_OFFSET),
                sample_rate_id << 27,
            );
            assert_eq!(read_u32(&encoded, PADDED_SAMPLE_COUNT_OFFSET), padding);
        }
    }

    #[test]
    #[should_panic(expected = "Invalid sample rate")]
    fn an_unsupported_sample_rate_panics() {
        let fingerprint = Fingerprint {
            sample_rate_hz: 22050,
            sample_count: 0,
            peaks_by_band: HashMap::new(),
        };

        fingerprint.encode();
    }

    #[test]
    fn both_length_fields_hold_everything_after_the_header() {
        let encoded = fingerprint_of(vec![(
            FrequencyBand::Hz250To520,
            vec![peak(0, 0x1234, 0x5678)],
        )])
        .encode();

        let body_length = (encoded.len() - HEADER_LENGTH) as u32;

        assert_eq!(read_u32(&encoded, BODY_LENGTH_OFFSET), body_length);
        assert_eq!(read_u32(&encoded, BODY_LENGTH_COPY_OFFSET), body_length);
    }

    #[test]
    fn the_checksum_covers_every_byte_after_itself() {
        let encoded =
            fingerprint_of(vec![(FrequencyBand::Hz1450To3500, vec![peak(3, 7, 9)])]).encode();

        assert_eq!(
            read_u32(&encoded, CHECKSUM_OFFSET),
            crc32fast::hash(&encoded[CHECKSUM_OFFSET + 4..]),
        );
    }

    #[test]
    fn a_peak_is_written_as_a_delta_from_the_previous_frame() {
        let encoded = fingerprint_of(vec![(
            FrequencyBand::Hz250To520,
            vec![peak(0, 0x1234, 0x5678), peak(7, 0x4321, 0x8765)],
        )])
        .encode();

        assert_eq!(
            &encoded[FIRST_BAND_OFFSET + 8..FIRST_BAND_OFFSET + 18],
            &[0, 0x34, 0x12, 0x78, 0x56, 7, 0x21, 0x43, 0x65, 0x87],
        );
    }

    #[test]
    fn a_gap_of_255_frames_or_more_is_escaped_with_an_absolute_frame_index() {
        // A delta of 255 would read back as the escape byte itself, and 300 does not fit
        //  a byte, so both write the frame index whole and a zero delta after it.
        for frame in [255_u32, 300] {
            let encoded = fingerprint_of(vec![(
                FrequencyBand::Hz250To520,
                vec![peak(0, 1, 2), peak(frame, 3, 4)],
            )])
            .encode();

            let mut expected = vec![0xff];
            expected.extend(frame.to_le_bytes());
            expected.extend([0, 3, 0, 4, 0]);

            assert_eq!(
                &encoded[FIRST_BAND_OFFSET + 13..FIRST_BAND_OFFSET + 23],
                expected.as_slice(),
            );
        }
    }

    #[test]
    #[should_panic(expected = "frame order")]
    fn peaks_out_of_frame_order_panic() {
        fingerprint_of(vec![(
            FrequencyBand::Hz250To520,
            vec![peak(5, 1, 2), peak(4, 3, 4)],
        )])
        .encode();
    }

    #[test]
    fn a_band_is_padded_to_a_multiple_of_four_bytes() {
        let encoded =
            fingerprint_of(vec![(FrequencyBand::Hz250To520, vec![peak(0, 1, 2)])]).encode();

        // One peak is five bytes, so three bytes of padding follow it.
        assert_eq!(read_u32(&encoded, FIRST_BAND_OFFSET + 4), 5);
        assert_eq!(encoded.len(), FIRST_BAND_OFFSET + 8 + 5 + 3);
        assert_eq!(&encoded[encoded.len() - 3..], &[0, 0, 0]);
    }

    #[test]
    fn bands_are_written_in_ascending_order_whatever_the_map_hands_back() {
        // Inserted highest band first, and `HashMap` iteration order is randomised per
        //  process anyway: the sort in `encode` is the only thing making the output
        //  stable, and the golden fingerprints depend on it.
        let encoded = fingerprint_of(vec![
            (FrequencyBand::Hz3500To5500, vec![peak(0, 1, 2)]),
            (FrequencyBand::Hz520To1450, vec![peak(0, 1, 2)]),
            (FrequencyBand::Hz1450To3500, vec![peak(0, 1, 2)]),
            (FrequencyBand::Hz250To520, vec![peak(0, 1, 2)]),
        ])
        .encode();

        assert_eq!(
            band_markers(&encoded),
            vec![0x60030040, 0x60030041, 0x60030042, 0x60030043],
        );
    }

    #[test]
    fn a_frequency_falls_in_the_band_whose_bounds_hold_it() {
        for (frequency_hz, band) in [
            (249, None),
            (250, Some(FrequencyBand::Hz250To520)),
            (519, Some(FrequencyBand::Hz250To520)),
            (520, Some(FrequencyBand::Hz520To1450)),
            (1450, Some(FrequencyBand::Hz1450To3500)),
            (3500, Some(FrequencyBand::Hz3500To5500)),
            (5500, Some(FrequencyBand::Hz3500To5500)),
            (5501, None),
        ] {
            assert_eq!(
                FrequencyBand::containing(frequency_hz),
                band,
                "{frequency_hz}"
            );
        }
    }

    #[test]
    fn the_uri_is_the_prefix_and_the_binary_survives_a_base64_round_trip() {
        let fingerprint =
            fingerprint_of(vec![(FrequencyBand::Hz520To1450, vec![peak(11, 22, 33)])]);

        let uri = fingerprint.to_data_uri();

        let payload = uri.strip_prefix(DATA_URI_PREFIX).unwrap();
        let decoded = general_purpose::STANDARD.decode(payload).unwrap();

        assert_eq!(decoded, fingerprint.encode());
    }
}
