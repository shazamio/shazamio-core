pub mod algorithm;
pub mod communication;
pub mod decode;
pub mod hanning;
pub mod opus_decoder;
pub mod resample;
pub mod signature_format;

// The probes are the ones `tests/` uses; `tests/data/generate.sh` regenerates them.
#[cfg(test)]
pub(crate) const DATA_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");

#[cfg(test)]
pub(crate) fn probe_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(DATA_DIRECTORY).join(name)
}
