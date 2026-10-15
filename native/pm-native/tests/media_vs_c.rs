//! Differential test for the media module: MPEG-TS demux, ISO box walking,
//! CMAF fragment/initialization parsing, VK unmasking and the loaded-ahead
//! fold are compared with the original C on well-formed, mutated and random
//! input. AES is checked against OpenSSL-generated vectors instead (the C
//! version calls CommonCrypto, which does not exist on Linux).

use pm_native::media::*;
use std::ffi::c_void;

mod common;
use common::{build_reference, sym, Rng};

fn rng_bytes(rng: &mut Rng, n: usize) -> Vec<u8> {
    (0..n).map(|_| rng.u32() as u8).collect()
}

/// Flip, overwrite or truncate a few bytes so parsers see near-valid input.
fn mutate(rng: &mut Rng, mut v: Vec<u8>) -> Vec<u8> {
    if v.is_empty() {
        return v;
    }
    match rng.below(5) {
        0 => {}
        1 => {
            let keep = rng.below(v.len() + 1);
            v.truncate(keep);
        }
        _ => {
            for _ in 0..1 + rng.below(4) {
                let i = rng.below(v.len());
                v[i] = match rng.below(4) {
                    0 => 0,
                    1 => 0xFF,
                    2 => v[i] ^ (1 << rng.below(8)),
                    _ => rng.u32() as u8,
                };
            }
        }
    }
    v
}

fn be32(x: u32) -> [u8; 4] {
    x.to_be_bytes()
}

fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = be32(8 + payload.len() as u32).to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

// ------------------------------------------------------------------- TS

fn ts_packet(pid: u16, start: bool, payload: &[u8], counter: u8) -> Vec<u8> {
    let mut p = vec![0x47, ((start as u8) << 6) | (pid >> 8) as u8, pid as u8, 0x10 | (counter & 0xF)];
    let room = 184;
    let mut body = payload.to_vec();
    body.truncate(room);
    if body.len() < room {
        // Stuff with an adaptation field.
        let pad = room - body.len();
        p[3] = 0x30 | (counter & 0xF);
        p.push((pad - 1) as u8);
        if pad > 1 {
            p.push(0);
            p.extend(std::iter::repeat(0xFF).take(pad - 2));
        }
    }
    p.extend_from_slice(&body);
    p
}

fn psi(table: u8, body: &[u8]) -> Vec<u8> {
    let mut section = vec![table, 0xB0 | ((body.len() + 9) >> 8) as u8, (body.len() + 9) as u8, 0, 1, 0xC1, 0, 0];
    section.extend_from_slice(body);
    section.extend_from_slice(&[0, 0, 0, 0]);
    let mut v = vec![0];
    v.extend(section);
    v
}

fn ts_stream(rng: &mut Rng, adts: bool) -> Vec<u8> {
    let audio_pid = 0x100 + rng.below(8) as u16;
    let pmt_pid = 0x20u16;
    let stream_type = if adts { 0x0F } else { 0x03 };
    let mut out = Vec::new();
    out.extend(ts_packet(0, true, &psi(0, &[0, 1, 0xE0, pmt_pid as u8]), 0));
    let pmt = [0xE1, audio_pid as u8, 0xF0, 0, stream_type, 0xE0 | (audio_pid >> 8) as u8, audio_pid as u8, 0xF0, 0];
    out.extend(ts_packet(pmt_pid, true, &psi(2, &pmt), 0));
    let frames = 2 + rng.below(6);
    for f in 0..frames {
        let es_len = 40 + rng.below(500);
        let mut es = rng_bytes(rng, es_len);
        if adts {
            es[0] = 0xFF;
            es[1] = 0xF1;
        } else {
            es[0] = 0xFF;
            es[1] = 0xFB;
        }
        let bounded = rng.below(2) == 0;
        let header = if bounded { es_len + 3 } else { 0 };
        let mut pes = vec![0, 0, 1, 0xC0, (header >> 8) as u8, header as u8, 0x80, 0x00, 0x00];
        pes.extend(&es);
        let mut counter = 0u8;
        let mut first = true;
        for chunk in pes.chunks(184) {
            out.extend(ts_packet(audio_pid, first, chunk, counter));
            first = false;
            counter = counter.wrapping_add(1);
        }
        if f == 1 && rng.below(3) == 0 {
            out.extend(ts_packet(0x200, true, &rng_bytes(rng, 100), 0));
        }
    }
    out
}

