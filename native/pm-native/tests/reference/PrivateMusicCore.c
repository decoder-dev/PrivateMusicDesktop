#include "PrivateMusicCore.h"

#include <float.h>
#include <string.h>

#pragma mark - Hashing

const uint64_t pm_hash_fnv1a64_basis = 0xcbf29ce484222325ULL;

static const uint64_t pm_hash_fnv1a64_prime = 0x100000001b3ULL;
/// Stand-in for a hash that lands on 0, keeping 0 free as the empty sentinel.
static const uint64_t pm_hash_zero_replacement = 0x9e3779b97f4a7c15ULL;

uint64_t pm_hash_fnv1a64_value(uint64_t hash, uint64_t value) {
    return (hash ^ value) * pm_hash_fnv1a64_prime;
}

uint64_t pm_hash_fnv1a64_bytes(
    uint64_t hash,
    const uint8_t *bytes,
    int32_t length
) {
    if (bytes == NULL || length <= 0) {
        return hash;
    }
    for (int32_t index = 0; index < length; ++index) {
        hash = (hash ^ (uint64_t)bytes[index]) * pm_hash_fnv1a64_prime;
    }
    return hash;
}

uint64_t pm_text_identity_hash(const uint8_t *bytes, int32_t length) {
    if (bytes == NULL || length <= 0) {
        return 0;
    }
    const uint64_t hash = pm_hash_fnv1a64_bytes(
        pm_hash_fnv1a64_basis,
        bytes,
        length
    );
    return hash == 0 ? pm_hash_zero_replacement : hash;
}

#pragma mark - Text folding and search

/// Fold one Cyrillic scalar the way the platform folder does for Russian:
/// lowercase, then strip the marks that decompose out of `ё` and `й`.
/// Returns false for the extended Cyrillic letters this fast path declines.
static bool pm_fold_cyrillic_scalar(uint32_t scalar, uint32_t *folded) {
    if (scalar == 0x0401 || scalar == 0x0451) {
        *folded = 0x0435;
        return true;
    }
    if (scalar == 0x0419 || scalar == 0x0439) {
        *folded = 0x0438;
        return true;
    }
    if (scalar >= 0x0410 && scalar <= 0x042F) {
        *folded = scalar + 0x20;
        return true;
    }
    if (scalar >= 0x0430 && scalar <= 0x044F) {
        *folded = scalar;
        return true;
    }
    return false;
}

int32_t pm_text_fold_utf8(
    const uint8_t *input,
    int32_t length,
    uint8_t *out,
    int32_t out_capacity
) {
    if (length < 0 || out_capacity < 0) {
        return PM_TEXT_FOLD_UNSUPPORTED;
    }
    if (length == 0) {
        return 0;
    }
    if (input == NULL) {
        return PM_TEXT_FOLD_UNSUPPORTED;
    }
    if (out == NULL) {
        return PM_TEXT_FOLD_OVERFLOW;
    }

    int32_t written = 0;
    int32_t index = 0;
    while (index < length) {
        const uint8_t lead = input[index];
        if (lead < 0x80) {
            if (written >= out_capacity) {
                return PM_TEXT_FOLD_OVERFLOW;
            }
            out[written] = (lead >= 'A' && lead <= 'Z')
                ? (uint8_t)(lead + ('a' - 'A'))
                : lead;
            written += 1;
            index += 1;
            continue;
        }

        if ((lead != 0xD0 && lead != 0xD1) || index + 1 >= length) {
            return PM_TEXT_FOLD_UNSUPPORTED;
        }
        const uint8_t trail = input[index + 1];
        if ((trail & 0xC0) != 0x80) {
            return PM_TEXT_FOLD_UNSUPPORTED;
        }
        const uint32_t scalar =
            ((uint32_t)(lead & 0x1F) << 6) | (uint32_t)(trail & 0x3F);
        uint32_t folded = 0;
        if (!pm_fold_cyrillic_scalar(scalar, &folded)) {
            return PM_TEXT_FOLD_UNSUPPORTED;
        }
        if (written + 2 > out_capacity) {
            return PM_TEXT_FOLD_OVERFLOW;
        }
        out[written] = (uint8_t)(0xC0 | (folded >> 6));
        out[written + 1] = (uint8_t)(0x80 | (folded & 0x3F));
        written += 2;
        index += 2;
    }
    return written;
}

int32_t pm_text_find(
    const uint8_t *haystack,
    int32_t haystack_length,
    const uint8_t *needle,
    int32_t needle_length
) {
    if (needle == NULL || needle_length <= 0) {
        return -1;
    }
    if (haystack == NULL || haystack_length < needle_length) {
        return -1;
    }

    const uint8_t first = needle[0];
    const int32_t last_start = haystack_length - needle_length;
    int32_t start = 0;
    while (start <= last_start) {
        const uint8_t *hit = memchr(
            haystack + start,
            first,
            (size_t)(last_start - start + 1)
        );
        if (hit == NULL) {
            return -1;
        }
        const int32_t offset = (int32_t)(hit - haystack);
        if (memcmp(hit, needle, (size_t)needle_length) == 0) {
            return offset;
        }
        start = offset + 1;
    }
    return -1;
}

