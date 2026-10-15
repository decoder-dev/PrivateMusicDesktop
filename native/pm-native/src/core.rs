//! Hashing, text folding and search, artwork tint, mix ranking and the
//! buffer-health estimator. Behaviour mirrors the C implementation it
//! replaces (kept under `tests/reference/` and compared bit for bit).

use core::ffi::c_void;
use core::slice;

// ---------------------------------------------------------------- hashing

#[no_mangle]
pub static pm_hash_fnv1a64_basis: u64 = 0xcbf2_9ce4_8422_2325;

const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// Stand-in for a hash that lands on 0, keeping 0 free as the empty sentinel.
const ZERO_REPLACEMENT: u64 = 0x9e37_79b9_7f4a_7c15;

#[no_mangle]
pub extern "C" fn pm_hash_fnv1a64_value(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(FNV_PRIME)
}

#[inline]
fn fnv_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
    }
    hash
}

#[no_mangle]
pub unsafe extern "C" fn pm_hash_fnv1a64_bytes(hash: u64, bytes: *const u8, length: i32) -> u64 {
    if bytes.is_null() || length <= 0 {
        return hash;
    }
    fnv_bytes(hash, slice::from_raw_parts(bytes, length as usize))
}

#[no_mangle]
pub unsafe extern "C" fn pm_text_identity_hash(bytes: *const u8, length: i32) -> u64 {
    if bytes.is_null() || length <= 0 {
        return 0;
    }
    let hash = fnv_bytes(
        pm_hash_fnv1a64_basis,
        slice::from_raw_parts(bytes, length as usize),
    );
    if hash == 0 {
        ZERO_REPLACEMENT
    } else {
        hash
    }
}

// --------------------------------------------------- text folding and search

pub const PM_TEXT_FOLD_UNSUPPORTED: i32 = -1;
pub const PM_TEXT_FOLD_OVERFLOW: i32 = -2;

/// Fold one Cyrillic scalar the way the platform folder does for Russian:
/// lowercase, then strip the marks that decompose out of `ё` and `й`.
fn fold_cyrillic_scalar(scalar: u32) -> Option<u32> {
    match scalar {
        0x0401 | 0x0451 => Some(0x0435),
        0x0419 | 0x0439 => Some(0x0438),
        0x0410..=0x042F => Some(scalar + 0x20),
        0x0430..=0x044F => Some(scalar),
        _ => None,
    }
}

#[no_mangle]
pub unsafe extern "C" fn pm_text_fold_utf8(
    input: *const u8,
    length: i32,
    out: *mut u8,
    out_capacity: i32,
) -> i32 {
    if length < 0 || out_capacity < 0 {
        return PM_TEXT_FOLD_UNSUPPORTED;
    }
    if length == 0 {
        return 0;
    }
    if input.is_null() {
        return PM_TEXT_FOLD_UNSUPPORTED;
    }
    if out.is_null() {
        return PM_TEXT_FOLD_OVERFLOW;
    }
    let input = slice::from_raw_parts(input, length as usize);
    let capacity = out_capacity as usize;

    let mut written = 0usize;
    let mut index = 0usize;
    while index < input.len() {
        let lead = input[index];
        if lead < 0x80 {
            if written >= capacity {
                return PM_TEXT_FOLD_OVERFLOW;
            }
            *out.add(written) = lead.to_ascii_lowercase();
            written += 1;
            index += 1;
            continue;
        }

        if (lead != 0xD0 && lead != 0xD1) || index + 1 >= input.len() {
            return PM_TEXT_FOLD_UNSUPPORTED;
        }
        let trail = input[index + 1];
        if trail & 0xC0 != 0x80 {
            return PM_TEXT_FOLD_UNSUPPORTED;
        }
        let scalar = (u32::from(lead & 0x1F) << 6) | u32::from(trail & 0x3F);
        let Some(folded) = fold_cyrillic_scalar(scalar) else {
            return PM_TEXT_FOLD_UNSUPPORTED;
        };
        if written + 2 > capacity {
            return PM_TEXT_FOLD_OVERFLOW;
        }
        *out.add(written) = (0xC0 | (folded >> 6)) as u8;
        *out.add(written + 1) = (0x80 | (folded & 0x3F)) as u8;
        written += 2;
        index += 2;
    }
    written as i32
}

