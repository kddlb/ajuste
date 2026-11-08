//! RBJ "Audio EQ Cookbook" biquads, matching what Adjust Pro computes client-side.

use crate::eq::{EqPoint, FilterType};

/// Sample rates the headset DSP runs at. The discriminant is the bit used on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SampleRate {
    S44k = 1,
    S48k = 2,
    S96k = 4,
}

impl SampleRate {
    /// In the order Adjust Pro sends coefficients.
    pub const ALL: [SampleRate; 3] = [SampleRate::S44k, SampleRate::S48k, SampleRate::S96k];
    /// Mask sent in the SET_EQ_SETTINGS header (all three).
    pub const MASK_ALL: u8 = 7;

    pub fn code(self) -> u8 {
        self as u8
    }

    /// Note: the app uses 44000, not 44100, for the "44k" rate.
    pub fn hz(self) -> f64 {
        match self {
            SampleRate::S44k => 44_000.0,
            SampleRate::S48k => 48_000.0,
            SampleRate::S96k => 96_000.0,
        }
    }
}

/// Normalised biquad (a0 = 1): `H(z) = (b0 + b1 z^-1 + b2 z^-2) / (1 + a1 z^-1 + a2 z^-2)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefs {
    pub b: [f32; 3],
    pub a: [f32; 2],
}

impl Coefs {
    /// Pass-through filters the app sends for an empty profile (b == [1, a1, a2]).
    /// Values copied verbatim from the bundle.
    #[allow(clippy::excessive_precision)]
    pub fn passthrough(rate: SampleRate) -> Coefs {
        let (a1, a2) = match rate {
            SampleRate::S44k => (-1.862_065_96, 0.866_822_41),
            SampleRate::S48k => (-1.873_204_42, 0.877_223_70),
            SampleRate::S96k => (-1.935_598_18, 0.936_635_08),
        };
        Coefs { b: [1.0, a1, a2], a: [a1, a2] }
    }

    /// Linear magnitude |H(e^jw)| at `freq` Hz.
    pub fn magnitude(&self, freq: f64, fs: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / fs;
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let [b0, b1, b2] = self.b.map(f64::from);
        let [a1, a2] = self.a.map(f64::from);
        let nr = b0 + b1 * c1 + b2 * c2;
        let ni = -(b1 * s1 + b2 * s2);
        let dr = 1.0 + a1 * c1 + a2 * c2;
        let di = -(a1 * s1 + a2 * s2);
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

/// Design one filter for one sample rate.
pub fn design(p: &EqPoint, rate: SampleRate) -> Coefs {
    design_f64(p.filter, f64::from(p.freq), f64::from(p.gain), f64::from(p.q), rate.hz())
}

pub fn design_f64(filter: FilterType, f: f64, gain_db: f64, q: f64, fs: f64) -> Coefs {
    let w0 = 2.0 * std::f64::consts::PI * f / fs;
    let (cw, sw) = (w0.cos(), w0.sin());
    let alpha = sw / (2.0 * q);
    let a = 10f64.powf(gain_db / 40.0);
    let sa = a.sqrt();

    let (b0, b1, b2, a0, a1, a2) = match filter {
        FilterType::Peak => (1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a),
        FilterType::LowShelf => (
            a * ((a + 1.0) - (a - 1.0) * cw + 2.0 * sa * alpha),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
            a * ((a + 1.0) - (a - 1.0) * cw - 2.0 * sa * alpha),
            (a + 1.0) + (a - 1.0) * cw + 2.0 * sa * alpha,
            -2.0 * ((a - 1.0) + (a + 1.0) * cw),
            (a + 1.0) + (a - 1.0) * cw - 2.0 * sa * alpha,
        ),
        FilterType::HighShelf => (
            a * ((a + 1.0) + (a - 1.0) * cw + 2.0 * sa * alpha),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
            a * ((a + 1.0) + (a - 1.0) * cw - 2.0 * sa * alpha),
            (a + 1.0) - (a - 1.0) * cw + 2.0 * sa * alpha,
            2.0 * ((a - 1.0) - (a + 1.0) * cw),
            (a + 1.0) - (a - 1.0) * cw - 2.0 * sa * alpha,
        ),
        FilterType::LowPass => ((1.0 - cw) / 2.0, 1.0 - cw, (1.0 - cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
        FilterType::HighPass => ((1.0 + cw) / 2.0, -(1.0 + cw), (1.0 + cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
        FilterType::Notch => (1.0, -2.0 * cw, 1.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
        FilterType::BandPass => (sw / 2.0, 0.0, -sw / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
    };
    Coefs {
        b: [(b0 / a0) as f32, (b1 / a0) as f32, (b2 / a0) as f32],
        a: [(a1 / a0) as f32, (a2 / a0) as f32],
    }
}

/// Combined response of a filter chain in dB at `freq`.
pub fn chain_db(points: &[EqPoint], freq: f64, rate: SampleRate) -> f64 {
    let mag: f64 = points.iter().map(|p| design(p, rate).magnitude(freq, rate.hz())).product();
    20.0 * mag.log10()
}
