use std::error::Error;
use std::time::SystemTime;

use crate::fingerprinting::signature_format::Fingerprint;
use crate::response::{Signature, SignatureSong};

pub fn get_signature_json(signature: &Fingerprint) -> Result<Signature, Box<dyn Error>> {
    // Epoch milliseconds, whole: the client posts this field to the endpoint as it
    //  stands, and fills it with `int(time.time() * 1000)` on its other code path. A
    //  `u32` here wrapped it every 49.7 days, so the two disagreed.
    //  https://github.com/shazamio/ShazamIO/blob/b5321b5c15d88ed98663420e63916704c6537512/shazamio/api.py#L548-L553
    let timestamp_ms = u64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_millis(),
    )?;
    let samples = (signature.sample_count as f32 / signature.sample_rate_hz as f32 * 1000.) as u32;
    Ok(Signature {
        signature: SignatureSong {
            samples,
            uri: signature.to_data_uri(),
        },
        timestamp: timestamp_ms,
        timezone: "Europe/Paris".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn epoch_milliseconds() -> u64 {
        u64::try_from(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    }

    #[test]
    fn the_sample_count_is_reported_as_a_duration_in_milliseconds() {
        let decoded = Fingerprint {
            sample_rate_hz: 16000,
            sample_count: 24_500,
            peaks_by_band: HashMap::new(),
        };
        let expected_uri = decoded.to_data_uri();

        let signature = get_signature_json(&decoded).unwrap();

        // `samples` names a duration, not a count: the sample count over the sample
        //  rate, truncated to whole milliseconds.
        assert_eq!(signature.signature.samples, 1531);

        assert_eq!(signature.timezone, "Europe/Paris");
        assert_eq!(signature.signature.uri, expected_uri);
    }

    #[test]
    fn the_timestamp_is_the_clock_in_whole_milliseconds() {
        let decoded = Fingerprint {
            sample_rate_hz: 16000,
            sample_count: 16_000,
            peaks_by_band: HashMap::new(),
        };

        let before = epoch_milliseconds();
        let signature = get_signature_json(&decoded).unwrap();
        let after = epoch_milliseconds();

        assert!(signature.timestamp >= before);
        assert!(signature.timestamp <= after);

        // The cast this replaced kept the low 32 bits, so everything it produced sat
        //  below this bound, which a real clock passed in February 1970.
        assert!(signature.timestamp > u64::from(u32::MAX));
    }
}