#[no_mangle]
pub unsafe extern "C" fn pm_text_find(
    haystack: *const u8,
    haystack_length: i32,
    needle: *const u8,
    needle_length: i32,
) -> i32 {
    if needle.is_null() || needle_length <= 0 {
        return -1;
    }
    if haystack.is_null() || haystack_length < needle_length {
        return -1;
    }
    let haystack = slice::from_raw_parts(haystack, haystack_length as usize);
    let needle = slice::from_raw_parts(needle, needle_length as usize);
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
        .map_or(-1, |offset| offset as i32)
}

#[inline]
fn is_ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

#[no_mangle]
pub unsafe extern "C" fn pm_text_normalize_identity(
    input: *const u8,
    length: i32,
    out: *mut u8,
    out_capacity: i32,
) -> i32 {
    let folded = pm_text_fold_utf8(input, length, out, out_capacity);
    if folded < 0 {
        return folded;
    }
    let mut start = 0i32;
    let mut end = folded;
    while start < end && is_ascii_whitespace(*out.add(start as usize)) {
        start += 1;
    }
    while end > start && is_ascii_whitespace(*out.add(end as usize - 1)) {
        end -= 1;
    }
    if start > 0 {
        core::ptr::copy(out.add(start as usize), out, (end - start) as usize);
    }
    end - start
}

