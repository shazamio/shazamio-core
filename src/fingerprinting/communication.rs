use std::error::Error;
use std::time::SystemTime;

use crate::fingerprinting::signature_format::Fingerprint;
use crate::response::{Signature, SignatureSong};

const REQUEST_TIMEZONE: &str = "Europe/Paris";

// Whole epoch milliseconds: the client posts this field to the endpoint as it
//  stands, and fills it with `int(time.time() * 1000)` on its other code path. A
//  `u32` here wrapped it every 49.7 days, so the two disagreed.
//  https://github.com/shazamio/ShazamIO/blob/b5321b5c15d88ed98663420e63916704c6537512/shazamio/api.py#L548-L553
fn epoch_milliseconds() -> Result<u64, Box<dyn Error>> {
    let elapsed = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;

    Ok(u64::try_from(elapsed.as_millis())?)
}

pub fn get_signature_json(fingerprint: &Fingerprint) -> Result<Signature, Box<dyn Error>> {
    let song = SignatureSong {
        samples: fingerprint.duration_ms(),
        uri: fingerprint.to_data_uri(),
    };

    Ok(Signature {
        signature: song,
        timestamp: epoch_milliseconds()?,
        timezone: REQUEST_TIMEZONE.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn fingerprint(sample_count: u32) -> Fingerprint {
        Fingerprint {
            sample_rate_hz: 16000,
            sample_count,
            peaks_by_band: HashMap::new(),
        }
    }

    #[test]
    fn the_sample_count_is_reported_as_a_duration_in_milliseconds() {
        let fingerprint = fingerprint(24_500);

        let signature = get_signature_json(&fingerprint).unwrap();

        assert_eq!(signature.signature.samples, 1531);
        assert_eq!(signature.timezone, "Europe/Paris");
        assert_eq!(signature.signature.uri, fingerprint.to_data_uri());
    }

    #[test]
    fn the_duration_keeps_the_released_f32_truncation() {
        assert_eq!(fingerprint(4016).duration_ms(), 250);
    }

    #[test]
    fn the_timestamp_is_the_clock_in_whole_milliseconds() {
        let before = epoch_milliseconds().unwrap();
        let signature = get_signature_json(&fingerprint(16_000)).unwrap();
        let after = epoch_milliseconds().unwrap();

        assert!(signature.timestamp >= before);
        assert!(signature.timestamp <= after);

        // The cast this replaced kept the low 32 bits, so everything it produced sat
        //  below this bound, which a real clock passed in February 1970.
        assert!(signature.timestamp > u64::from(u32::MAX));
    }
}