static bool pm_is_ascii_whitespace(uint8_t byte) {
    return byte == ' '
        || byte == '\t'
        || byte == '\n'
        || byte == '\r'
        || byte == '\v'
        || byte == '\f';
}

int32_t pm_text_normalize_identity(
    const uint8_t *input,
    int32_t length,
    uint8_t *out,
    int32_t out_capacity
) {
    const int32_t folded = pm_text_fold_utf8(
        input,
        length,
        out,
        out_capacity
    );
    if (folded < 0) {
        return folded;
    }
    int32_t start = 0;
    int32_t end = folded;
    while (start < end && pm_is_ascii_whitespace(out[start])) {
        start += 1;
    }
    while (end > start && pm_is_ascii_whitespace(out[end - 1])) {
        end -= 1;
    }
    if (start > 0) {
        memmove(out, out + start, (size_t)(end - start));
    }
    return end - start;
}

static float pm_saturation(float red, float green, float blue) {
    const float high = red > green
        ? (red > blue ? red : blue)
        : (green > blue ? green : blue);
    const float low = red < green
        ? (red < blue ? red : blue)
        : (green < blue ? green : blue);
    if (high <= 0.0f) {
        return 0.0f;
    }
    return (high - low) / high;
}

bool pm_artwork_extract_tint_rgba(
    const uint8_t *rgba,
    int32_t length,
    PMArtworkTint *out
) {
    if (out == NULL || rgba == NULL || length < 4 || (length % 4) != 0) {
        return false;
    }

    double weighted_red = 0.0;
    double weighted_green = 0.0;
    double weighted_blue = 0.0;
    double total_weight = 0.0;

    for (int32_t pixel = 0; pixel < length; pixel += 4) {
        const double alpha = (double)rgba[pixel + 3] / 255.0;
        if (alpha <= 0.4) {
            continue;
        }
        const float red = (float)((double)rgba[pixel] / 255.0);
        const float green = (float)((double)rgba[pixel + 1] / 255.0);
        const float blue = (float)((double)rgba[pixel + 2] / 255.0);
        const float saturation = pm_saturation(red, green, blue);
        const double weight = alpha * (0.08 + (double)saturation);
        weighted_red += (double)red * weight;
        weighted_green += (double)green * weight;
        weighted_blue += (double)blue * weight;
        total_weight += weight;
    }

    if (total_weight <= 0.0) {
        return false;
    }

    out->red = (float)(weighted_red / total_weight);
    out->green = (float)(weighted_green / total_weight);
    out->blue = (float)(weighted_blue / total_weight);
    return true;
}

#pragma mark - Mix queue ranking

static bool pm_mix_history_contains(
    const uint64_t *history_hashes,
    int32_t history_count,
    uint64_t artist_hash
) {
    if (history_hashes == NULL || history_count <= 0) {
        return false;
    }
    int32_t low = 0;
    int32_t high = history_count - 1;
    while (low <= high) {
        const int32_t middle = low + (high - low) / 2;
        const uint64_t value = history_hashes[middle];
        if (value == artist_hash) {
            return true;
        }
        if (value < artist_hash) {
            low = middle + 1;
        } else {
            high = middle - 1;
        }
    }
    return false;
}

/// Penalize an artist that just played, which is what stopped mix radio from
/// looping the same handful of picks.
static double pm_mix_spacing_bonus(
    uint64_t artist_hash,
    const uint64_t *recent_hashes,
    int32_t recent_count,
    double immediate,
    double near,
    double window,
    double fresh
) {
    if (artist_hash == 0) {
        return 0.0;
    }
    if (recent_hashes == NULL || recent_count <= 0) {
        return fresh;
    }
    if (recent_hashes[recent_count - 1] == artist_hash) {
        return immediate;
    }
    const int32_t near_start = recent_count >= 2 ? recent_count - 2 : 0;
    for (int32_t index = near_start; index < recent_count; ++index) {
        if (recent_hashes[index] == artist_hash) {
            return near;
        }
    }
    for (int32_t index = 0; index < near_start; ++index) {
        if (recent_hashes[index] == artist_hash) {
            return window;
        }
    }
    return fresh;
}

