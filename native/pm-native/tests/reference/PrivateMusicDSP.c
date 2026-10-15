#include "PrivateMusicDSP.h"

#include <Accelerate/Accelerate.h>
#include <math.h>

/// Inlinable twin of `pm_biquad_process` so the per-sample loops below do not
/// pay a call for every frame.
static inline float pm_biquad_step(
    float input,
    const PMBiquadCoeffs *coeffs,
    float *state
) {
    const float x1 = state[0];
    const float x2 = state[1];
    const float y1 = state[2];
    const float y2 = state[3];
    const float output = coeffs->b0 * input
        + coeffs->b1 * x1
        + coeffs->b2 * x2
        - coeffs->a1 * y1
        - coeffs->a2 * y2;
    state[0] = input;
    state[1] = x1;
    state[2] = output;
    state[3] = y1;
    return output;
}

float pm_biquad_process(
    float input,
    const PMBiquadCoeffs *coeffs,
    float *state
) {
    return pm_biquad_step(input, coeffs, state);
}

void pm_biquad_process_channel(
    float *samples,
    int frame_count,
    int stride,
    const PMBiquadCoeffs *coeffs,
    float *state,
    bool clamp_output
) {
    if (samples == NULL || coeffs == NULL || state == NULL) {
        return;
    }
    if (frame_count <= 0 || stride <= 0) {
        return;
    }

    const PMBiquadCoeffs local = *coeffs;
    int sample_index = 0;
    for (int frame = 0; frame < frame_count; ++frame) {
        float value = pm_biquad_step(samples[sample_index], &local, state);
        if (clamp_output) {
            if (value > 1.0f) {
                value = 1.0f;
            } else if (value < -1.0f) {
                value = -1.0f;
            }
        }
        samples[sample_index] = value;
        sample_index += stride;
    }
}

void pm_eq_process_channel(
    float *samples,
    int frame_count,
    int stride,
    const PMBiquadCoeffs *bands,
    int band_count,
    float *band_states,
    float preamp,
    bool drc_enabled,
    float compressor_threshold,
    float compressor_ratio,
    float compressor_makeup,
    float *envelope_inout,
    bool loudness_enabled,
    float loudness_gain
) {
    if (samples == NULL || frame_count <= 0 || stride <= 0) {
        return;
    }
    if (band_count > 0 && (bands == NULL || band_states == NULL)) {
        return;
    }

    float envelope = envelope_inout != NULL ? *envelope_inout : 0.0f;
    int sample_index = 0;

    for (int frame = 0; frame < frame_count; ++frame) {
        float value = samples[sample_index] * preamp;

        for (int band = 0; band < band_count; ++band) {
            float *state = band_states + (band * 4);
            value = pm_biquad_process(value, &bands[band], state);
        }

        if (drc_enabled) {
            const float abs_val = fabsf(value);
            const float target = abs_val > compressor_threshold
                ? compressor_threshold
                    + (abs_val - compressor_threshold) / compressor_ratio
                : abs_val;
            const float peak_gain = abs_val > 0.001f
                ? target / abs_val
                : 1.0f;
            envelope += (peak_gain - envelope) * 0.01f;
            value *= envelope * compressor_makeup;
        }

        if (loudness_enabled) {
            value *= loudness_gain;
        }

        if (value > 1.0f) {
            value = 1.0f;
        } else if (value < -1.0f) {
            value = -1.0f;
        }

        samples[sample_index] = value;
        sample_index += stride;
    }

    if (envelope_inout != NULL) {
        *envelope_inout = envelope;
    }
}

static const float pm_spatial_maximum_side_gain = 1.22f;

static inline float pm_spatial_clamp_intensity(float intensity) {
    if (intensity < 0.0f) {
        return 0.0f;
    }
    if (intensity > 1.0f) {
        return 1.0f;
    }
    return intensity;
}

