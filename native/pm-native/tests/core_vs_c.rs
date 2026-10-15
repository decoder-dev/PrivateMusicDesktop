//! Differential test for the core module: hashing, text folding/search,
//! artwork tint, mix ranking and the buffer-health estimator are compared
//! against the original C implementation on random and edge-case inputs.

use pm_native::core::*;
use std::ffi::c_void;

mod common;
use common::{build_reference, sym, Rng};

fn bytes(rng: &mut Rng, max: usize, alphabet: &[&str]) -> Vec<u8> {
    let n = ((rng.next().abs() * max as f32) as usize).min(max);
    let mut out = Vec::new();
    for _ in 0..n {
        let pick = ((rng.next() + 1.0) * 0.5 * alphabet.len() as f32) as usize;
        out.extend_from_slice(alphabet[pick.min(alphabet.len() - 1)].as_bytes());
    }
    out
}

const ALPHABET: &[&str] = &[
    "a", "Z", "q", " ", "\t", "\n", "-", "9", "А", "я", "Ё", "ё", "Й", "й", "ъ", "Я", "é", "日", "ґ", "Ѐ",
];

#[test]
fn matches_the_c_reference() {
    let Some(h) = build_reference("PrivateMusicCore.c") else {
        eprintln!("no C compiler; differential test skipped");
        return;
    };
    type Fold = unsafe extern "C" fn(*const u8, i32, *mut u8, i32) -> i32;
    type Find = unsafe extern "C" fn(*const u8, i32, *const u8, i32) -> i32;
    type HashBytes = unsafe extern "C" fn(u64, *const u8, i32) -> u64;
    type HashValue = unsafe extern "C" fn(u64, u64) -> u64;
    type Identity = unsafe extern "C" fn(*const u8, i32) -> u64;
    type Tint = unsafe extern "C" fn(*const u8, i32, *mut PMArtworkTint) -> bool;
    type Score = unsafe extern "C" fn(PMMixCandidate, f64, *const PMMixRankContext, *const u64, i32) -> f64;
    type Select = unsafe extern "C" fn(
        *const PMMixCandidate, *const f64, i32, *const PMMixRankContext, *const u64, i32,
    ) -> i32;
    type Reset = unsafe extern "C" fn(*mut pm_buffer_health);
    type Observe = unsafe extern "C" fn(*mut pm_buffer_health, f64, f64);
    type Through = unsafe extern "C" fn(*const pm_buffer_health) -> f64;
    type Underrun = unsafe extern "C" fn(*const pm_buffer_health, f64) -> f64;

    let c_fold: Fold = sym(h, "pm_text_fold_utf8");
    let c_norm: Fold = sym(h, "pm_text_normalize_identity");
    let c_find: Find = sym(h, "pm_text_find");
    let c_hash_bytes: HashBytes = sym(h, "pm_hash_fnv1a64_bytes");
    let c_hash_value: HashValue = sym(h, "pm_hash_fnv1a64_value");
    let c_identity: Identity = sym(h, "pm_text_identity_hash");
    let c_tint: Tint = sym(h, "pm_artwork_extract_tint_rgba");
    let c_score: Score = sym(h, "pm_mix_score_candidate");
    let c_select: Select = sym(h, "pm_mix_select_best");
    let c_reset: Reset = sym(h, "pm_buffer_health_reset");
    let c_observe: Observe = sym(h, "pm_buffer_health_observe");
    let c_through: Through = sym(h, "pm_buffer_health_throughput");
    let c_underrun: Underrun = sym(h, "pm_buffer_health_predicted_underrun");
    let c_basis = sym::<*const u64>(h, "pm_hash_fnv1a64_basis");
    assert_eq!(unsafe { *c_basis }, pm_hash_fnv1a64_basis);

    let mut rng = Rng(0xD1B54A32D192ED03);

    for round in 0..3000 {
        // ---- text: fold / normalize / find / hashes
        let text = bytes(&mut rng, 24, ALPHABET);
        // Occasionally corrupt the bytes so the unsupported/invalid paths run.
        let mut text = text;
        if round % 7 == 0 && !text.is_empty() {
            let at = round % text.len();
            text[at] = 0xD0;
        }
        if round % 11 == 0 && !text.is_empty() {
            let at = (round * 3) % text.len();
            text[at] = 0x80 | (round as u8 & 0x3F);
        }
        for capacity in [0i32, 3, text.len() as i32, text.len() as i32 + 4] {
            let mut a = vec![0xAAu8; capacity as usize + 1];
            let mut b = a.clone();
            let (ra, rb) = unsafe {
                (
                    pm_text_fold_utf8(text.as_ptr(), text.len() as i32, a.as_mut_ptr(), capacity),
                    c_fold(text.as_ptr(), text.len() as i32, b.as_mut_ptr(), capacity),
                )
            };
            assert_eq!(ra, rb, "fold result for {text:?} cap {capacity}");
            if ra >= 0 {
                assert_eq!(a[..ra as usize], b[..rb as usize]);
            }
            let mut a = vec![0xAAu8; capacity as usize + 1];
            let mut b = a.clone();
            let (ra, rb) = unsafe {
                (
                    pm_text_normalize_identity(text.as_ptr(), text.len() as i32, a.as_mut_ptr(), capacity),
                    c_norm(text.as_ptr(), text.len() as i32, b.as_mut_ptr(), capacity),
                )
            };
            assert_eq!(ra, rb, "normalize result for {text:?}");
            if ra >= 0 {
                assert_eq!(a[..ra as usize], b[..rb as usize]);
            }
        }

        let needle = bytes(&mut rng, 4, ALPHABET);
        unsafe {
            assert_eq!(
                pm_text_find(text.as_ptr(), text.len() as i32, needle.as_ptr(), needle.len() as i32),
                c_find(text.as_ptr(), text.len() as i32, needle.as_ptr(), needle.len() as i32),
                "find {needle:?} in {text:?}"
            );
            let seed = (rng.next().to_bits() as u64) << 20 | round as u64;
            assert_eq!(
                pm_hash_fnv1a64_bytes(seed, text.as_ptr(), text.len() as i32),
                c_hash_bytes(seed, text.as_ptr(), text.len() as i32)
            );
            assert_eq!(pm_hash_fnv1a64_value(seed, round as u64), c_hash_value(seed, round as u64));
            assert_eq!(
                pm_text_identity_hash(text.as_ptr(), text.len() as i32),
                c_identity(text.as_ptr(), text.len() as i32)
            );
        }

        // ---- artwork tint
        if round % 3 == 0 {
            let pixels = (round % 40) + 1;
            let data: Vec<u8> = (0..pixels * 4).map(|_| ((rng.next() + 1.0) * 127.5) as u8).collect();
            let (mut a, mut b) = (PMArtworkTint::default(), PMArtworkTint::default());
            let (ra, rb) = unsafe {
                (
                    pm_artwork_extract_tint_rgba(data.as_ptr(), data.len() as i32, &mut a),
                    c_tint(data.as_ptr(), data.len() as i32, &mut b),
                )
            };
            assert_eq!(ra, rb);
            assert_eq!((a.red.to_bits(), a.green.to_bits(), a.blue.to_bits()), (b.red.to_bits(), b.green.to_bits(), b.blue.to_bits()));
        }

        // ---- mix ranking
        let pool: Vec<u64> = (0..6).map(|i| 100 + i).collect();
        let pick = |r: &mut Rng| -> u64 {
            let k = ((r.next() + 1.0) * 0.5 * 7.0) as usize;
            if k >= pool.len() { 0 } else { pool[k] }
        };
        let mut history: Vec<u64> = (0..(round % 5)).map(|_| pick(&mut rng)).collect();
        history.sort_unstable();
        let recent: Vec<u64> = (0..(round % 9)).map(|_| pick(&mut rng)).collect();
        let candidates: Vec<PMMixCandidate> = (0..1 + round % 12)
            .map(|_| PMMixCandidate { artist_hash: pick(&mut rng), album_hash: pick(&mut rng) })
            .collect();
        let jitter: Vec<f64> = candidates.iter().map(|_| f64::from(rng.next()) * 5.0).collect();
        for mode in [0, 1, 2, 3] {
            let ctx = PMMixRankContext {
                mode,
                seed_artist_hash: pick(&mut rng),
                seed_album_hash: pick(&mut rng),
                history_hashes: if history.is_empty() { std::ptr::null() } else { history.as_ptr() },
                history_count: history.len() as i32,
            };
            let recent_ptr = if recent.is_empty() { std::ptr::null() } else { recent.as_ptr() };
            unsafe {
                assert_eq!(
                    pm_mix_select_best(candidates.as_ptr(), jitter.as_ptr(), candidates.len() as i32, &ctx, recent_ptr, recent.len() as i32),
                    c_select(candidates.as_ptr(), jitter.as_ptr(), candidates.len() as i32, &ctx, recent_ptr, recent.len() as i32)
                );
                let one = pm_mix_score_candidate(candidates[0], jitter[0], &ctx, recent_ptr, recent.len() as i32);
                let two = c_score(candidates[0], jitter[0], &ctx, recent_ptr, recent.len() as i32);
                assert_eq!(one.to_bits(), two.to_bits());
            }
        }
    }

    // ---- buffer health: identical state after long random tick sequences
    for run in 0..40 {
        let mut a: pm_buffer_health = unsafe { std::mem::zeroed() };
        let mut b: pm_buffer_health = unsafe { std::mem::zeroed() };
        unsafe {
            pm_buffer_health_reset(&mut a);
            c_reset(&mut b);
        }
        let (mut now, mut loaded) = (1000.0f64, 12.0f64);
        for tick in 0..(80 + run) {
            // Mostly forward ticks; some duplicate or backwards ones.
            now += match tick % 13 {
                5 => 0.0,
                9 => -0.4,
                _ => 0.45 + f64::from(rng.next()).abs() * 0.1,
            };
            loaded = (loaded + f64::from(rng.next()) * 1.5).max(0.0);
            unsafe {
                pm_buffer_health_observe(&mut a, now, loaded);
                c_observe(&mut b, now, loaded);
                assert_eq!(pm_buffer_health_throughput(&a).to_bits(), c_through(&b).to_bits());
                for rate in [1.0, 0.5, 2.0, -1.0] {
                    assert_eq!(
                        pm_buffer_health_predicted_underrun(&a, rate).to_bits(),
                        c_underrun(&b, rate).to_bits()
                    );
                }
            }
            assert_eq!(a.rate_count, b.rate_count);
            assert_eq!(a.next_index, b.next_index);
            assert_eq!(a.rate_sum.to_bits(), b.rate_sum.to_bits());
        }
        let _ = c_void::from;
    }
}

#[test]
fn hash_constants_and_sentinels_hold() {
    unsafe {
        assert_eq!(pm_text_identity_hash(std::ptr::null(), 3), 0);
        assert_ne!(pm_text_identity_hash(b"a".as_ptr(), 1), 0);
        assert_eq!(pm_hash_fnv1a64_bytes(7, std::ptr::null(), 4), 7);
        let mut out = [0u8; 8];
        let n = pm_text_fold_utf8("ЁЖ й".as_bytes().as_ptr(), 7, out.as_mut_ptr(), 8);
        assert_eq!(&out[..n as usize], "еж и".as_bytes());
        assert_eq!(pm_text_find(b"abc".as_ptr(), 3, b"".as_ptr(), 0), -1);
    }
    assert_eq!(core::mem::size_of::<pm_buffer_health>(), 32 * 8 + 8 + 4 + 4 + 8 * 3 + 8);
}
