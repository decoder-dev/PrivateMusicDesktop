//! Audio DSP: biquad filters, the equalizer cascade with optional
//! compressor and loudness gain, mid/side stereo widening, and the silence
//! gate's peak meter. Behaviour mirrors the C implementation it replaces,
//! sample for sample.

use core::ffi::c_int;

/// Direct-form IIR coefficients (`PMBiquadCoeffs`).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct PMBiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

/// One filter step. `state` is exactly four floats: x1, x2, y1, y2.
#[inline(always)]
unsafe fn biquad_step(input: f32, c: &PMBiquadCoeffs, state: *mut f32) -> f32 {
    let x1 = *state;
    let x2 = *state.add(1);
    let y1 = *state.add(2);
    let y2 = *state.add(3);
    let output = c.b0 * input + c.b1 * x1 + c.b2 * x2 - c.a1 * y1 - c.a2 * y2;
    *state = input;
    *state.add(1) = x1;
    *state.add(2) = output;
    *state.add(3) = y1;
    output
}

#[inline(always)]
fn clamp_unit(value: f32) -> f32 {
    if value > 1.0 {
        1.0
    } else if value < -1.0 {
        -1.0
    } else {
        value
    }
}

#[no_mangle]
pub unsafe extern "C" fn pm_biquad_process(
    input: f32,
    coeffs: *const PMBiquadCoeffs,
    state: *mut f32,
) -> f32 {
    // The C version dereferenced unconditionally; a null here is a caller
    // bug, answered with the input untouched rather than a crash.
    if coeffs.is_null() || state.is_null() {
        return input;
    }
    biquad_step(input, &*coeffs, state)
}

