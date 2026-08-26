use std::error::Error;
use std::time::SystemTime;

use crate::fingerprinting::signature_format::DecodedSignature;

#[derive(Debug)]
pub struct GeolocationResponse {
    pub(crate) altitude: i16,
    pub(crate) latitude: i8,
    pub(crate) longitude: i8,
}

#[derive(Debug)]
pub struct SignatureSong {
    pub(crate) samples: u32,
    pub(crate) timestamp: u32,
    pub(crate) uri: String,
}

#[derive(Debug)]
pub struct Signature {
    pub(crate) geolocation: GeolocationResponse,
    pub(crate) signature: SignatureSong,
    pub(crate) timestamp: u32,
    pub(crate) timezone: String,
}

pub fn get_signature_json(signature: &DecodedSignature) -> Result<Signature, Box<dyn Error>> {
    let timestamp_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis();
    let samples =
        (signature.number_samples as f32 / signature.sample_rate_hz as f32 * 1000.) as u32;
    Ok(Signature {
        geolocation: GeolocationResponse {
            altitude: 300,
            latitude: 45,
            longitude: 2,
        },
        signature: SignatureSong {
            samples,
            timestamp: timestamp_ms as u32,
            uri: signature.encode_to_uri()?,
        },
        timestamp: timestamp_ms as u32,
        timezone: "Europe/Paris".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn the_sample_count_is_reported_as_a_duration_in_milliseconds() {
        let decoded = DecodedSignature {
            sample_rate_hz: 16000,
            number_samples: 24_500,
            frequency_band_to_sound_peaks: HashMap::new(),
        };
        let expected_uri = decoded.encode_to_uri().unwrap();

        let signature = get_signature_json(&decoded).unwrap();

        // `samples` names a duration, not a count: the sample count over the sample
        //  rate, truncated to whole milliseconds.
        assert_eq!(signature.signature.samples, 1531);

        assert_eq!(signature.timestamp, signature.signature.timestamp);
        assert_eq!(signature.timezone, "Europe/Paris");
        assert_eq!(signature.geolocation.altitude, 300);
        assert_eq!(signature.geolocation.latitude, 45);
        assert_eq!(signature.geolocation.longitude, 2);
        assert_eq!(signature.signature.uri, expected_uri);
    }
}