/// Mid/side recombination shared by the single-sample and buffer entry points.
/// `side_gain` / `headroom` only depend on the intensity, so the loops hoist
/// them out and call this per frame.
static inline void pm_spatial_apply(
    float left,
    float right,
    float side,
    float side_gain,
    float headroom,
    float *out_left,
    float *out_right
) {
    const float mid = (left + right) * 0.5f;
    *out_left = (mid + side * side_gain) * headroom;
    *out_right = (mid - side * side_gain) * headroom;
}

void pm_spatial_widen(
    float left,
    float right,
    float intensity,
    float processed_side,
    bool has_processed_side,
    float *out_left,
    float *out_right
) {
    if (out_left == NULL || out_right == NULL) {
        return;
    }

    const float amount = pm_spatial_clamp_intensity(intensity);
    if (amount <= 0.0f) {
        *out_left = left;
        *out_right = right;
        return;
    }

    const float side = has_processed_side
        ? processed_side
        : (left - right) * 0.5f;
    const float side_gain =
        1.0f + (pm_spatial_maximum_side_gain - 1.0f) * amount;
    const float headroom = 1.0f / side_gain;
    pm_spatial_apply(left, right, side, side_gain, headroom, out_left, out_right);
}

void pm_spatial_process_planar(
    float *left,
    float *right,
    int frame_count,
    float intensity,
    const PMBiquadCoeffs *side_high_pass,
    bool has_side_high_pass,
    float *side_state
) {
    if (left == NULL || right == NULL || frame_count <= 0) {
        return;
    }
    const float amount = pm_spatial_clamp_intensity(intensity);
    if (amount <= 0.0f) {
        return;
    }
    const bool filters_side =
        has_side_high_pass && side_high_pass != NULL && side_state != NULL;
    const PMBiquadCoeffs high_pass = filters_side
        ? *side_high_pass
        : (PMBiquadCoeffs){0};
    const float side_gain =
        1.0f + (pm_spatial_maximum_side_gain - 1.0f) * amount;
    const float headroom = 1.0f / side_gain;

    for (int frame = 0; frame < frame_count; ++frame) {
        const float in_left = left[frame];
        const float in_right = right[frame];
        float side = (in_left - in_right) * 0.5f;
        if (filters_side) {
            side = pm_biquad_step(side, &high_pass, side_state);
        }
        pm_spatial_apply(
            in_left,
            in_right,
            side,
            side_gain,
            headroom,
            &left[frame],
            &right[frame]
        );
    }
}

void pm_spatial_process_interleaved(
    float *samples,
    int frame_count,
    int stride,
    float intensity,
    const PMBiquadCoeffs *side_high_pass,
    bool has_side_high_pass,
    float *side_state
) {
    if (samples == NULL || frame_count <= 0 || stride < 2) {
        return;
    }
    const float amount = pm_spatial_clamp_intensity(intensity);
    if (amount <= 0.0f) {
        return;
    }
    const bool filters_side =
        has_side_high_pass && side_high_pass != NULL && side_state != NULL;
    const PMBiquadCoeffs high_pass = filters_side
        ? *side_high_pass
        : (PMBiquadCoeffs){0};
    const float side_gain =
        1.0f + (pm_spatial_maximum_side_gain - 1.0f) * amount;
    const float headroom = 1.0f / side_gain;

    int index = 0;
    for (int frame = 0; frame < frame_count; ++frame) {
        const float in_left = samples[index];
        const float in_right = samples[index + 1];
        float side = (in_left - in_right) * 0.5f;
        if (filters_side) {
            side = pm_biquad_step(side, &high_pass, side_state);
        }
        pm_spatial_apply(
            in_left,
            in_right,
            side,
            side_gain,
            headroom,
            &samples[index],
            &samples[index + 1]
        );
        index += stride;
    }
}

float pm_buffer_peak_magnitude(const float *samples, int32_t count) {
    if (samples == NULL || count <= 0) {
        return 0.0f;
    }
    float peak = 0.0f;
    vDSP_maxmgv(samples, 1, &peak, (vDSP_Length)count);
    return peak;
}