double pm_mix_score_candidate(
    PMMixCandidate candidate,
    double jitter,
    const PMMixRankContext *context,
    const uint64_t *recent_hashes,
    int32_t recent_count
) {
    if (context == NULL) {
        return 0.0;
    }

    const uint64_t artist = candidate.artist_hash;
    const uint64_t album = candidate.album_hash;
    const uint64_t seed_artist = context->seed_artist_hash;
    const uint64_t seed_album = context->seed_album_hash;
    const bool in_history = pm_mix_history_contains(
        context->history_hashes,
        context->history_count,
        artist
    );
    double value = 0.0;

    if (context->mode == PM_MIX_MODE_CLOSER_TO_SEED) {
        if (seed_artist != 0 && artist == seed_artist) {
            value += 42.0;
        }
        if (seed_album != 0 && album == seed_album) {
            value += 18.0;
        }
        if (in_history) {
            value += 6.0;
        }
        if (seed_artist != 0 && artist != seed_artist) {
            value += 4.0;
        }
        // Mild spacing only — affinity to the seed artist must still win the
        // first pick after the current track.
        value += pm_mix_spacing_bonus(
            artist,
            recent_hashes,
            recent_count,
            -16.0,
            -8.0,
            -3.0,
            0.0
        );
    } else if (context->mode == PM_MIX_MODE_MORE_NOVEL) {
        if (seed_artist != 0 && artist != seed_artist) {
            value += 32.0;
        }
        if (seed_artist != 0 && artist == seed_artist) {
            value -= 18.0;
        }
        if (in_history) {
            value -= 16.0;
        } else {
            value += 26.0;
        }
        if (seed_album != 0 && album != seed_album) {
            value += 8.0;
        }
        value += pm_mix_spacing_bonus(
            artist,
            recent_hashes,
            recent_count,
            -55.0,
            -28.0,
            -12.0,
            12.0
        );
    } else {
        return 0.0;
    }

    return value + jitter;
}

int32_t pm_mix_select_best(
    const PMMixCandidate *candidates,
    const double *jitter,
    int32_t count,
    const PMMixRankContext *context,
    const uint64_t *recent_hashes,
    int32_t recent_count
) {
    if (candidates == NULL || jitter == NULL || context == NULL) {
        return 0;
    }
    if (count <= 0) {
        return 0;
    }

    int32_t best_index = 0;
    double best_score = -DBL_MAX;
    for (int32_t index = 0; index < count; ++index) {
        const double value = pm_mix_score_candidate(
            candidates[index],
            jitter[index],
            context,
            recent_hashes,
            recent_count
        );
        if (value > best_score) {
            best_score = value;
            best_index = index;
        }
    }
    return best_index;
}

#pragma mark - Buffer health estimator

/// Ticks closer together than this (or non-monotonic) are dropped instead of
/// dividing by a near-zero interval.
static const double pm_buffer_health_min_interval = 1e-6;

void pm_buffer_health_reset(pm_buffer_health *state) {
    if (state == NULL) {
        return;
    }
    memset(state, 0, sizeof(*state));
}

void pm_buffer_health_observe(
    pm_buffer_health *state,
    double now_seconds,
    double loaded_ahead_seconds
) {
    if (state == NULL) {
        return;
    }

    if (!state->has_previous) {
        state->previous_timestamp = now_seconds;
        state->previous_loaded_ahead = loaded_ahead_seconds;
        state->latest_loaded_ahead = loaded_ahead_seconds;
        state->has_previous = true;
        return;
    }

    const double elapsed = now_seconds - state->previous_timestamp;
    if (elapsed <= pm_buffer_health_min_interval) {
        // Non-monotonic or duplicate tick: keep the latest reading for
        // predicted-underrun purposes, but do not fold it into the rolling
        // rate — the previous tick stays the basis for the next real delta.
        state->latest_loaded_ahead = loaded_ahead_seconds;
        return;
    }

    double rate = 1.0
        + (loaded_ahead_seconds - state->previous_loaded_ahead) / elapsed;
    if (rate < 0.0) {
        rate = 0.0;
    }

    const int32_t index = state->next_index;
    if (state->rate_count == PM_BUFFER_HEALTH_WINDOW_CAPACITY) {
        state->rate_sum -= state->rate_samples[index];
    } else {
        state->rate_count += 1;
    }
    state->rate_samples[index] = rate;
    state->rate_sum += rate;
    state->next_index = (index + 1) % PM_BUFFER_HEALTH_WINDOW_CAPACITY;

    state->previous_timestamp = now_seconds;
    state->previous_loaded_ahead = loaded_ahead_seconds;
    state->latest_loaded_ahead = loaded_ahead_seconds;
}

double pm_buffer_health_throughput(const pm_buffer_health *state) {
    if (state == NULL || state->rate_count == 0) {
        return 1.0;
    }
    return state->rate_sum / (double)state->rate_count;
}

double pm_buffer_health_predicted_underrun(
    const pm_buffer_health *state,
    double play_rate
) {
    if (state == NULL || !state->has_previous || play_rate <= 0.0) {
        return PM_BUFFER_HEALTH_UNDERRUN_NONE;
    }

    const double drain_rate = play_rate - pm_buffer_health_throughput(state);
    if (drain_rate <= 0.0) {
        return PM_BUFFER_HEALTH_UNDERRUN_NONE;
    }
    if (state->latest_loaded_ahead <= 0.0) {
        return 0.0;
    }
    return state->latest_loaded_ahead / drain_rate;
}