#[test]
fn matches_the_c_reference() {
    let Some(h) = build_reference("PrivateMusicMedia.c") else {
        eprintln!("no C compiler; differential test skipped");
        return;
    };
    type PacketSize = unsafe extern "C" fn(*const u8, i32) -> i32;
    type ParsePacket = unsafe extern "C" fn(*const u8, i32, *mut i32, *mut bool, *mut i32, *mut i32) -> bool;
    type ParsePat = unsafe extern "C" fn(*const u8, i32) -> i32;
    type ParsePmt = unsafe extern "C" fn(*const u8, i32, *mut i32, *mut u8) -> bool;
    type PesSlice = unsafe extern "C" fn(*const u8, i32, bool, *mut i32, *mut i32, *mut i32) -> bool;
    type Extract = unsafe extern "C" fn(*const u8, i32, *mut u8, i32, *mut u8, i32, *mut i32) -> i32;
    type Walk = unsafe extern "C" fn(*const u8, i32, i32, i32, *mut pm_iso_box, i32, *mut pm_iso_status) -> i32;
    type Contains = unsafe extern "C" fn(*const u8, i32, *const u32, i32) -> bool;
    type Fragment = unsafe extern "C" fn(
        *const u8, i32, u32, u32, bool, u32, bool, u32, bool, *mut i64, *mut bool, *mut pm_cmaf_sample, i32,
        *mut pm_cmaf_status,
    ) -> i32;
    type Init = unsafe extern "C" fn(*const u8, i32, *mut pm_cmaf_init, *mut pm_cmaf_status) -> i32;
    type Unmask = unsafe extern "C" fn(*const u8, i32, i32, *mut u8, i32) -> i32;
    type Ahead = unsafe extern "C" fn(f64, *const f64, i32) -> f64;

    let c_packet_size: PacketSize = sym(h, "pm_mpegts_packet_size");
    let c_parse_packet: ParsePacket = sym(h, "pm_mpegts_parse_packet");
    let c_parse_pat: ParsePat = sym(h, "pm_mpegts_parse_pat");
    let c_parse_pmt: ParsePmt = sym(h, "pm_mpegts_parse_pmt");
    let c_pes_slice: PesSlice = sym(h, "pm_mpegts_pes_slice");
    let c_extract: Extract = sym(h, "pm_mpegts_extract_audio");
    let c_walk: Walk = sym(h, "pm_iso_walk");
    let c_contains: Contains = sym(h, "pm_iso_contains_types");
    let c_fragment: Fragment = sym(h, "pm_cmaf_extract_fragment");
    let c_init: Init = sym(h, "pm_cmaf_parse_initialization");
    let c_unmask: Unmask = sym(h, "pm_vk_unmask");
    let c_ahead: Ahead = sym(h, "pm_buffer_max_loaded_ahead");
    let _ = h as *mut c_void;

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);

    // ---- MPEG-TS -------------------------------------------------------
    let mut ts_cases = 0;
    for round in 0..600 {
        let base = ts_stream(&mut rng, round % 2 == 0);
        let data = match round % 3 {
            0 => base,
            1 => mutate(&mut rng, base),
            _ => {
                let n = rng.below(1500);
                let mut d = rng_bytes(&mut rng, n);
                for i in (0..d.len()).step_by(188) {
                    d[i] = 0x47;
                }
                d
            }
        };
        let len = data.len() as i32;
        unsafe {
            assert_eq!(c_packet_size(data.as_ptr(), len), pm_mpegts_packet_size(data.as_ptr(), len));

            for _ in 0..4 {
                let off = rng.below(data.len().max(1));
                let slice = &data[off.min(data.len())..];
                let n = slice.len() as i32;
                let (mut a, mut b) = ((0i32, false, 0i32, 0i32), (0i32, false, 0i32, 0i32));
                let ra = c_parse_packet(slice.as_ptr(), n.min(188), &mut a.0, &mut a.1, &mut a.2, &mut a.3);
                let rb = pm_mpegts_parse_packet(slice.as_ptr(), n.min(188), &mut b.0, &mut b.1, &mut b.2, &mut b.3);
                assert_eq!(ra, rb);
                if ra {
                    assert_eq!(a, b);
                }
                assert_eq!(c_parse_pat(slice.as_ptr(), n), pm_mpegts_parse_pat(slice.as_ptr(), n));
                let (mut pa, mut ta, mut pb, mut tb) = (-7i32, 0u8, -7i32, 0u8);
                let ra = c_parse_pmt(slice.as_ptr(), n, &mut pa, &mut ta);
                let rb = pm_mpegts_parse_pmt(slice.as_ptr(), n, &mut pb, &mut tb);
                assert_eq!(ra, rb);
                assert_eq!((pa, ta), (pb, tb));
                for start in [false, true] {
                    let init = [-1, 0, 5, 184][rng.below(4)];
                    let (mut rema, mut remb) = (init, init);
                    let (mut oa, mut la, mut ob, mut lb) = (-9, -9, -9, -9);
                    let ra = c_pes_slice(slice.as_ptr(), n, start, &mut rema, &mut oa, &mut la);
                    let rb = pm_mpegts_pes_slice(slice.as_ptr(), n, start, &mut remb, &mut ob, &mut lb);
                    assert_eq!((ra, rema, oa, la), (rb, remb, ob, lb));
                }
            }

            let mut out_a = vec![0u8; data.len() + 8];
            let mut out_b = out_a.clone();
            let mut scratch_a = vec![0u8; data.len() + 8];
            let mut scratch_b = scratch_a.clone();
            let (mut la, mut lb) = (-1, -1);
            let ka = c_extract(
                data.as_ptr(), len, out_a.as_mut_ptr(), out_a.len() as i32, scratch_a.as_mut_ptr(),
                scratch_a.len() as i32, &mut la,
            );
            let kb = pm_mpegts_extract_audio(
                data.as_ptr(), len, out_b.as_mut_ptr(), out_b.len() as i32, scratch_b.as_mut_ptr(),
                scratch_b.len() as i32, &mut lb,
            );
            assert_eq!((ka, la), (kb, lb), "extract result round {round}");
            if ka != 0 {
                ts_cases += 1;
                assert_eq!(&out_a[..la as usize], &out_b[..lb as usize]);
            }
        }
    }
    assert!(ts_cases > 150, "TS generator should usually produce audio, got {ts_cases}");

    // ---- ISO walk / contains --------------------------------------------
    let kinds: [&[u8; 4]; 6] = [b"ftyp", b"moov", b"moof", b"mdat", b"free", b"trak"];
    for _ in 0..1500 {
        let mut data = Vec::new();
        for _ in 0..1 + rng.below(6) {
            let k = kinds[rng.below(kinds.len())];
            let n = rng.below(40);
            let payload = rng_bytes(&mut rng, n);
            data.extend(boxed(k, &payload));
        }
        let data = mutate(&mut rng, data);
        let len = data.len() as i32;
        let start = rng.below(data.len().max(1) + 2) as i32 - 1;
        let end = rng.below(data.len() + 3) as i32;
        let cap = [0usize, 1, 3, 70][rng.below(4)];
        let mut ba = vec![pm_iso_box::default(); cap];
        let mut bb = ba.clone();
        let (mut sa, mut sb) = (pm_iso_status::default(), pm_iso_status::default());
        unsafe {
            let na = c_walk(data.as_ptr(), len, start, end, ba.as_mut_ptr(), cap as i32, &mut sa);
            let nb = pm_iso_walk(data.as_ptr(), len, start, end, bb.as_mut_ptr(), cap as i32, &mut sb);
            assert_eq!(na, nb);
            assert_eq!((sa.code, sa.expected, sa.available), (sb.code, sb.expected, sb.available));
            for i in 0..cap.min(na.max(0) as usize) {
                assert_eq!(
                    (ba[i].type_, ba[i].start, ba[i].size, ba[i].header_size),
                    (bb[i].type_, bb[i].start, bb[i].size, bb[i].header_size)
                );
            }
            let want: Vec<u32> = (0..1 + rng.below(3)).map(|_| u32::from_be_bytes(*kinds[rng.below(6)])).collect();
            assert_eq!(
                c_contains(data.as_ptr(), len, want.as_ptr(), want.len() as i32),
                pm_iso_contains_types(data.as_ptr(), len, want.as_ptr(), want.len() as i32)
            );
        }
    }

    // ---- CMAF fragments ----------------------------------------------------
    let mut fragments_ok = 0;
    for round in 0..4000 {
        let track = 1 + rng.below(2) as u32;
        let mut trun_flags = 0u32;
        for bit in [0x1u32, 0x4, 0x100, 0x200, 0x400, 0x800] {
            if rng.below(2) == 0 {
                trun_flags |= bit;
            }
        }
        let samples = 1 + rng.below(8);
        let mut trun = vec![0u8];
        trun.extend_from_slice(&trun_flags.to_be_bytes()[1..]);
        trun.extend(be32(samples as u32));
        let mut sizes = Vec::new();
        let default_size = 20 + rng.below(30) as u32;
        // moof size is fixed below; data_offset is patched after assembly.
        let data_offset_at = trun.len();
        if trun_flags & 1 != 0 {
            trun.extend(be32(0));
        }
        if trun_flags & 4 != 0 {
            trun.extend(be32(if rng.below(2) == 0 { 0 } else { 0x0001_0000 }));
        }
        for _ in 0..samples {
            let size = 20 + rng.below(30) as u32;
            sizes.push(size);
            if trun_flags & 0x100 != 0 {
                trun.extend(be32(1000 + rng.below(50) as u32));
            }
            if trun_flags & 0x200 != 0 {
                trun.extend(be32(size));
            }
            if trun_flags & 0x400 != 0 {
                trun.extend(be32(if rng.below(3) == 0 { 0x0001_0000 } else { 0 }));
            }
            if trun_flags & 0x800 != 0 {
                trun.extend(be32((rng.below(200) as i32 - 100) as u32));
            }
        }
        let tfhd_flags = [0u32, 0x8, 0x10, 0x20, 0x38, 0x2, 0x1][rng.below(7)];
        let mut tfhd = vec![0u8];
        tfhd.extend_from_slice(&tfhd_flags.to_be_bytes()[1..]);
        tfhd.extend(be32(track));
        if tfhd_flags & 1 != 0 {
            tfhd.extend([0u8; 8]);
        }
        if tfhd_flags & 2 != 0 {
            tfhd.extend(be32(1));
        }
        if tfhd_flags & 8 != 0 {
            tfhd.extend(be32(1024));
        }
        if tfhd_flags & 0x10 != 0 {
            tfhd.extend(be32(default_size));
        }
        if tfhd_flags & 0x20 != 0 {
            tfhd.extend(be32(0));
        }
        let mut traf_children = boxed(b"tfhd", &tfhd);
        if rng.below(2) == 0 {
            let mut tfdt = vec![if rng.below(2) == 0 { 1 } else { 0 }, 0, 0, 0];
            if tfdt[0] == 1 {
                tfdt.extend((rng.u32() as u64).to_be_bytes());
            } else {
                tfdt.extend(be32(rng.u32() >> 4));
            }
            traf_children.extend(boxed(b"tfdt", &tfdt));
        }
        let trun_pos_in_traf = traf_children.len();
        traf_children.extend(boxed(b"trun", &trun));
        let traf = boxed(b"traf", &traf_children);
        let mut moof_payload = boxed(b"mfhd", &[0, 0, 0, 0, 0, 0, 0, 1]);
        let traf_start = moof_payload.len();
        moof_payload.extend(&traf);
        let mut moof = boxed(b"moof", &moof_payload);
        if trun_flags & 1 != 0 {
            let at = 8 + traf_start + 8 + trun_pos_in_traf + 8 + data_offset_at;
            let off = (moof.len() + 8) as i32 + if rng.below(8) == 0 { rng.below(20) as i32 - 10 } else { 0 };
            moof[at..at + 4].copy_from_slice(&off.to_be_bytes());
        }
        let total: u32 = sizes.iter().sum::<u32>().max(default_size * samples as u32);
        let mut data = moof;
        if rng.below(10) == 0 {
            data.splice(0..0, boxed(b"styp", b"msdh"));
        }
        let mdat_payload = rng_bytes(&mut rng, total as usize);
        data.extend(boxed(b"mdat", &mdat_payload));
        let data = if round % 4 == 0 { data } else { mutate(&mut rng, data) };

        let want_track = if rng.below(8) == 0 { track + 1 } else { track };
        let (dd, dh) = (rng.below(3) == 0, rng.below(3) == 0);
        let (ds, dsh) = (rng.below(2) == 0, rng.below(2) == 0);
        let (df, dfh) = (rng.below(2) == 0, rng.below(2) == 0);
        let start_time = rng.u32() as i64;
        let has_time = rng.below(2) == 0;
        let cap = [0usize, 2, 80][rng.below(3)];
        let mut sa = vec![pm_cmaf_sample::default(); cap];
        let mut sb = sa.clone();
        let (mut ta, mut tb) = (start_time, start_time);
        let (mut ha, mut hb) = (has_time, has_time);
        let (mut sta, mut stb) = (pm_cmaf_status::default(), pm_cmaf_status::default());
        let (pa, pb) = if cap == 0 { (std::ptr::null_mut(), std::ptr::null_mut()) } else { (sa.as_mut_ptr(), sb.as_mut_ptr()) };
        unsafe {
            let na = c_fragment(
                data.as_ptr(), data.len() as i32, want_track, 1024, dd, default_size, ds, 0, df, &mut ta, &mut ha,
                pa, cap as i32, &mut sta,
            );
            let nb = pm_cmaf_extract_fragment(
                data.as_ptr(), data.len() as i32, want_track, 1024, dd, default_size, ds, 0, df, &mut tb, &mut hb,
                pb, cap as i32, &mut stb,
            );
            let _ = dh;
            let _ = dsh;
            let _ = dfh;
            assert_eq!(na, nb, "fragment count round {round}");
            assert_eq!((sta.code, sta.found_track_id), (stb.code, stb.found_track_id), "round {round}");
            assert_eq!((ta, ha), (tb, hb));
            if na > 0 {
                fragments_ok += 1;
            }
            for i in 0..cap.min(na.max(0) as usize) {
                assert_eq!(
                    (sa[i].offset, sa[i].size, sa[i].decode_time, sa[i].presentation_time, sa[i].duration, sa[i].is_sync),
                    (sb[i].offset, sb[i].size, sb[i].decode_time, sb[i].presentation_time, sb[i].duration, sb[i].is_sync),
                    "sample {i} round {round}"
                );
            }
        }
    }
    assert!(fragments_ok > 400, "fragment generator should usually succeed, got {fragments_ok}");

    // ---- CMAF initialization ----------------------------------------------
    let mut inits_ok = 0;
    for round in 0..4000 {
        let version = rng.below(2) as u8;
        let mut tkhd = vec![version, 0, 0, 7];
        tkhd.extend(if version == 1 { vec![0u8; 16] } else { vec![0u8; 8] });
        tkhd.extend(be32(1 + rng.below(3) as u32));
        tkhd.extend([0u8; 60]);
        let mut mdhd = vec![version, 0, 0, 0];
        mdhd.extend(if version == 1 { vec![0u8; 16] } else { vec![0u8; 8] });
        mdhd.extend(be32([44100, 48000, 0][rng.below(3)]));
        mdhd.extend([0u8; 8]);
        let mut hdlr = vec![0u8, 0, 0, 0, 0, 0, 0, 0];
        hdlr.extend(if rng.below(5) == 0 { *b"vide" } else { *b"soun" });
        hdlr.extend([0u8; 12]);
        let asc = [0x12u8, 0x10];
        let mut dec_specific = vec![0x05, asc.len() as u8];
        dec_specific.extend(asc);
        let mut dec_config = vec![0x04, (13 + dec_specific.len()) as u8, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        dec_config.extend(&dec_specific);
        let mut es = vec![0x03, (3 + dec_config.len()) as u8, 0, 1, 0];
        es.extend(&dec_config);
        let mut esds_payload = vec![0u8; 4];
        esds_payload.extend(&es);
        let mut entry = vec![0u8; 6];
        entry.extend([0, 1]);
        entry.extend([0u8; 8]);
        entry.extend([0, 2, 0, 16, 0, 0, 0, 0]);
        entry.extend(be32(44100u32 << 16));
        entry.extend(boxed(b"esds", &esds_payload));
        let codec: &[u8; 4] = if rng.below(6) == 0 { b"alac" } else { b"mp4a" };
        let mut stsd = vec![0u8, 0, 0, 0, 0, 0, 0, 1];
        stsd.extend(boxed(codec, &entry));
        let stbl = boxed(b"stbl", &boxed(b"stsd", &stsd));
        let minf = boxed(b"minf", &stbl);
        let mut mdia_payload = boxed(b"mdhd", &mdhd);
        mdia_payload.extend(boxed(b"hdlr", &hdlr));
        mdia_payload.extend(minf);
        let mut trak_payload = boxed(b"tkhd", &tkhd);
        trak_payload.extend(boxed(b"mdia", &mdia_payload));
        let trak = boxed(b"trak", &trak_payload);
        let mut trex = vec![0u8; 4];
        for v in [1u32, 1, 1024, 99, 0] {
            trex.extend(be32(v));
        }
        let mvex = boxed(b"mvex", &boxed(b"trex", &trex));
        let mut moov_payload = Vec::new();
        if rng.below(4) == 0 {
            moov_payload.extend(boxed(b"free", &[0; 12]));
        }
        moov_payload.extend(&trak);
        if rng.below(4) == 0 {
            moov_payload.extend(&trak);
        }
        moov_payload.extend(&mvex);
        let mut data = boxed(b"ftyp", b"isom\0\0\0\0");
        data.extend(boxed(b"moov", &moov_payload));
        let data = if round % 3 == 0 { data } else { mutate(&mut rng, data) };

        let mut ia = zero_init();
        let mut ib = zero_init();
        let (mut sa, mut sb) = (pm_cmaf_status::default(), pm_cmaf_status::default());
        unsafe {
            let ra = c_init(data.as_ptr(), data.len() as i32, &mut ia, &mut sa);
            let rb = pm_cmaf_parse_initialization(data.as_ptr(), data.len() as i32, &mut ib, &mut sb);
            assert_eq!(ra, rb, "init result round {round}");
            assert_eq!((sa.code, sa.found_track_id), (sb.code, sb.found_track_id), "round {round}");
            let flat = |i: &pm_cmaf_init| -> [i64; 15] {
                [
                    i.track_id as i64, i.timescale as i64, i.sample_rate as i64, i.channel_count as i64,
                    i.codec_fourcc as i64, i.default_sample_duration as i64, i.default_sample_size as i64,
                    i.default_sample_flags as i64, i.has_default_duration as i64, i.has_default_size as i64,
                    i.has_default_flags as i64, i.esds_offset as i64, i.esds_length as i64, i.asc_offset as i64,
                    i.asc_length as i64,
                ]
            };
            assert_eq!(flat(&ia), flat(&ib), "init fields round {round}");
            if ra == 1 {
                inits_ok += 1;
            }
        }
    }
    assert!(inits_ok > 800, "init generator should usually succeed, got {inits_ok}");

    // ---- VK unmask -----------------------------------------------------------
    let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN0PQRSTUVWXYZO123456789+/=";
    let encode = |bytes: &[u8]| -> Vec<u8> {
        let std_b64 = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            let chars = chunk.len() + 1;
            for i in 0..chars {
                let idx = ((n >> (18 - 6 * i)) & 63) as usize;
                let _ = std_b64;
                out.push(alphabet[idx]);
            }
        }
        out
    };
    let mut unmask_ok = 0;
    for round in 0..3000 {
        let url_len = 5 + rng.below(80);
        let url: Vec<u8> = (0..url_len).map(|_| b'a' + rng.below(26) as u8).collect();
        let mut key = vec![b'x', 0x0B];
        key.extend(format!("{}", rng.u32() >> (rng.below(20) + 1)).bytes());
        if rng.below(4) == 0 {
            key.push(0x0B);
        }
        let mut raw = b"https://vk.com/audio_api_unavailable.mp3?extra=".to_vec();
        raw.extend(encode(&url));
        raw.push(b'#');
        raw.extend(encode(&key));
        let raw = match round % 4 {
            0 => raw,
            1 => mutate(&mut rng, raw),
            2 => b"https://cdn.example/a.m3u8".to_vec(),
            _ => {
                let n = rng.below(120);
                (0..n).map(|_| rng.u32() as u8).collect()
            }
        };
        let user = [0, 1, -5, 123_456_789, i32::MAX, i32::MIN][rng.below(6)];
        let cap = [4096usize, 8, 64][rng.below(3)];
        let (mut oa, mut ob) = (vec![0u8; cap], vec![0u8; cap]);
        unsafe {
            let ra = c_unmask(raw.as_ptr(), raw.len() as i32, user, oa.as_mut_ptr(), cap as i32);
            let rb = pm_vk_unmask(raw.as_ptr(), raw.len() as i32, user, ob.as_mut_ptr(), cap as i32);
            assert_eq!(ra, rb, "unmask round {round}");
            if ra > 0 {
                unmask_ok += 1;
                assert_eq!(&oa[..ra as usize], &ob[..rb as usize]);
            }
        }
    }
    assert!(unmask_ok > 400, "unmask generator should usually succeed, got {unmask_ok}");

    // ---- loaded ahead ----------------------------------------------------------
    for _ in 0..500 {
        let n = rng.below(8);
        let ends: Vec<f64> = (0..n)
            .map(|_| match rng.below(6) {
                0 => f64::NAN,
                1 => f64::INFINITY,
                2 => f64::NEG_INFINITY,
                _ => (rng.u32() % 4000) as f64 / 7.0,
            })
            .collect();
        let pos = match rng.below(8) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            _ => (rng.u32() % 4000) as f64 / 11.0,
        };
        let a = unsafe { c_ahead(pos, ends.as_ptr(), n as i32) };
        let b = unsafe { pm_buffer_max_loaded_ahead(pos, ends.as_ptr(), n as i32) };
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

fn zero_init() -> pm_cmaf_init {
    unsafe { std::mem::zeroed() }
}
