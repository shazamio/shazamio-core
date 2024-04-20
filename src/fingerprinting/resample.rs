use rubato::Resampler;
use rubato::SincFixedIn;
use rubato::SincInterpolationType;
use rubato::SincInterpolationParameters;
use symphonia::core::audio::SignalSpec;
use std::error::Error;

pub fn resample(spec: SignalSpec, samples: Vec<f32>) -> Result<Vec<i16>, Box<dyn Error>> {
    let target_rate = 16_000;
    let resample_ratio = target_rate as f64 / spec.rate as f64;
    let max_resample_ratio_relative = 2.0;
    let channel_count = spec.channels.count();
    let chunk_size = samples.len() / channel_count;
    let parameters = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Cubic,
        oversampling_factor: 160,
        window: rubato::WindowFunction::BlackmanHarris2,
    };
    let mut resampler = SincFixedIn::new(
        resample_ratio,
        max_resample_ratio_relative,
        parameters,
        chunk_size,
        1,
    )?;

    let mut mono_samples = vec![0f32; samples.len() / channel_count];
    for (i, sample) in samples.iter().enumerate() {
        mono_samples[i / channel_count] += sample / channel_count as f32;
    }

    let resampled_samples = resampler.process(&[&mono_samples], None)?;
    let result: Vec<i16> = resampled_samples[0]
        .iter()
        .map(|&sample| (sample * i16::MAX as f32) as i16)
        .collect();

    Ok(result)
}