#[no_mangle]
pub unsafe extern "C" fn pm_biquad_process_channel(
    samples: *mut f32,
    frame_count: c_int,
    stride: c_int,
    coeffs: *const PMBiquadCoeffs,
    state: *mut f32,
    clamp_output: bool,
) {
    if samples.is_null() || coeffs.is_null() || state.is_null() {
        return;
    }
    if frame_count <= 0 || stride <= 0 {
        return;
    }
    let local = *coeffs;
    let mut index: isize = 0;
    for _ in 0..frame_count {
        let mut value = biquad_step(*samples.offset(index), &local, state);
        if clamp_output {
            value = clamp_unit(value);
        }
        *samples.offset(index) = value;
        index += stride as isize;
    }
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pm_eq_process_channel(
    samples: *mut f32,
    frame_count: c_int,
    stride: c_int,
    bands: *const PMBiquadCoeffs,
    band_count: c_int,
    band_states: *mut f32,
    preamp: f32,
    drc_enabled: bool,
    compressor_threshold: f32,
    compressor_ratio: f32,
    compressor_makeup: f32,
    envelope_inout: *mut f32,
    loudness_enabled: bool,
    loudness_gain: f32,
) {
    if samples.is_null() || frame_count <= 0 || stride <= 0 {
        return;
    }
    if band_count > 0 && (bands.is_null() || band_states.is_null()) {
        return;
    }

    let mut envelope = if envelope_inout.is_null() {
        0.0
    } else {
        *envelope_inout
    };
    let mut index: isize = 0;

    for _ in 0..frame_count {
        let mut value = *samples.offset(index) * preamp;

        for band in 0..band_count as isize {
            let state = band_states.offset(band * 4);
            value = biquad_step(value, &*bands.offset(band), state);
        }

        if drc_enabled {
            let abs_val = value.abs();
            let target = if abs_val > compressor_threshold {
                compressor_threshold + (abs_val - compressor_threshold) / compressor_ratio
            } else {
                abs_val
            };
            let peak_gain = if abs_val > 0.001 {
                target / abs_val
            } else {
                1.0
            };
            envelope += (peak_gain - envelope) * 0.01;
            value *= envelope * compressor_makeup;
        }

        if loudness_enabled {
            value *= loudness_gain;
        }

        *samples.offset(index) = clamp_unit(value);
        index += stride as isize;
    }

    if !envelope_inout.is_null() {
        *envelope_inout = envelope;
    }
}

const MAXIMUM_SIDE_GAIN: f32 = 1.22;

#[inline(always)]
fn clamp_intensity(intensity: f32) -> f32 {
    if intensity < 0.0 {
        0.0
    } else if intensity > 1.0 {
        1.0
    } else {
        intensity
    }
}

#[inline(always)]
fn spatial_apply(left: f32, right: f32, side: f32, side_gain: f32, headroom: f32) -> (f32, f32) {
    let mid = (left + right) * 0.5;
    (
        (mid + side * side_gain) * headroom,
        (mid - side * side_gain) * headroom,
    )
}

#[no_mangle]
pub unsafe extern "C" fn pm_spatial_widen(
    left: f32,
    right: f32,
    intensity: f32,
    processed_side: f32,
    has_processed_side: bool,
    out_left: *mut f32,
    out_right: *mut f32,
) {
    if out_left.is_null() || out_right.is_null() {
        return;
    }
    let amount = clamp_intensity(intensity);
    if amount <= 0.0 {
        *out_left = left;
        *out_right = right;
        return;
    }
    let side = if has_processed_side {
        processed_side
    } else {
        (left - right) * 0.5
    };
    let side_gain = 1.0 + (MAXIMUM_SIDE_GAIN - 1.0) * amount;
    let headroom = 1.0 / side_gain;
    let (l, r) = spatial_apply(left, right, side, side_gain, headroom);
    *out_left = l;
    *out_right = r;
}

#[no_mangle]
pub unsafe extern "C" fn pm_spatial_process_planar(
    left: *mut f32,
    right: *mut f32,
    frame_count: c_int,
    intensity: f32,
    side_high_pass: *const PMBiquadCoeffs,
    has_side_high_pass: bool,
    side_state: *mut f32,
) {
    if left.is_null() || right.is_null() || frame_count <= 0 {
        return;
    }
    let amount = clamp_intensity(intensity);
    if amount <= 0.0 {
        return;
    }
    let filters_side = has_side_high_pass && !side_high_pass.is_null() && !side_state.is_null();
    let high_pass = if filters_side {
        *side_high_pass
    } else {
        PMBiquadCoeffs::default()
    };
    let side_gain = 1.0 + (MAXIMUM_SIDE_GAIN - 1.0) * amount;
    let headroom = 1.0 / side_gain;

    for frame in 0..frame_count as isize {
        let in_left = *left.offset(frame);
        let in_right = *right.offset(frame);
        let mut side = (in_left - in_right) * 0.5;
        if filters_side {
            side = biquad_step(side, &high_pass, side_state);
        }
        let (l, r) = spatial_apply(in_left, in_right, side, side_gain, headroom);
        *left.offset(frame) = l;
        *right.offset(frame) = r;
    }
}

#[no_mangle]
pub unsafe extern "C" fn pm_spatial_process_interleaved(
    samples: *mut f32,
    frame_count: c_int,
    stride: c_int,
    intensity: f32,
    side_high_pass: *const PMBiquadCoeffs,
    has_side_high_pass: bool,
    side_state: *mut f32,
) {
    if samples.is_null() || frame_count <= 0 || stride < 2 {
        return;
    }
    let amount = clamp_intensity(intensity);
    if amount <= 0.0 {
        return;
    }
    let filters_side = has_side_high_pass && !side_high_pass.is_null() && !side_state.is_null();
    let high_pass = if filters_side {
        *side_high_pass
    } else {
        PMBiquadCoeffs::default()
    };
    let side_gain = 1.0 + (MAXIMUM_SIDE_GAIN - 1.0) * amount;
    let headroom = 1.0 / side_gain;

    let mut index: isize = 0;
    for _ in 0..frame_count {
        let in_left = *samples.offset(index);
        let in_right = *samples.offset(index + 1);
        let mut side = (in_left - in_right) * 0.5;
        if filters_side {
            side = biquad_step(side, &high_pass, side_state);
        }
        let (l, r) = spatial_apply(in_left, in_right, side, side_gain, headroom);
        *samples.offset(index) = l;
        *samples.offset(index + 1) = r;
        index += stride as isize;
    }
}

#[no_mangle]
pub unsafe extern "C" fn pm_buffer_peak_magnitude(samples: *const f32, count: i32) -> f32 {
    if samples.is_null() || count <= 0 {
        return 0.0;
    }
    let mut peak = 0.0f32;
    for i in 0..count as isize {
        let magnitude = (*samples.offset(i)).abs();
        if magnitude > peak {
            peak = magnitude;
        }
    }
    peak
}

// ------------------------------------------------- filter design (RBJ cookbook)

fn to_coeffs(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> PMBiquadCoeffs {
    PMBiquadCoeffs { b0: b0 as f32, b1: b1 as f32, b2: b2 as f32, a1: a1 as f32, a2: a2 as f32 }
}

fn max_f64(a: f64, b: f64) -> f64 {
    // Swift's `max`/`min` return the second operand unless the first wins
    // outright, which matters only for NaN; mirror that.
    if b > a { b } else { a }
}

fn min_f64(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// Peaking equalizer section (Q = 1.15), frequency capped at 0.45 * rate.
#[no_mangle]
pub extern "C" fn pm_eq_peaking_coefficients(frequency: f64, gain_db: f64, sample_rate: f64) -> PMBiquadCoeffs {
    let safe_frequency = min_f64(frequency, sample_rate * 0.45);
    let amplitude = 10f64.powf(gain_db / 40.0);
    let omega = 2.0 * core::f64::consts::PI * safe_frequency / sample_rate;
    let alpha = omega.sin() / (2.0 * 1.15);
    let cosine = omega.cos();
    let a0 = 1.0 + alpha / amplitude;
    to_coeffs(
        (1.0 + alpha * amplitude) / a0,
        (-2.0 * cosine) / a0,
        (1.0 - alpha * amplitude) / a0,
        (-2.0 * cosine) / a0,
        (1.0 - alpha / amplitude) / a0,
    )
}

/// Low shelf with slope 1, frequency clamped to 20 Hz ... 0.45 * rate.
#[no_mangle]
pub extern "C" fn pm_eq_low_shelf_coefficients(frequency: f64, gain_db: f64, sample_rate: f64) -> PMBiquadCoeffs {
    let safe_frequency = min_f64(max_f64(frequency, 20.0), sample_rate * 0.45);
    let amplitude = 10f64.powf(gain_db / 40.0);
    let omega = 2.0 * core::f64::consts::PI * safe_frequency / sample_rate;
    let cosine = omega.cos();
    let sine = omega.sin();
    let beta = amplitude.sqrt() / 1.0;
    let a0 = (amplitude + 1.0) + (amplitude - 1.0) * cosine + beta * sine;
    to_coeffs(
        (amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cosine + beta * sine)) / a0,
        (2.0 * amplitude * ((amplitude - 1.0) - (amplitude + 1.0) * cosine)) / a0,
        (amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cosine - beta * sine)) / a0,
        (-2.0 * ((amplitude - 1.0) + (amplitude + 1.0) * cosine)) / a0,
        ((amplitude + 1.0) + (amplitude - 1.0) * cosine - beta * sine) / a0,
    )
}

/// Second-order Butterworth high-pass, frequency clamped like the shelf.
#[no_mangle]
pub extern "C" fn pm_eq_high_pass_coefficients(frequency: f64, sample_rate: f64) -> PMBiquadCoeffs {
    let safe_frequency = min_f64(max_f64(frequency, 20.0), sample_rate * 0.45);
    let omega = 2.0 * core::f64::consts::PI * safe_frequency / sample_rate;
    let cosine = omega.cos();
    let sine = omega.sin();
    let alpha = sine / (2.0 * (1.0 / 2.0f64.sqrt()));
    let a0 = 1.0 + alpha;
    to_coeffs(
        ((1.0 + cosine) / 2.0) / a0,
        (-(1.0 + cosine)) / a0,
        ((1.0 + cosine) / 2.0) / a0,
        (-2.0 * cosine) / a0,
        (1.0 - alpha) / a0,
    )
}