// ------------------------------------------------------------ artwork tint

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct PMArtworkTint {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

fn saturation(red: f32, green: f32, blue: f32) -> f32 {
    let high = if red > green {
        if red > blue {
            red
        } else {
            blue
        }
    } else if green > blue {
        green
    } else {
        blue
    };
    let low = if red < green {
        if red < blue {
            red
        } else {
            blue
        }
    } else if green < blue {
        green
    } else {
        blue
    };
    if high <= 0.0 {
        return 0.0;
    }
    (high - low) / high
}

#[no_mangle]
pub unsafe extern "C" fn pm_artwork_extract_tint_rgba(
    rgba: *const u8,
    length: i32,
    out: *mut PMArtworkTint,
) -> bool {
    if out.is_null() || rgba.is_null() || length < 4 || length % 4 != 0 {
        return false;
    }
    let pixels = slice::from_raw_parts(rgba, length as usize);

    let (mut weighted_red, mut weighted_green, mut weighted_blue) = (0.0f64, 0.0f64, 0.0f64);
    let mut total_weight = 0.0f64;

    for pixel in pixels.chunks_exact(4) {
        let alpha = f64::from(pixel[3]) / 255.0;
        if alpha <= 0.4 {
            continue;
        }
        let red = (f64::from(pixel[0]) / 255.0) as f32;
        let green = (f64::from(pixel[1]) / 255.0) as f32;
        let blue = (f64::from(pixel[2]) / 255.0) as f32;
        let weight = alpha * (0.08 + f64::from(saturation(red, green, blue)));
        weighted_red += f64::from(red) * weight;
        weighted_green += f64::from(green) * weight;
        weighted_blue += f64::from(blue) * weight;
        total_weight += weight;
    }

    if total_weight <= 0.0 {
        return false;
    }
    *out = PMArtworkTint {
        red: (weighted_red / total_weight) as f32,
        green: (weighted_green / total_weight) as f32,
        blue: (weighted_blue / total_weight) as f32,
    };
    true
}

// ------------------------------------------------------ mix queue ranking

pub const PM_MIX_MODE_CLOSER_TO_SEED: i32 = 1;
pub const PM_MIX_MODE_MORE_NOVEL: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PMMixCandidate {
    pub artist_hash: u64,
    pub album_hash: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PMMixRankContext {
    pub mode: i32,
    pub seed_artist_hash: u64,
    pub seed_album_hash: u64,
    pub history_hashes: *const u64,
    pub history_count: i32,
}

unsafe fn history_contains(history: *const u64, count: i32, artist_hash: u64) -> bool {
    if history.is_null() || count <= 0 {
        return false;
    }
    let sorted = slice::from_raw_parts(history, count as usize);
    // Same binary search as the C code: the list is sorted ascending.
    let (mut low, mut high) = (0i32, count - 1);
    while low <= high {
        let middle = low + (high - low) / 2;
        let value = sorted[middle as usize];
        if value == artist_hash {
            return true;
        }
        if value < artist_hash {
            low = middle + 1;
        } else {
            high = middle - 1;
        }
    }
    false
}

/// Penalize an artist that just played, which is what stopped mix radio from
/// looping the same handful of picks.
unsafe fn spacing_bonus(
    artist_hash: u64,
    recent: *const u64,
    recent_count: i32,
    immediate: f64,
    near: f64,
    window: f64,
    fresh: f64,
) -> f64 {
    if artist_hash == 0 {
        return 0.0;
    }
    if recent.is_null() || recent_count <= 0 {
        return fresh;
    }
    let recent = slice::from_raw_parts(recent, recent_count as usize);
    let count = recent.len();
    if recent[count - 1] == artist_hash {
        return immediate;
    }
    let near_start = if count >= 2 { count - 2 } else { 0 };
    if recent[near_start..].contains(&artist_hash) {
        return near;
    }
    if recent[..near_start].contains(&artist_hash) {
        return window;
    }
    fresh
}

#[no_mangle]
pub unsafe extern "C" fn pm_mix_score_candidate(
    candidate: PMMixCandidate,
    jitter: f64,
    context: *const PMMixRankContext,
    recent_hashes: *const u64,
    recent_count: i32,
) -> f64 {
    if context.is_null() {
        return 0.0;
    }
    let context = &*context;
    let artist = candidate.artist_hash;
    let album = candidate.album_hash;
    let seed_artist = context.seed_artist_hash;
    let seed_album = context.seed_album_hash;
    let in_history = history_contains(context.history_hashes, context.history_count, artist);
    let mut value = 0.0f64;

    if context.mode == PM_MIX_MODE_CLOSER_TO_SEED {
        if seed_artist != 0 && artist == seed_artist {
            value += 42.0;
        }
        if seed_album != 0 && album == seed_album {
            value += 18.0;
        }
        if in_history {
            value += 6.0;
        }
        if seed_artist != 0 && artist != seed_artist {
            value += 4.0;
        }
        // Mild spacing only: affinity to the seed artist must still win the
        // first pick after the current track.
        value += spacing_bonus(artist, recent_hashes, recent_count, -16.0, -8.0, -3.0, 0.0);
    } else if context.mode == PM_MIX_MODE_MORE_NOVEL {
        if seed_artist != 0 && artist != seed_artist {
            value += 32.0;
        }
        if seed_artist != 0 && artist == seed_artist {
            value -= 18.0;
        }
        if in_history {
            value -= 16.0;
        } else {
            value += 26.0;
        }
        if seed_album != 0 && album != seed_album {
            value += 8.0;
        }
        value += spacing_bonus(artist, recent_hashes, recent_count, -55.0, -28.0, -12.0, 12.0);
    } else {
        return 0.0;
    }
    value + jitter
}

#[no_mangle]
pub unsafe extern "C" fn pm_mix_select_best(
    candidates: *const PMMixCandidate,
    jitter: *const f64,
    count: i32,
    context: *const PMMixRankContext,
    recent_hashes: *const u64,
    recent_count: i32,
) -> i32 {
    if candidates.is_null() || jitter.is_null() || context.is_null() || count <= 0 {
        return 0;
    }
    let mut best_index = 0i32;
    let mut best_score = -f64::MAX;
    for index in 0..count {
        let value = pm_mix_score_candidate(
            *candidates.add(index as usize),
            *jitter.add(index as usize),
            context,
            recent_hashes,
            recent_count,
        );
        if value > best_score {
            best_score = value;
            best_index = index;
        }
    }
    best_index
}

// ------------------------------------------------- buffer health estimator

pub const PM_BUFFER_HEALTH_WINDOW_CAPACITY: usize = 32;
pub const PM_BUFFER_HEALTH_UNDERRUN_NONE: f64 = 1.0e9;
/// Ticks closer together than this (or non-monotonic) are dropped instead of
/// dividing by a near-zero interval.
const BUFFER_HEALTH_MIN_INTERVAL: f64 = 1e-6;

/// Fixed-memory rolling estimator over `loadedTimeRanges` samples. Layout
/// matches `pm_buffer_health` in `PrivateMusicCore.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct pm_buffer_health {
    pub rate_samples: [f64; PM_BUFFER_HEALTH_WINDOW_CAPACITY],
    pub rate_sum: f64,
    pub rate_count: i32,
    pub next_index: i32,
    pub previous_timestamp: f64,
    pub previous_loaded_ahead: f64,
    pub latest_loaded_ahead: f64,
    pub has_previous: bool,
}

#[no_mangle]
pub unsafe extern "C" fn pm_buffer_health_reset(state: *mut pm_buffer_health) {
    if state.is_null() {
        return;
    }
    core::ptr::write_bytes(state as *mut c_void as *mut u8, 0, core::mem::size_of::<pm_buffer_health>());
}

#[no_mangle]
pub unsafe extern "C" fn pm_buffer_health_observe(
    state: *mut pm_buffer_health,
    now_seconds: f64,
    loaded_ahead_seconds: f64,
) {
    if state.is_null() {
        return;
    }
    let state = &mut *state;

    if !state.has_previous {
        state.previous_timestamp = now_seconds;
        state.previous_loaded_ahead = loaded_ahead_seconds;
        state.latest_loaded_ahead = loaded_ahead_seconds;
        state.has_previous = true;
        return;
    }

    let elapsed = now_seconds - state.previous_timestamp;
    if elapsed <= BUFFER_HEALTH_MIN_INTERVAL {
        // Non-monotonic or duplicate tick: keep the latest reading for the
        // predicted underrun, but do not fold it into the rolling rate.
        state.latest_loaded_ahead = loaded_ahead_seconds;
        return;
    }

    let mut rate = 1.0 + (loaded_ahead_seconds - state.previous_loaded_ahead) / elapsed;
    if rate < 0.0 {
        rate = 0.0;
    }

    let index = state.next_index as usize;
    if state.rate_count as usize == PM_BUFFER_HEALTH_WINDOW_CAPACITY {
        state.rate_sum -= state.rate_samples[index];
    } else {
        state.rate_count += 1;
    }
    state.rate_samples[index] = rate;
    state.rate_sum += rate;
    state.next_index = ((index + 1) % PM_BUFFER_HEALTH_WINDOW_CAPACITY) as i32;

    state.previous_timestamp = now_seconds;
    state.previous_loaded_ahead = loaded_ahead_seconds;
    state.latest_loaded_ahead = loaded_ahead_seconds;
}

#[no_mangle]
pub unsafe extern "C" fn pm_buffer_health_throughput(state: *const pm_buffer_health) -> f64 {
    if state.is_null() || (*state).rate_count == 0 {
        return 1.0;
    }
    (*state).rate_sum / f64::from((*state).rate_count)
}

#[no_mangle]
pub unsafe extern "C" fn pm_buffer_health_predicted_underrun(
    state: *const pm_buffer_health,
    play_rate: f64,
) -> f64 {
    if state.is_null() || !(*state).has_previous || play_rate <= 0.0 {
        return PM_BUFFER_HEALTH_UNDERRUN_NONE;
    }
    let drain_rate = play_rate - pm_buffer_health_throughput(state);
    if drain_rate <= 0.0 {
        return PM_BUFFER_HEALTH_UNDERRUN_NONE;
    }
    if (*state).latest_loaded_ahead <= 0.0 {
        return 0.0;
    }
    (*state).latest_loaded_ahead / drain_rate
}
