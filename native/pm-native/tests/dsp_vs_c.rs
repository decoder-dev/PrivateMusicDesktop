//! Differential test: the original C implementation (kept as a reference in
//! `tests/reference/`, compiled here with a
//! tiny Accelerate shim) and the Rust one must agree on random inputs.
//! Skipped quietly when no C compiler is available.

use pm_native::dsp::*;
use std::ffi::c_int;

mod common;
use common::{build_reference, same, sym, Rng};

fn coeffs(rng: &mut Rng) -> PMBiquadCoeffs {
    PMBiquadCoeffs {
        b0: 0.5 + rng.next() * 0.4,
        b1: rng.next() * 0.5,
        b2: rng.next() * 0.3,
        a1: rng.next() * 0.6,
        a2: rng.next() * 0.3,
    }
}

#[test]
fn matches_the_c_reference() {
    let Some(h) = build_reference("PrivateMusicDSP.c") else {
        eprintln!("no C compiler; differential test skipped");
        return;
    };
    type Channel = unsafe extern "C" fn(*mut f32, c_int, c_int, *const PMBiquadCoeffs, *mut f32, bool);
    type Eq = unsafe extern "C" fn(
        *mut f32, c_int, c_int, *const PMBiquadCoeffs, c_int, *mut f32, f32, bool, f32, f32, f32,
        *mut f32, bool, f32,
    );
    type Planar = unsafe extern "C" fn(*mut f32, *mut f32, c_int, f32, *const PMBiquadCoeffs, bool, *mut f32);
    type Inter = unsafe extern "C" fn(*mut f32, c_int, c_int, f32, *const PMBiquadCoeffs, bool, *mut f32);
    type Widen = unsafe extern "C" fn(f32, f32, f32, f32, bool, *mut f32, *mut f32);
    type Peak = unsafe extern "C" fn(*const f32, i32) -> f32;

    let c_channel: Channel = sym(h, "pm_biquad_process_channel");
    let c_eq: Eq = sym(h, "pm_eq_process_channel");
    let c_planar: Planar = sym(h, "pm_spatial_process_planar");
    let c_inter: Inter = sym(h, "pm_spatial_process_interleaved");
    let c_widen: Widen = sym(h, "pm_spatial_widen");
    let c_peak: Peak = sym(h, "pm_buffer_peak_magnitude");

    let mut rng = Rng(0x9E3779B97F4A7C15);
    for round in 0..200 {
        let frames = 1 + (round * 7) % 257;
        let stride = 1 + round % 3;
        let len = frames * stride + 2;
        let src: Vec<f32> = (0..len).map(|_| rng.next() * 1.4).collect();
        let c0 = coeffs(&mut rng);

        // single biquad channel, with and without clamping
        for clamp in [false, true] {
            let (mut a, mut b) = (src.clone(), src.clone());
            let (mut sa, mut sb) = ([0.1f32, -0.2, 0.3, 0.05], [0.1f32, -0.2, 0.3, 0.05]);
            unsafe {
                pm_biquad_process_channel(a.as_mut_ptr(), frames as c_int, stride as c_int, &c0, sa.as_mut_ptr(), clamp);
                c_channel(b.as_mut_ptr(), frames as c_int, stride as c_int, &c0, sb.as_mut_ptr(), clamp);
            }
            same(&a, &b);
            same(&sa, &sb);
        }

        // equalizer cascade
        let bands: Vec<PMBiquadCoeffs> = (0..1 + round % 5).map(|_| coeffs(&mut rng)).collect();
        for (drc, loud) in [(false, false), (true, false), (true, true)] {
            let (mut a, mut b) = (src.clone(), src.clone());
            let mut states_a = vec![0.0f32; bands.len() * 4];
            let mut states_b = states_a.clone();
            let (mut env_a, mut env_b) = (0.7f32, 0.7f32);
            unsafe {
                pm_eq_process_channel(
                    a.as_mut_ptr(), frames as c_int, stride as c_int, bands.as_ptr(), bands.len() as c_int,
                    states_a.as_mut_ptr(), 0.9, drc, 0.5, 4.0, 1.1, &mut env_a, loud, 1.05,
                );
                c_eq(
                    b.as_mut_ptr(), frames as c_int, stride as c_int, bands.as_ptr(), bands.len() as c_int,
                    states_b.as_mut_ptr(), 0.9, drc, 0.5, 4.0, 1.1, &mut env_b, loud, 1.05,
                );
            }
            same(&a, &b);
            same(&states_a, &states_b);
            assert!((env_a - env_b).abs() <= 1e-6);
        }

        // spatial widening (planar and interleaved), with and without the side filter
        let intensity = rng.next();
        for filtered in [false, true] {
            let (mut la, mut ra) = (src[..frames].to_vec(), src[frames..2 * frames.min(len / 2)].to_vec());
            ra.resize(frames, 0.0);
            let (mut lb, mut rb) = (la.clone(), ra.clone());
            let (mut sa, mut sb) = ([0.0f32; 4], [0.0f32; 4]);
            unsafe {
                pm_spatial_process_planar(la.as_mut_ptr(), ra.as_mut_ptr(), frames as c_int, intensity, &c0, filtered, sa.as_mut_ptr());
                c_planar(lb.as_mut_ptr(), rb.as_mut_ptr(), frames as c_int, intensity, &c0, filtered, sb.as_mut_ptr());
            }
            same(&la, &lb);
            same(&ra, &rb);
            same(&sa, &sb);

            let stride2 = 2 + round % 3;
            let n = frames * stride2 + 2;
            let inter: Vec<f32> = (0..n).map(|_| rng.next()).collect();
            let (mut a, mut b) = (inter.clone(), inter.clone());
            let (mut sa, mut sb) = ([0.0f32; 4], [0.0f32; 4]);
            unsafe {
                pm_spatial_process_interleaved(a.as_mut_ptr(), frames as c_int, stride2 as c_int, intensity, &c0, filtered, sa.as_mut_ptr());
                c_inter(b.as_mut_ptr(), frames as c_int, stride2 as c_int, intensity, &c0, filtered, sb.as_mut_ptr());
            }
            same(&a, &b);
            same(&sa, &sb);
        }

        // single-sample widen and the peak meter
        let (l, r, side) = (rng.next(), rng.next(), rng.next());
        for has_side in [false, true] {
            let (mut ol_a, mut or_a, mut ol_b, mut or_b) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            unsafe {
                pm_spatial_widen(l, r, intensity, side, has_side, &mut ol_a, &mut or_a);
                c_widen(l, r, intensity, side, has_side, &mut ol_b, &mut or_b);
            }
            assert_eq!(ol_a.to_bits(), ol_b.to_bits());
            assert_eq!(or_a.to_bits(), or_b.to_bits());
        }
        unsafe {
            assert_eq!(
                pm_buffer_peak_magnitude(src.as_ptr(), len as i32).to_bits(),
                c_peak(src.as_ptr(), len as i32).to_bits()
            );
        }
    }
}

#[test]
fn null_and_empty_inputs_are_ignored() {
    unsafe {
        pm_biquad_process_channel(std::ptr::null_mut(), 4, 1, std::ptr::null(), std::ptr::null_mut(), true);
        assert_eq!(pm_buffer_peak_magnitude(std::ptr::null(), 4), 0.0);
        let data = [0.5f32, -0.9, 0.2];
        assert_eq!(pm_buffer_peak_magnitude(data.as_ptr(), 0), 0.0);
        assert!((pm_buffer_peak_magnitude(data.as_ptr(), 3) - 0.9).abs() < 1e-7);
    }
}
