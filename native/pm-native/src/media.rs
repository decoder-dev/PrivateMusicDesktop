//! Container and stream plumbing: MPEG-TS demux, ISO BMFF box walking, CMAF
//! fragment/initialization parsing, VK URL unmasking, loaded-ahead folding
//! and HLS AES-128-CBC. Behaviour mirrors the C implementation it replaces
//! (kept under `tests/reference/` and compared on random and malformed input).
//!
//! Every read of caller data goes through [`Buf`], which yields 0 outside the
//! slice instead of panicking: the C code never read out of range on inputs
//! that reached those lines, and the Rust build aborts on panic.

use core::slice;

// ------------------------------------------------------------------ helpers

#[derive(Clone, Copy)]
struct Buf<'a>(&'a [u8]);

impl<'a> Buf<'a> {
    #[inline]
    fn u8(&self, at: i32) -> u8 {
        if at < 0 {
            return 0;
        }
        self.0.get(at as usize).copied().unwrap_or(0)
    }
    #[inline]
    fn u16(&self, at: i32) -> u16 {
        (u16::from(self.u8(at)) << 8) | u16::from(self.u8(at.wrapping_add(1)))
    }
    #[inline]
    fn u24(&self, at: i32) -> u32 {
        (u32::from(self.u8(at)) << 16)
            | (u32::from(self.u8(at.wrapping_add(1))) << 8)
            | u32::from(self.u8(at.wrapping_add(2)))
    }
    #[inline]
    fn u32(&self, at: i32) -> u32 {
        (u32::from(self.u8(at)) << 24)
            | (u32::from(self.u8(at.wrapping_add(1))) << 16)
            | (u32::from(self.u8(at.wrapping_add(2))) << 8)
            | u32::from(self.u8(at.wrapping_add(3)))
    }
    #[inline]
    fn i32(&self, at: i32) -> i32 {
        self.u32(at) as i32
    }
    #[inline]
    fn u64(&self, at: i32) -> u64 {
        (u64::from(self.u32(at)) << 32) | u64::from(self.u32(at.wrapping_add(4)))
    }
}

unsafe fn input<'a>(ptr: *const u8, length: i32) -> Option<&'a [u8]> {
    if ptr.is_null() || length < 0 {
        return None;
    }
    Some(slice::from_raw_parts(ptr, length as usize))
}

// ------------------------------------------------------------------ MPEG-TS

pub const PM_MPEGTS_KIND_NONE: i32 = 0;
pub const PM_MPEGTS_KIND_ADTS: i32 = 1;
pub const PM_MPEGTS_KIND_MP3: i32 = 2;
pub const PM_MPEGTS_PES_UNBOUNDED: i32 = -1;

fn looks_adts(d: &[u8]) -> bool {
    d.len() >= 2 && d[0] == 0xFF && (d[1] & 0xF6) == 0xF0
}

fn looks_mp3(d: &[u8]) -> bool {
    if d.len() < 3 {
        return false;
    }
    if d[0] == 0x49 && d[1] == 0x44 && d[2] == 0x33 {
        return true;
    }
    if d[0] != 0xFF {
        return false;
    }
    let version = (d[1] >> 3) & 0x03;
    let layer = (d[1] >> 1) & 0x03;
    if version == 1 || layer == 0 {
        return false;
    }
    (d[1] & 0xE0) == 0xE0
}

fn infer_kind(d: &[u8]) -> i32 {
    if looks_adts(d) {
        PM_MPEGTS_KIND_ADTS
    } else if looks_mp3(d) {
        PM_MPEGTS_KIND_MP3
    } else {
        PM_MPEGTS_KIND_NONE
    }
}

fn kind_for_stream_type(t: u8) -> i32 {
    match t {
        0x0F => PM_MPEGTS_KIND_ADTS,
        0x03 | 0x04 => PM_MPEGTS_KIND_MP3,
        _ => PM_MPEGTS_KIND_NONE,
    }
}

fn packet_size(d: &[u8]) -> i32 {
    let length = d.len() as i64;
    if length <= 0 {
        return 0;
    }
    for size in [188i64, 192, 204] {
        let mut syncs = 0;
        let mut offset = 0i64;
        while offset + size <= length {
            if d[offset as usize] != 0x47 {
                break;
            }
            syncs += 1;
            if syncs >= 3 {
                return size as i32;
            }
            offset += size;
        }
        if syncs >= 2 && offset >= length {
            return size as i32;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn pm_mpegts_packet_size(data: *const u8, length: i32) -> i32 {
    if length <= 0 {
        return 0;
    }
    match input(data, length) {
        Some(d) => packet_size(d),
        None => 0,
    }
}

struct TsPacket {
    pid: i32,
    unit_start: bool,
    payload_offset: i32,
    payload_length: i32,
}

fn parse_packet(p: &[u8], size: i32) -> Option<TsPacket> {
    if size < 4 || p.len() < size as usize || p[0] != 0x47 {
        return None;
    }
    let pid = (i32::from(p[1] & 0x1F) << 8) | i32::from(p[2]);
    let unit_start = (p[1] & 0x40) != 0;
    let adaptation = (p[3] >> 4) & 0x03;
    let mut offset = 4i32;
    if adaptation == 2 || adaptation == 3 {
        if offset >= size {
            return None;
        }
        offset += 1 + i32::from(p[offset as usize]);
    }
    if adaptation != 1 && adaptation != 3 {
        return None;
    }
    if offset >= size {
        return None;
    }
    Some(TsPacket { pid, unit_start, payload_offset: offset, payload_length: size - offset })
}

#[no_mangle]
pub unsafe extern "C" fn pm_mpegts_parse_packet(
    packet: *const u8,
    packet_size: i32,
    pid: *mut i32,
    unit_start: *mut bool,
    payload_offset: *mut i32,
    payload_length: *mut i32,
) -> bool {
    let Some(p) = input(packet, packet_size.max(0)) else { return false };
    let Some(parsed) = parse_packet(p, packet_size) else { return false };
    if !pid.is_null() {
        *pid = parsed.pid;
    }
    if !unit_start.is_null() {
        *unit_start = parsed.unit_start;
    }
    if !payload_offset.is_null() {
        *payload_offset = parsed.payload_offset;
    }
    if !payload_length.is_null() {
        *payload_length = parsed.payload_length;
    }
    true
}

fn psi_section(payload: &[u8]) -> Option<&[u8]> {
    if payload.is_empty() {
        return None;
    }
    let start = 1 + i32::from(payload[0]);
    if start as usize >= payload.len() {
        return None;
    }
    Some(&payload[start as usize..])
}

fn parse_pat(payload: &[u8]) -> i32 {
    let Some(section) = psi_section(payload) else { return -1 };
    let section_length = section.len() as i32;
    if section_length < 8 || section[0] != 0x00 {
        return -1;
    }
    let declared = (i32::from(section[1] & 0x0F) << 8) | i32::from(section[2]);
    let section_end = 3 + declared;
    if section_length < section_end || section_end < 12 {
        return -1;
    }
    let mut offset = 8i32;
    let end = section_end - 4;
    while offset + 4 <= end {
        let o = offset as usize;
        let program = (i32::from(section[o]) << 8) | i32::from(section[o + 1]);
        let pid = (i32::from(section[o + 2] & 0x1F) << 8) | i32::from(section[o + 3]);
        offset += 4;
        if program != 0 {
            return pid;
        }
    }
    -1
}

#[no_mangle]
pub unsafe extern "C" fn pm_mpegts_parse_pat(payload: *const u8, length: i32) -> i32 {
    match input(payload, length) {
        Some(p) => parse_pat(p),
        None => -1,
    }
}

fn parse_pmt(payload: &[u8]) -> Option<(i32, u8)> {
    let section = psi_section(payload)?;
    let section_length = section.len() as i32;
    if section_length < 12 || section[0] != 0x02 {
        return None;
    }
    let declared = (i32::from(section[1] & 0x0F) << 8) | i32::from(section[2]);
    let section_end = 3 + declared;
    if section_length < section_end || section_end < 16 {
        return None;
    }
    let program_info = (i32::from(section[10] & 0x0F) << 8) | i32::from(section[11]);
    let mut offset = 12 + program_info;
    let end = section_end - 4;
    let mut preferred: Option<(i32, u8)> = None;
    while offset + 5 <= end {
        let o = offset as usize;
        let t = section[o];
        let pid = (i32::from(section[o + 1] & 0x1F) << 8) | i32::from(section[o + 2]);
        let es_info = (i32::from(section[o + 3] & 0x0F) << 8) | i32::from(section[o + 4]);
        offset += 5 + es_info;
        match kind_for_stream_type(t) {
            PM_MPEGTS_KIND_NONE => continue,
            PM_MPEGTS_KIND_ADTS => return Some((pid, t)),
            _ => {
                if preferred.is_none() {
                    preferred = Some((pid, t));
                }
            }
        }
    }
    preferred
}

#[no_mangle]
pub unsafe extern "C" fn pm_mpegts_parse_pmt(
    payload: *const u8,
    length: i32,
    audio_pid: *mut i32,
    stream_type: *mut u8,
) -> bool {
    let Some(p) = input(payload, length) else { return false };
    let Some((pid, t)) = parse_pmt(p) else { return false };
    if !audio_pid.is_null() {
        *audio_pid = pid;
    }
    if !stream_type.is_null() {
        *stream_type = t;
    }
    true
}

/// Returns (offset, length) of the elementary slice, updating `remaining`.
fn pes_slice(payload: &[u8], unit_start: bool, remaining: &mut i32) -> Option<(i32, i32)> {
    let payload_length = payload.len() as i32;
    let mut start = 0i32;
    if unit_start {
        if payload_length < 9 || payload[0] != 0 || payload[1] != 0 || payload[2] != 1 {
            *remaining = PM_MPEGTS_PES_UNBOUNDED;
            return None;
        }
        let packet_length = (i32::from(payload[4]) << 8) | i32::from(payload[5]);
        let header_length = i32::from(payload[8]);
        let payload_start = 9 + header_length;
        if payload_start > payload_length {
            *remaining = PM_MPEGTS_PES_UNBOUNDED;
            return None;
        }
        *remaining = if packet_length == 0 {
            PM_MPEGTS_PES_UNBOUNDED
        } else {
            packet_length - 3 - header_length
        };
        if *remaining < 0 {
            *remaining = 0;
        }
        start = payload_start;
    } else if *remaining == 0 {
        return None;
    }
    let available = payload_length - start;
    if available <= 0 {
        return None;
    }
    let mut count = available;
    if *remaining != PM_MPEGTS_PES_UNBOUNDED && *remaining < count {
        count = *remaining;
    }
    if *remaining != PM_MPEGTS_PES_UNBOUNDED {
        *remaining -= count;
        if *remaining < 0 {
            *remaining = 0;
        }
    }
    // The C version published offset/length before the final `count > 0`
    // check, so a zero-length slice still reported them while returning false.
    Some((start, count))
}

#[no_mangle]
pub unsafe extern "C" fn pm_mpegts_pes_slice(
    payload: *const u8,
    payload_length: i32,
    unit_start: bool,
    remaining: *mut i32,
    out_offset: *mut i32,
    out_length: *mut i32,
) -> bool {
    if remaining.is_null() || payload_length < 0 {
        return false;
    }
    let Some(p) = input(payload, payload_length) else { return false };
    match pes_slice(p, unit_start, &mut *remaining) {
        Some((offset, count)) => {
            if !out_offset.is_null() {
                *out_offset = offset;
            }
            if !out_length.is_null() {
                *out_length = count;
            }
            count > 0
        }
        None => false,
    }
}

fn find_pes(d: &[u8]) -> i32 {
    if d.len() < 4 {
        return -1;
    }
    for i in 0..d.len() - 3 {
        if d[i] == 0 && d[i + 1] == 0 && d[i + 2] == 1 {
            return i as i32;
        }
    }
    -1
}

struct Reassembly<'a> {
    buf: &'a mut [u8],
    len: i32,
}

impl Reassembly<'_> {
    fn consume(&mut self, count: i32) {
        if count <= 0 {
            return;
        }
        if count >= self.len {
            self.len = 0;
            return;
        }
        self.buf.copy_within(count as usize..self.len as usize, 0);
        self.len -= count;
    }
}

/// Returns consumed bytes, 0 to stop draining, -1 when more payload is needed.
fn drain_pes(
    r: &mut Reassembly,
    out: &mut [u8],
    out_len: &mut i32,
    audio_only: bool,
    allow_partial: bool,
) -> i32 {
    let start = find_pes(&r.buf[..r.len as usize]);
    if start < 0 {
        if r.len > 10000 {
            r.consume(r.len - 3);
        }
        return 0;
    }
    if start > 0 {
        r.consume(start);
    }
    if r.len < 9 {
        return if allow_partial { 0 } else { -1 };
    }
    let stream_id = r.buf[3];
    if audio_only && !(0xC0..=0xDF).contains(&stream_id) {
        r.consume(4);
        return 4;
    }
    let pes_packet_length = (i32::from(r.buf[4]) << 8) | i32::from(r.buf[5]);
    let header_data_length = i32::from(r.buf[8]);
    let payload_start = 9 + header_data_length;
    if r.len < payload_start {
        return if allow_partial { 0 } else { -1 };
    }
    let packet_end;
    if pes_packet_length == 0 {
        if !allow_partial {
            return -1;
        }
        packet_end = r.len;
    } else {
        packet_end = 6 + pes_packet_length;
        if r.len < packet_end && !allow_partial {
            return -1;
        }
    }
    let end = packet_end.min(r.len);
    if payload_start < end {
        let copy = end - payload_start;
        if *out_len >= 0 && copy > 0 && *out_len + copy <= out.len() as i32 {
            let from = payload_start as usize;
            out[*out_len as usize..(*out_len + copy) as usize]
                .copy_from_slice(&r.buf[from..from + copy as usize]);
            *out_len += copy;
        }
    }
    r.consume(end);
    end
}

fn append_and_drain(
    r: &mut Reassembly,
    payload: &[u8],
    out: &mut [u8],
    out_len: &mut i32,
    audio_only: bool,
) {
    let plen = payload.len() as i32;
    if plen > 0 && r.len + plen <= r.buf.len() as i32 {
        let at = r.len as usize;
        r.buf[at..at + payload.len()].copy_from_slice(payload);
        r.len += plen;
    }
    while drain_pes(r, out, out_len, audio_only, false) > 0 {}
}

#[no_mangle]
pub unsafe extern "C" fn pm_mpegts_extract_audio(
    data: *const u8,
    length: i32,
    out: *mut u8,
    out_capacity: i32,
    scratch: *mut u8,
    scratch_capacity: i32,
    out_length: *mut i32,
) -> i32 {
    if !out_length.is_null() {
        *out_length = 0;
    }
    if data.is_null() || length <= 0 || out.is_null() || scratch.is_null() {
        return PM_MPEGTS_KIND_NONE;
    }
    let d = slice::from_raw_parts(data, length as usize);
    let out_slice = slice::from_raw_parts_mut(out, out_capacity.max(0) as usize);
    let scratch_slice = slice::from_raw_parts_mut(scratch, scratch_capacity.max(0) as usize);
    let (kind, produced) = extract_audio(d, out_slice, scratch_slice);
    if kind != PM_MPEGTS_KIND_NONE && !out_length.is_null() {
        *out_length = produced;
    }
    kind
}

fn extract_audio(d: &[u8], out: &mut [u8], scratch: &mut [u8]) -> (i32, i32) {
    let size = packet_size(d);
    if size <= 0 {
        return (PM_MPEGTS_KIND_NONE, 0);
    }
    let length = d.len() as i64;
    let step = i64::from(size);

    let mut pmt_pid = -1;
    let mut offset = 0i64;
    while offset + step <= length {
        let p = &d[offset as usize..(offset + step) as usize];
        if let Some(pk) = parse_packet(p, size) {
            if pk.pid == 0 && pmt_pid < 0 {
                pmt_pid = parse_pat(&p[pk.payload_offset as usize..]);
            }
        }
        offset += step;
    }

    let mut audio_pid = -1;
    let mut stream_type = 0u8;
    if pmt_pid >= 0 {
        offset = 0;
        while offset + step <= length {
            let p = &d[offset as usize..(offset + step) as usize];
            if let Some(pk) = parse_packet(p, size) {
                if pk.pid == pmt_pid {
                    if let Some((pid, t)) = parse_pmt(&p[pk.payload_offset as usize..]) {
                        audio_pid = pid;
                        stream_type = t;
                        break;
                    }
                }
            }
            offset += step;
        }
    }

    let audio_only = audio_pid < 0;
    let mut r = Reassembly { buf: scratch, len: 0 };
    let mut produced = 0i32;
    offset = 0;
    while offset + step <= length {
        let p = &d[offset as usize..(offset + step) as usize];
        if let Some(pk) = parse_packet(p, size) {
            if audio_only || pk.pid == audio_pid {
                append_and_drain(&mut r, &p[pk.payload_offset as usize..], out, &mut produced, audio_only);
            }
        }
        offset += step;
    }
    drain_pes(&mut r, out, &mut produced, audio_only, true);

    if produced <= 32 {
        return (PM_MPEGTS_KIND_NONE, 0);
    }
    let mut kind = kind_for_stream_type(stream_type);
    if kind == PM_MPEGTS_KIND_NONE {
        kind = infer_kind(&out[..produced as usize]);
    }
    if kind == PM_MPEGTS_KIND_NONE {
        return (PM_MPEGTS_KIND_NONE, 0);
    }
    (kind, produced)
}

// ------------------------------------------------------------------ ISO BMFF

pub const PM_ISO_OK: i32 = 0;
pub const PM_ISO_TRUNCATED: i32 = 1;
pub const PM_ISO_INVALID: i32 = 2;
pub const PM_ISO_WALK_LIMIT: i32 = 4096;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct pm_iso_box {
    pub type_: u32,
    pub start: i32,
    pub size: i32,
    pub header_size: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct pm_iso_status {
    pub code: i32,
    pub expected: i32,
    pub available: i32,
}

#[no_mangle]
pub unsafe extern "C" fn pm_iso_fourcc(type_: *const u8) -> u32 {
    if type_.is_null() {
        return 0;
    }
    let mut value = 0u32;
    for i in 0..4 {
        let byte = *type_.add(i);
        if byte == 0 {
            break;
        }
        value = (value << 8) | u32::from(byte);
    }
    value
}

const fn tag(a: u8, b: u8, c: u8, d: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8) | (d as u32)
}

fn set_status(status: &mut pm_iso_status, code: i32, expected: i32, available: i32) {
    status.code = code;
    status.expected = expected;
    status.available = available;
}

fn parse_box(d: Buf, length: i32, offset: i32, limit: i32, st: &mut pm_iso_status) -> Option<pm_iso_box> {
    if offset < 0 || offset + 8 > length || offset + 8 > limit {
        let available = (length - offset.max(0)).max(0);
        set_status(st, PM_ISO_TRUNCATED, 8, available);
        return None;
    }
    let size32 = d.u32(offset);
    let type_ = d.u32(offset + 4);
    let mut header_size = 8;
    let mut size = i64::from(size32);
    if size32 == 0 {
        // `offset + 8 <= limit` already held above.
        set_status(st, PM_ISO_OK, 0, 0);
        return Some(pm_iso_box { type_, start: offset, size: limit - offset, header_size: 8 });
    }
    if size32 == 1 {
        if offset + 16 > length {
            set_status(st, PM_ISO_TRUNCATED, 16, (length - offset).max(0));
            return None;
        }
        let size64 = d.u64(offset + 8);
        if size64 > i32::MAX as u64 || size64 < 16 {
            set_status(st, PM_ISO_INVALID, 0, 0);
            return None;
        }
        header_size = 16;
        size = size64 as i64;
    } else if size < 8 {
        set_status(st, PM_ISO_INVALID, 0, 0);
        return None;
    }
    let remaining = limit - offset;
    if remaining < 0 || size > i64::from(remaining) {
        set_status(st, PM_ISO_TRUNCATED, size as i32, remaining.max(0));
        return None;
    }
    set_status(st, PM_ISO_OK, 0, 0);
    Some(pm_iso_box { type_, start: offset, size: size as i32, header_size })
}

/// Walks `[start, end)`; boxes beyond `cap` are counted but not stored.
fn walk(d: Buf, length: i32, start: i32, end: i32, out: &mut [pm_iso_box], st: &mut pm_iso_status) -> i32 {
    set_status(st, PM_ISO_OK, 0, 0);
    if start < 0 || start > end || end > length {
        let expected = (end - start).max(0);
        let available = (length - start).max(0);
        set_status(st, PM_ISO_TRUNCATED, expected, available);
        return 0;
    }
    let mut written = 0i32;
    let mut offset = start;
    let mut count = 0;
    while offset + 8 <= end && count < PM_ISO_WALK_LIMIT {
        count += 1;
        let Some(b) = parse_box(d, length, offset, end, st) else { return written };
        if (written as usize) < out.len() {
            out[written as usize] = b;
        }
        written += 1;
        if b.size <= 0 || b.size > end - offset {
            break;
        }
        offset += b.size;
    }
    st.code = PM_ISO_OK;
    written
}

#[no_mangle]
pub unsafe extern "C" fn pm_iso_walk(
    data: *const u8,
    length: i32,
    start: i32,
    end: i32,
    out: *mut pm_iso_box,
    out_capacity: i32,
    status: *mut pm_iso_status,
) -> i32 {
    let mut local = pm_iso_status::default();
    let st = if status.is_null() { &mut local } else { &mut *status };
    let Some(d) = input(data, length) else {
        let expected = (end - start).max(0);
        let available = (length - start).max(0);
        set_status(st, PM_ISO_TRUNCATED, expected, available);
        return 0;
    };
    let out_slice: &mut [pm_iso_box] = if out.is_null() || out_capacity <= 0 {
        &mut []
    } else {
        slice::from_raw_parts_mut(out, out_capacity as usize)
    };
    walk(Buf(d), length, start, end, out_slice, st)
}

#[no_mangle]
pub unsafe extern "C" fn pm_iso_contains_types(
    data: *const u8,
    length: i32,
    types: *const u32,
    type_count: i32,
) -> bool {
    if type_count <= 0 {
        return true;
    }
    if data.is_null() || types.is_null() || length < 8 {
        return false;
    }
    let d = Buf(slice::from_raw_parts(data, length as usize));
    let mut remaining = [0u32; 16];
    let mut remaining_count = type_count.min(16) as usize;
    remaining[..remaining_count].copy_from_slice(slice::from_raw_parts(types, remaining_count));

    let mut offset = 0i32;
    let mut walked = 0;
    while offset + 8 <= length && remaining_count > 0 && walked < PM_ISO_WALK_LIMIT {
        walked += 1;
        let size32 = d.u32(offset);
        let type_ = d.u32(offset + 4);
        for i in 0..remaining_count {
            if type_ != remaining[i] {
                continue;
            }
            remaining[i] = remaining[remaining_count - 1];
            remaining_count -= 1;
            break;
        }
        if remaining_count == 0 {
            return true;
        }
        let box_size: i64 = if size32 == 1 {
            if offset + 16 > length {
                break;
            }
            let size64 = d.u64(offset + 8);
            if size64 > i32::MAX as u64 {
                i64::from(length - offset)
            } else {
                size64 as i64
            }
        } else if size32 == 0 {
            i64::from(length - offset)
        } else {
            i64::from(size32)
        };
        if box_size < 8 || box_size > i64::from(length - offset) {
            break;
        }
        offset += box_size as i32;
    }
    remaining_count == 0
}

// --------------------------------------------------------------------- CMAF

pub const PM_CMAF_OK: i32 = 0;
pub const PM_CMAF_MISSING_MOOF: i32 = 1;
pub const PM_CMAF_MISSING_TFHD: i32 = 2;
pub const PM_CMAF_TRACK_MISMATCH: i32 = 3;
pub const PM_CMAF_MISSING_SIZE: i32 = 4;
pub const PM_CMAF_SAMPLE_OUTSIDE: i32 = 5;
pub const PM_CMAF_MISSING_MDAT: i32 = 6;
pub const PM_CMAF_TRUNCATED: i32 = 7;
pub const PM_CMAF_OVERFLOW: i32 = 8;
pub const PM_CMAF_MISSING_FTYP: i32 = 9;
pub const PM_CMAF_MISSING_MOOV: i32 = 10;
pub const PM_CMAF_NO_AUDIO_TRACK: i32 = 11;
pub const PM_CMAF_INVALID_INIT: i32 = 13;
pub const PM_CMAF_SAMPLE_LIMIT: i32 = 65536;
const BOX_CAP: usize = 64;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct pm_cmaf_sample {
    pub offset: i32,
    pub size: i32,
    pub decode_time: i64,
    pub presentation_time: i64,
    pub duration: i64,
    pub is_sync: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct pm_cmaf_status {
    pub code: i32,
    pub found_track_id: u32,
}

fn fail(st: &mut pm_cmaf_status, code: i32, track: u32) {
    st.code = code;
    st.found_track_id = track;
}

fn fits(offset: i32, bytes: i32, limit: i32) -> bool {
    offset >= 0 && bytes >= 0 && offset <= limit.wrapping_sub(bytes)
}

fn payload_start(b: &pm_iso_box) -> i32 {
    b.start + b.header_size
}

fn payload_end(b: &pm_iso_box) -> i32 {
    b.start + b.size
}

fn find_box(boxes: &[pm_iso_box], type_: u32) -> Option<pm_iso_box> {
    boxes.iter().find(|b| b.type_ == type_).copied()
}

/// A walk result: the stored boxes plus the full count (which may exceed the
/// storage cap, exactly like the C walk).
struct Children {
    boxes: [pm_iso_box; BOX_CAP],
    count: i32,
}

impl Children {
    fn stored(&self) -> &[pm_iso_box] {
        &self.boxes[..(self.count.max(0) as usize).min(BOX_CAP)]
    }
}

fn walk_cap(d: Buf, length: i32, start: i32, end: i32) -> (Children, pm_iso_status) {
    let mut c = Children { boxes: [pm_iso_box::default(); BOX_CAP], count: 0 };
    let mut st = pm_iso_status::default();
    c.count = walk(d, length, start, end, &mut c.boxes, &mut st);
    (c, st)
}

/// `pm_cmaf_walk_children`: failure code depends on the walk status.
fn walk_children(d: Buf, length: i32, parent: &pm_iso_box, st: &mut pm_cmaf_status) -> Option<Children> {
    let (c, walk_st) = walk_cap(d, length, payload_start(parent), payload_end(parent));
    if walk_st.code != PM_ISO_OK && c.count == 0 {
        let code = if walk_st.code == PM_ISO_TRUNCATED { PM_CMAF_TRUNCATED } else { PM_CMAF_MISSING_MOOF };
        fail(st, code, 0);
        return None;
    }
    Some(c)
}

/// `pm_cmaf_walk_range`: always reports truncation.
fn walk_range(d: Buf, length: i32, start: i32, end: i32, st: &mut pm_cmaf_status) -> Option<Children> {
    let (c, walk_st) = walk_cap(d, length, start, end);
    if walk_st.code != PM_ISO_OK && c.count == 0 {
        fail(st, PM_CMAF_TRUNCATED, 0);
        return None;
    }
    Some(c)
}

#[derive(Clone, Copy, Default)]
struct Tfhd {
    track_id: u32,
    default_duration: u32,
    default_size: u32,
    default_flags: u32,
    has_duration: bool,
    has_size: bool,
    has_flags: bool,
}

fn parse_tfhd(d: Buf, length: i32, b: &pm_iso_box, st: &mut pm_cmaf_status) -> Option<Tfhd> {
    let body = payload_start(b);
    let end = payload_end(b);
    if !fits(body, 8, end) || !fits(body, 8, length) {
        fail(st, PM_CMAF_TRUNCATED, 0);
        return None;
    }
    let flags = d.u24(body + 1);
    let mut offset = body + 8;
    let mut h = Tfhd { track_id: d.u32(body + 4), ..Tfhd::default() };
    if flags & 0x1 != 0 {
        if !fits(offset, 8, end) {
            fail(st, PM_CMAF_TRUNCATED, h.track_id);
            return None;
        }
        offset += 8;
    }
    if flags & 0x2 != 0 {
        if !fits(offset, 4, end) {
            fail(st, PM_CMAF_TRUNCATED, h.track_id);
            return None;
        }
        offset += 4;
    }
    if flags & 0x8 != 0 {
        if !fits(offset, 4, end) {
            fail(st, PM_CMAF_TRUNCATED, h.track_id);
            return None;
        }
        h.default_duration = d.u32(offset);
        h.has_duration = true;
        offset += 4;
    }
    if flags & 0x10 != 0 {
        if !fits(offset, 4, end) {
            fail(st, PM_CMAF_TRUNCATED, h.track_id);
            return None;
        }
        h.default_size = d.u32(offset);
        h.has_size = true;
        offset += 4;
    }
    if flags & 0x20 != 0 {
        if !fits(offset, 4, end) {
            fail(st, PM_CMAF_TRUNCATED, h.track_id);
            return None;
        }
        h.default_flags = d.u32(offset);
        h.has_flags = true;
    }
    Some(h)
}

fn parse_tfdt(d: Buf, length: i32, b: &pm_iso_box, st: &mut pm_cmaf_status) -> Option<i64> {
    let body = payload_start(b);
    let end = payload_end(b);
    if !fits(body, 5, end) || !fits(body, 5, length) {
        fail(st, PM_CMAF_TRUNCATED, 0);
        return None;
    }
    if d.u8(body) == 1 {
        if !fits(body + 4, 8, end) {
            fail(st, PM_CMAF_TRUNCATED, 0);
            return None;
        }
        return Some(d.u64(body + 4) as i64);
    }
    if !fits(body + 4, 4, end) {
        fail(st, PM_CMAF_TRUNCATED, 0);
        return None;
    }
    Some(i64::from(d.u32(body + 4)))
}

struct SampleSink<'a> {
    out: Option<&'a mut [pm_cmaf_sample]>,
    written: i32,
}

impl SampleSink<'_> {
    fn store(&mut self, sample: pm_cmaf_sample, st: &mut pm_cmaf_status) -> bool {
        if self.written >= PM_CMAF_SAMPLE_LIMIT {
            fail(st, PM_CMAF_OVERFLOW, 0);
            return false;
        }
        if let Some(out) = self.out.as_deref_mut() {
            if self.written as usize >= out.len() {
                fail(st, PM_CMAF_OVERFLOW, 0);
                return false;
            }
            out[self.written as usize] = sample;
        }
        self.written += 1;
        true
    }
}

#[derive(Clone, Copy)]
struct Defaults {
    duration: u32,
    has_duration: bool,
    size: u32,
    has_size: bool,
    flags: u32,
    has_flags: bool,
}

#[allow(clippy::too_many_arguments)]
fn extract_trun(
    d: Buf,
    length: i32,
    trun: &pm_iso_box,
    moof: &pm_iso_box,
    mdat_start: i32,
    mdat_end: i32,
    tfhd: &Tfhd,
    def: Defaults,
    decode_time: &mut i64,
    sink: &mut SampleSink,
    st: &mut pm_cmaf_status,
) -> bool {
    let body = payload_start(trun);
    let end = payload_end(trun);
    if !fits(body, 8, end) || !fits(body, 8, length) {
        fail(st, PM_CMAF_TRUNCATED, 0);
        return false;
    }
    let flags = d.u24(body + 1);
    let sample_count = d.i32(body + 4);
    if sample_count <= 0 {
        fail(st, PM_CMAF_MISSING_MDAT, 0);
        return false;
    }
    let mut offset = body + 8;
    let mut has_data_offset = false;
    let mut data_offset = 0i32;
    let mut has_first_flags = false;
    let mut first_flags = 0u32;
    if flags & 0x1 != 0 {
        if !fits(offset, 4, end) {
            fail(st, PM_CMAF_TRUNCATED, 0);
            return false;
        }
        data_offset = d.i32(offset);
        has_data_offset = true;
        offset += 4;
    }
    if flags & 0x4 != 0 {
        if !fits(offset, 4, end) {
            fail(st, PM_CMAF_TRUNCATED, 0);
            return false;
        }
        first_flags = d.u32(offset);
        has_first_flags = true;
        offset += 4;
    }

    let mut row = 0i32;
    for bit in [0x100u32, 0x200, 0x400, 0x800] {
        if flags & bit != 0 {
            row += 4;
        }
    }
    if row > 0 && sample_count > (end - offset) / row {
        fail(st, PM_CMAF_TRUNCATED, 0);
        return false;
    }

    let (first_size, has_first_size) = if flags & 0x200 != 0 {
        let at = if flags & 0x100 != 0 { offset + 4 } else { offset };
        (d.u32(at), true)
    } else if tfhd.has_size {
        (tfhd.default_size, true)
    } else if def.has_size {
        (def.size, true)
    } else {
        (0, false)
    };

    let moof_based = moof.start.wrapping_add(if has_data_offset { data_offset } else { 0 });
    let start_inside = moof_based >= mdat_start && moof_based < mdat_end;
    let first_fits = has_first_size && i64::from(moof_based) + i64::from(first_size) <= i64::from(mdat_end);
    let mut sample_offset = if start_inside && first_fits { moof_based } else { mdat_start };

    let mut cursor = offset;
    for index in 0..sample_count {
        let (mut duration, mut has_duration) = (0u32, false);
        let (mut size, mut has_size) = (0u32, false);
        let (mut sample_flags, mut has_flags) = (0u32, false);
        let mut cts = 0i32;
        if flags & 0x100 != 0 {
            duration = d.u32(cursor);
            has_duration = true;
            cursor += 4;
        }
        if flags & 0x200 != 0 {
            size = d.u32(cursor);
            has_size = true;
            cursor += 4;
        }
        if flags & 0x400 != 0 {
            sample_flags = d.u32(cursor);
            has_flags = true;
            cursor += 4;
        }
        if flags & 0x800 != 0 {
            cts = d.i32(cursor);
            cursor += 4;
        }
        if !has_duration {
            if tfhd.has_duration {
                duration = tfhd.default_duration;
                has_duration = true;
            } else if def.has_duration {
                duration = def.duration;
                has_duration = true;
            }
        }
        if !has_size {
            if tfhd.has_size {
                size = tfhd.default_size;
                has_size = true;
            } else if def.has_size {
                size = def.size;
                has_size = true;
            }
        }
        if !has_size || size == 0 {
            fail(st, PM_CMAF_MISSING_SIZE, 0);
            return false;
        }
        let start = sample_offset;
        if size > i32::MAX as u32 || start < mdat_start || i64::from(start) + i64::from(size) > i64::from(mdat_end) {
            fail(st, PM_CMAF_SAMPLE_OUTSIDE, 0);
            return false;
        }

        let non_sync = 0x0001_0000u32;
        let is_sync = if index == 0 && has_first_flags {
            first_flags & non_sync == 0
        } else if has_flags {
            sample_flags & non_sync == 0
        } else if tfhd.has_flags {
            tfhd.default_flags & non_sync == 0
        } else if def.has_flags {
            def.flags & non_sync == 0
        } else {
            true
        };

        let sample = pm_cmaf_sample {
            offset: start,
            size: size as i32,
            decode_time: *decode_time,
            presentation_time: decode_time.wrapping_add(i64::from(cts)),
            duration: if has_duration { i64::from(duration) } else { 0 },
            is_sync,
        };
        if !sink.store(sample, st) {
            return false;
        }
        *decode_time = decode_time.wrapping_add(sample.duration);
        sample_offset = sample_offset.wrapping_add(size as i32);
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn extract_traf(
    d: Buf,
    length: i32,
    traf: &pm_iso_box,
    moof: &pm_iso_box,
    mdat_start: i32,
    mdat_end: i32,
    track_id: u32,
    def: Defaults,
    decode_time: &mut i64,
    has_decode_time: &mut bool,
    sink: &mut SampleSink,
    st: &mut pm_cmaf_status,
) -> bool {
    let Some(children) = walk_children(d, length, traf, st) else { return false };
    let stored = children.stored();
    let Some(tfhd_box) = find_box(stored, tag(b't', b'f', b'h', b'd')) else {
        fail(st, PM_CMAF_MISSING_TFHD, 0);
        return false;
    };
    let Some(tfhd) = parse_tfhd(d, length, &tfhd_box, st) else { return false };
    if tfhd.track_id != track_id {
        fail(st, PM_CMAF_TRACK_MISMATCH, tfhd.track_id);
        return false;
    }
    if let Some(tfdt_box) = find_box(stored, tag(b't', b'f', b'd', b't')) {
        let Some(value) = parse_tfdt(d, length, &tfdt_box, st) else { return false };
        *decode_time = value;
        *has_decode_time = true;
    } else if !*has_decode_time {
        *decode_time = 0;
        *has_decode_time = true;
    }

    let mut saw_trun = false;
    let before = sink.written;
    let trun_tag = tag(b't', b'r', b'u', b'n');
    for child in stored {
        if child.type_ != trun_tag {
            continue;
        }
        saw_trun = true;
        if !extract_trun(d, length, child, moof, mdat_start, mdat_end, &tfhd, def, decode_time, sink, st) {
            return false;
        }
    }
    if !saw_trun || sink.written == before {
        fail(st, PM_CMAF_MISSING_MDAT, 0);
        return false;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn pm_cmaf_extract_fragment(
    data: *const u8,
    length: i32,
    track_id: u32,
    default_duration: u32,
    has_default_duration: bool,
    default_size: u32,
    has_default_size: bool,
    default_flags: u32,
    has_default_flags: bool,
    decode_time: *mut i64,
    has_decode_time: *mut bool,
    out: *mut pm_cmaf_sample,
    out_capacity: i32,
    status: *mut pm_cmaf_status,
) -> i32 {
    let mut local = pm_cmaf_status::default();
    let st = if status.is_null() { &mut local } else { &mut *status };
    st.code = PM_CMAF_OK;
    st.found_track_id = 0;
    if data.is_null() || length < 16 || decode_time.is_null() || has_decode_time.is_null() {
        fail(st, PM_CMAF_MISSING_MOOF, 0);
        return 0;
    }
    let d = Buf(slice::from_raw_parts(data, length as usize));

    let (top, _) = walk_cap(d, length, 0, length);
    let top_boxes = top.stored();
    let Some(moof) = find_box(top_boxes, tag(b'm', b'o', b'o', b'f')) else {
        fail(st, PM_CMAF_MISSING_MOOF, 0);
        return 0;
    };
    let Some(mdat) = find_box(top_boxes, tag(b'm', b'd', b'a', b't')) else {
        fail(st, PM_CMAF_MISSING_MDAT, 0);
        return 0;
    };
    let Some(moof_children) = walk_children(d, length, &moof, st) else { return 0 };

    let mdat_start = payload_start(&mdat);
    let mdat_end = payload_end(&mdat);
    let mut running = *decode_time;
    let mut has_running = *has_decode_time;
    let mut saw_traf = false;
    let out_slice = if out.is_null() { None } else { Some(slice::from_raw_parts_mut(out, out_capacity.max(0) as usize)) };
    let mut sink = SampleSink { out: out_slice, written: 0 };
    let def = Defaults {
        duration: default_duration,
        has_duration: has_default_duration,
        size: default_size,
        has_size: has_default_size,
        flags: default_flags,
        has_flags: has_default_flags,
    };
    let traf_tag = tag(b't', b'r', b'a', b'f');
    for child in moof_children.stored() {
        if child.type_ != traf_tag {
            continue;
        }
        saw_traf = true;
        if !extract_traf(
            d, length, child, &moof, mdat_start, mdat_end, track_id, def, &mut running, &mut has_running, &mut sink, st,
        ) {
            return 0;
        }
    }
    if !saw_traf || sink.written == 0 {
        fail(st, PM_CMAF_MISSING_MDAT, 0);
        return 0;
    }
    *decode_time = running;
    *has_decode_time = has_running;
    st.code = PM_CMAF_OK;
    sink.written
}

// ------------------------------------------------------- CMAF initialization

#[repr(C)]
#[derive(Clone, Copy)]
pub struct pm_cmaf_init {
    pub track_id: u32,
    pub timescale: u32,
    pub sample_rate: u32,
    pub channel_count: u32,
    pub codec_fourcc: u32,
    pub default_sample_duration: u32,
    pub default_sample_size: u32,
    pub default_sample_flags: u32,
    pub has_default_duration: bool,
    pub has_default_size: bool,
    pub has_default_flags: bool,
    pub esds_offset: i32,
    pub esds_length: i32,
    pub asc_offset: i32,
    pub asc_length: i32,
}

impl pm_cmaf_init {
    fn cleared() -> Self {
        pm_cmaf_init {
            track_id: 0,
            timescale: 0,
            sample_rate: 0,
            channel_count: 0,
            codec_fourcc: 0,
            default_sample_duration: 0,
            default_sample_size: 0,
            default_sample_flags: 0,
            has_default_duration: false,
            has_default_size: false,
            has_default_flags: false,
            esds_offset: -1,
            esds_length: 0,
            asc_offset: -1,
            asc_length: 0,
        }
    }
}

fn find_asc(d: Buf, start: i32, end: i32, depth: i32) -> Option<(i32, i32)> {
    if depth > 8 || start < 0 || start > end {
        return None;
    }
    let mut offset = start;
    while offset + 2 <= end {
        let t = d.u8(offset);
        offset += 1;
        let mut size = 0u32;
        let mut read = 0;
        while read < 4 {
            if offset >= end {
                return None;
            }
            let byte = d.u8(offset);
            offset += 1;
            read += 1;
            size = (size << 7) | u32::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                break;
            }
        }
        if size > i32::MAX as u32 {
            return None;
        }
        let payload_size = size as i32;
        if !fits(offset, payload_size, end) {
            break;
        }
        if t == 0x05 {
            return Some((offset, payload_size));
        }
        let payload_end = offset + payload_size;
        let nested_start = match t {
            0x03 => offset + 3,
            0x04 => offset + 13,
            _ => -1,
        };
        if nested_start >= 0 && nested_start < payload_end {
            if let Some(found) = find_asc(d, nested_start, payload_end, depth + 1) {
                return Some(found);
            }
        }
        offset = payload_end;
    }
    None
}

fn parse_tkhd_id(d: Buf, length: i32, b: &pm_iso_box) -> Option<u32> {
    let body = payload_start(b);
    let end = payload_end(b);
    if !fits(body, 1, end) || !fits(body, 1, length) {
        return None;
    }
    let id_offset = if d.u8(body) == 1 { body + 20 } else { body + 12 };
    if !fits(id_offset, 4, end) || !fits(id_offset, 4, length) {
        return None;
    }
    let value = d.u32(id_offset);
    (value != 0).then_some(value)
}

fn parse_mdhd_timescale(d: Buf, length: i32, b: &pm_iso_box) -> Option<u32> {
    let body = payload_start(b);
    let end = payload_end(b);
    if !fits(body, 5, end) || !fits(body, 5, length) {
        return None;
    }
    let at = if d.u8(body) == 1 { body + 20 } else { body + 12 };
    if !fits(at, 4, end) || !fits(at, 4, length) {
        return None;
    }
    let value = d.u32(at);
    (value != 0).then_some(value)
}

fn parse_hdlr_is_audio(d: Buf, length: i32, b: &pm_iso_box) -> Option<bool> {
    let body = payload_start(b);
    let end = payload_end(b);
    if !fits(body, 12, end) || !fits(body, 12, length) {
        return None;
    }
    Some(d.u32(body + 8) == tag(b's', b'o', b'u', b'n'))
}

fn parse_audio_sample_entry(d: Buf, length: i32, entry: &pm_iso_box) -> (u32, u32) {
    let body = payload_start(entry);
    let end = payload_end(entry);
    if !fits(body, 28, end) || !fits(body, 28, length) {
        return (0, 0);
    }
    (u32::from(d.u16(body + 16)), d.u32(body + 24) >> 16)
}

fn parse_esds(d: Buf, length: i32, entry: &pm_iso_box, out: &mut pm_cmaf_init, st: &mut pm_cmaf_status) -> bool {
    let children_start = payload_start(entry) + 28;
    let children_end = payload_end(entry);
    if children_start > children_end {
        return true;
    }
    let Some(nested) = walk_range(d, length, children_start, children_end, st) else { return false };
    let Some(esds) = find_box(nested.stored(), tag(b'e', b's', b'd', b's')) else { return true };
    let descriptor_start = payload_start(&esds) + 4;
    let descriptor_end = payload_end(&esds);
    if descriptor_start >= descriptor_end || !fits(descriptor_start, 0, length) {
        return true;
    }
    out.esds_offset = descriptor_start;
    out.esds_length = descriptor_end - descriptor_start;
    if let Some((offset, size)) = find_asc(d, descriptor_start, descriptor_end, 0) {
        out.asc_offset = offset;
        out.asc_length = size;
    }
    true
}

fn parse_trak(d: Buf, length: i32, trak: &pm_iso_box, st: &mut pm_cmaf_status) -> Option<(pm_cmaf_init, bool)> {
    let trak_children = walk_children(d, length, trak, st)?;
    let tkhd = find_box(trak_children.stored(), tag(b't', b'k', b'h', b'd'));
    let mdia = find_box(trak_children.stored(), tag(b'm', b'd', b'i', b'a'));
    let (tkhd, mdia) = (tkhd?, mdia?);

    let mdia_children = walk_children(d, length, &mdia, st)?;
    let mdhd = find_box(mdia_children.stored(), tag(b'm', b'd', b'h', b'd'));
    let hdlr = find_box(mdia_children.stored(), tag(b'h', b'd', b'l', b'r'));
    let minf = find_box(mdia_children.stored(), tag(b'm', b'i', b'n', b'f'));
    let (mdhd, hdlr, minf) = (mdhd?, hdlr?, minf?);

    let minf_children = walk_children(d, length, &minf, st)?;
    let stbl = find_box(minf_children.stored(), tag(b's', b't', b'b', b'l'))?;
    let stbl_children = walk_children(d, length, &stbl, st)?;
    let stsd = find_box(stbl_children.stored(), tag(b's', b't', b's', b'd'))?;

    let track_id = parse_tkhd_id(d, length, &tkhd)?;
    let timescale = parse_mdhd_timescale(d, length, &mdhd)?;
    let audio = parse_hdlr_is_audio(d, length, &hdlr)?;

    let entries_start = payload_start(&stsd) + 8;
    let entries_end = payload_end(&stsd);
    if entries_start > entries_end {
        return None;
    }
    let entries = walk_range(d, length, entries_start, entries_end, st)?;
    let entry = *entries.stored().first()?;

    let (channels, sample_rate) = parse_audio_sample_entry(d, length, &entry);
    let mut out = pm_cmaf_init::cleared();
    out.track_id = track_id;
    out.timescale = timescale;
    out.sample_rate = sample_rate;
    out.channel_count = channels;
    out.codec_fourcc = entry.type_;
    if entry.type_ == tag(b'm', b'p', b'4', b'a') && !parse_esds(d, length, &entry, &mut out, st) {
        return None;
    }
    Some((out, audio))
}

fn fold_trex(d: Buf, length: i32, moov: &pm_iso_box, out: &mut pm_cmaf_init, st: &mut pm_cmaf_status) {
    let Some(moov_children) = walk_children(d, length, moov, st) else { return };
    let Some(mvex) = find_box(moov_children.stored(), tag(b'm', b'v', b'e', b'x')) else { return };
    let Some(mvex_children) = walk_children(d, length, &mvex, st) else { return };
    let trex_tag = tag(b't', b'r', b'e', b'x');
    for child in mvex_children.stored() {
        if child.type_ != trex_tag {
            continue;
        }
        let body = payload_start(child);
        let end = payload_end(child);
        if !fits(body, 24, end) || !fits(body, 24, length) {
            continue;
        }
        if d.u32(body + 4) != out.track_id {
            continue;
        }
        out.default_sample_duration = d.u32(body + 12);
        out.default_sample_size = d.u32(body + 16);
        out.default_sample_flags = d.u32(body + 20);
        out.has_default_duration = true;
        out.has_default_size = true;
        out.has_default_flags = true;
        return;
    }
}

#[no_mangle]
pub unsafe extern "C" fn pm_cmaf_parse_initialization(
    data: *const u8,
    length: i32,
    out: *mut pm_cmaf_init,
    status: *mut pm_cmaf_status,
) -> i32 {
    let mut local = pm_cmaf_status::default();
    let st = if status.is_null() { &mut local } else { &mut *status };
    st.code = PM_CMAF_OK;
    st.found_track_id = 0;
    if !out.is_null() {
        *out = pm_cmaf_init::cleared();
    }
    if data.is_null() || length < 8 || out.is_null() {
        fail(st, PM_CMAF_INVALID_INIT, 0);
        return 0;
    }
    let d = Buf(slice::from_raw_parts(data, length as usize));

    let Some(top) = walk_range(d, length, 0, length, st) else { return 0 };
    if find_box(top.stored(), tag(b'f', b't', b'y', b'p')).is_none() {
        fail(st, PM_CMAF_MISSING_FTYP, 0);
        return 0;
    }
    let Some(moov) = find_box(top.stored(), tag(b'm', b'o', b'o', b'v')) else {
        fail(st, PM_CMAF_MISSING_MOOV, 0);
        return 0;
    };
    let Some(moov_children) = walk_children(d, length, &moov, st) else { return 0 };

    let mut selected: Option<pm_cmaf_init> = None;
    let mut fallback: Option<pm_cmaf_init> = None;
    let trak_tag = tag(b't', b'r', b'a', b'k');
    for child in moov_children.stored() {
        if child.type_ != trak_tag {
            continue;
        }
        let Some((candidate, is_audio)) = parse_trak(d, length, child, st) else {
            st.code = PM_CMAF_OK;
            st.found_track_id = 0;
            continue;
        };
        if is_audio {
            selected = Some(candidate);
            break;
        }
        if fallback.is_none() {
            fallback = Some(candidate);
        }
    }

    let Some(mut chosen) = selected.or(fallback) else {
        fail(st, PM_CMAF_NO_AUDIO_TRACK, 0);
        return 0;
    };
    fold_trex(d, length, &moov, &mut chosen, st);
    *out = chosen;
    st.code = PM_CMAF_OK;
    st.found_track_id = chosen.track_id;
    1
}

// ---------------------------------------------------------------- VK unmask

pub const PM_VK_UNMASK_NOT_MASKED: i32 = 0;
pub const PM_VK_UNMASK_FAILED: i32 = -1;
pub const PM_VK_UNMASK_OVERFLOW: i32 = -2;

const VK_ALPHABET: &[u8; 65] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN0PQRSTUVWXYZO123456789+/=";

fn vk_alphabet_index(c: u8) -> i32 {
    VK_ALPHABET.iter().position(|&a| a == c).map_or(-1, |i| i as i32)
}

fn vk_find(hay: &[u8], needle: &[u8]) -> i32 {
    if needle.is_empty() || needle.len() > hay.len() {
        return -1;
    }
    hay.windows(needle.len()).position(|w| w == needle).map_or(-1, |i| i as i32)
}

fn vk_decode(value: &[u8], out: &mut [u8]) -> i32 {
    let mut written = 0usize;
    let mut accumulator = 0i32;
    let mut position = 0i32;
    for &c in value {
        let raw = vk_alphabet_index(c);
        if raw < 0 {
            continue;
        }
        accumulator = if position % 4 == 0 { raw } else { 64 * accumulator + raw };
        let previous = position;
        position += 1;
        if previous % 4 == 0 {
            continue;
        }
        if written >= out.len() {
            return PM_VK_UNMASK_OVERFLOW;
        }
        let shift = (-2 * position) & 6;
        out[written] = ((accumulator >> shift) & 255) as u8;
        written += 1;
    }
    if written == 0 {
        PM_VK_UNMASK_FAILED
    } else {
        written as i32
    }
}

#[no_mangle]
pub unsafe extern "C" fn pm_vk_unmask(
    raw: *const u8,
    raw_length: i32,
    user_id: i32,
    out: *mut u8,
    out_capacity: i32,
) -> i32 {
    if raw.is_null() || raw_length <= 0 {
        return PM_VK_UNMASK_FAILED;
    }
    let raw = slice::from_raw_parts(raw, raw_length as usize);
    if vk_find(raw, b"audio_api_unavailable") < 0 {
        return PM_VK_UNMASK_NOT_MASKED;
    }
    let extra = vk_find(raw, b"?extra=");
    if extra < 0 {
        return PM_VK_UNMASK_FAILED;
    }
    let encoded_start = (extra + 7) as usize;
    if encoded_start >= raw.len() {
        return PM_VK_UNMASK_FAILED;
    }
    let Some(rel) = raw[encoded_start..].iter().position(|&b| b == b'#') else {
        return PM_VK_UNMASK_FAILED;
    };
    let hash = encoded_start + rel;
    if hash + 1 >= raw.len() {
        return PM_VK_UNMASK_FAILED;
    }

    let mut url_bytes = [0u8; 4096];
    let mut key_bytes = [0u8; 256];
    let url_decoded = vk_decode(&raw[encoded_start..hash], &mut url_bytes);
    if url_decoded <= 0 {
        return PM_VK_UNMASK_FAILED;
    }
    let key_decoded = vk_decode(&raw[hash + 1..], &mut key_bytes);
    if key_decoded <= 0 {
        return PM_VK_UNMASK_FAILED;
    }
    let key = &key_bytes[..key_decoded as usize];

    let Some(tab) = key.iter().position(|&b| b == 0x0B) else { return PM_VK_UNMASK_FAILED };
    if tab + 1 >= key.len() {
        return PM_VK_UNMASK_FAILED;
    }
    let mut parsed_key = 0i64;
    let mut have_digit = false;
    for &byte in &key[tab + 1..] {
        if byte == 0x0B {
            break;
        }
        if !byte.is_ascii_digit() {
            if !have_digit {
                return PM_VK_UNMASK_FAILED;
            }
            break;
        }
        have_digit = true;
        parsed_key = parsed_key * 10 + i64::from(byte - b'0');
        if parsed_key > i64::from(i32::MAX) {
            return PM_VK_UNMASK_FAILED;
        }
    }
    if !have_digit {
        return PM_VK_UNMASK_FAILED;
    }

    if out.is_null() {
        return PM_VK_UNMASK_FAILED;
    }
    if url_decoded > out_capacity {
        return PM_VK_UNMASK_OVERFLOW;
    }
    let len = url_decoded as usize;
    let out = slice::from_raw_parts_mut(out, len);
    out.copy_from_slice(&url_bytes[..len]);
    let character_length = url_decoded;
    let mut state = (parsed_key as i32) ^ user_id;
    let mut indexes = [0i32; 4096];
    for position in (0..character_length).rev() {
        // Same wrapping arithmetic the C build relied on for large keys.
        state = (character_length.wrapping_mul(position + 1) ^ state.wrapping_add(position)) % character_length;
        if state < 0 {
            state += character_length;
        }
        indexes[position as usize] = state;
    }
    if character_length > 1 {
        for position in 1..character_length {
            let target = indexes[(character_length - 1 - position) as usize] as usize;
            out.swap(position as usize, target);
        }
    }
    character_length
}

// ------------------------------------------------------------ loaded ahead

#[no_mangle]
pub unsafe extern "C" fn pm_buffer_max_loaded_ahead(
    position_seconds: f64,
    end_seconds: *const f64,
    count: i32,
) -> f64 {
    if end_seconds.is_null() || count <= 0 || !position_seconds.is_finite() {
        return 0.0;
    }
    let mut loaded_ahead = 0.0f64;
    for &end in slice::from_raw_parts(end_seconds, count as usize) {
        if !end.is_finite() {
            continue;
        }
        let ahead = end - position_seconds;
        if ahead.is_finite() && ahead > loaded_ahead {
            loaded_ahead = ahead;
        }
    }
    if loaded_ahead < 0.0 {
        0.0
    } else {
        loaded_ahead
    }
}

// ---------------------------------------------------------------- AES-128

pub const PM_AES128_OK: i32 = 0;
pub const PM_AES128_INVALID_ARGUMENT: i32 = -1;
pub const PM_AES128_DECRYPT_FAILED: i32 = -2;
pub const PM_AES128_OUTPUT_TOO_SMALL: i32 = -3;
const BLOCK: usize = 16;

#[cfg(target_vendor = "apple")]
extern "C" {
    fn CCCrypt(
        op: u32,
        alg: u32,
        options: u32,
        key: *const u8,
        key_length: usize,
        iv: *const u8,
        data_in: *const u8,
        data_in_length: usize,
        data_out: *mut u8,
        data_out_available: usize,
        data_out_moved: *mut usize,
    ) -> i32;
}

/// Hardware-backed CommonCrypto on Apple platforms.
#[cfg(target_vendor = "apple")]
unsafe fn cbc_decrypt_pkcs7(ct: &[u8], key: &[u8; 16], iv: &[u8; 16], out: &mut [u8]) -> Option<usize> {
    let mut moved = 0usize;
    // kCCDecrypt = 1, kCCAlgorithmAES = 0, kCCOptionPKCS7Padding = 1.
    let status = CCCrypt(
        1,
        0,
        1,
        key.as_ptr(),
        16,
        iv.as_ptr(),
        ct.as_ptr(),
        ct.len(),
        out.as_mut_ptr(),
        out.len(),
        &mut moved,
    );
    (status == 0 && moved <= out.len()).then_some(moved)
}

#[cfg(not(target_vendor = "apple"))]
unsafe fn cbc_decrypt_pkcs7(ct: &[u8], key: &[u8; 16], iv: &[u8; 16], out: &mut [u8]) -> Option<usize> {
    soft_cbc_decrypt_pkcs7(ct, key, iv, out)
}

/// Portable AES-128 CBC + PKCS#7, used off-Apple (and by tests everywhere).
pub fn soft_cbc_decrypt_pkcs7(ct: &[u8], key: &[u8; 16], iv: &[u8; 16], out: &mut [u8]) -> Option<usize> {
    if ct.is_empty() || ct.len() % BLOCK != 0 || out.len() < ct.len() {
        return None;
    }
    let round_keys = aes::expand_key(key);
    let mut prev = *iv;
    for (i, chunk) in ct.chunks_exact(BLOCK).enumerate() {
        let mut block = [0u8; BLOCK];
        block.copy_from_slice(chunk);
        let cipher = block;
        aes::decrypt_block(&round_keys, &mut block);
        for j in 0..BLOCK {
            block[j] ^= prev[j];
        }
        out[i * BLOCK..(i + 1) * BLOCK].copy_from_slice(&block);
        prev = cipher;
    }
    let pad = usize::from(out[ct.len() - 1]);
    if pad == 0 || pad > BLOCK || pad > ct.len() {
        return None;
    }
    if out[ct.len() - pad..ct.len()].iter().any(|&b| usize::from(b) != pad) {
        return None;
    }
    Some(ct.len() - pad)
}

#[no_mangle]
pub unsafe extern "C" fn pm_aes128_cbc_decrypt(
    ciphertext: *const u8,
    ciphertext_length: i32,
    key: *const u8,
    iv: *const u8,
    out: *mut u8,
    out_capacity: i32,
    out_length: *mut i32,
) -> i32 {
    if out_length.is_null() {
        return PM_AES128_INVALID_ARGUMENT;
    }
    *out_length = 0;
    if ciphertext.is_null() || key.is_null() || iv.is_null() || out.is_null() {
        return PM_AES128_INVALID_ARGUMENT;
    }
    if ciphertext_length <= 0
        || ciphertext_length % BLOCK as i32 != 0
        || i64::from(out_capacity) < i64::from(ciphertext_length) + BLOCK as i64
    {
        return PM_AES128_OUTPUT_TOO_SMALL;
    }
    let ct = slice::from_raw_parts(ciphertext, ciphertext_length as usize);
    let key: &[u8; 16] = &*(key as *const [u8; 16]);
    let iv: &[u8; 16] = &*(iv as *const [u8; 16]);
    let out = slice::from_raw_parts_mut(out, out_capacity as usize);
    match cbc_decrypt_pkcs7(ct, key, iv, out) {
        Some(n) => {
            *out_length = n as i32;
            PM_AES128_OK
        }
        None => PM_AES128_DECRYPT_FAILED,
    }
}

mod aes {
    //! Straightforward AES-128 block decryption (FIPS-197).

    fn build_sbox() -> ([u8; 256], [u8; 256]) {
        let mut sbox = [0u8; 256];
        let mut inv = [0u8; 256];
        let (mut p, mut q) = (1u8, 1u8);
        loop {
            // p *= 3 in GF(2^8)
            p = p ^ (p << 1) ^ if p & 0x80 != 0 { 0x1B } else { 0 };
            // q /= 3
            q ^= q << 1;
            q ^= q << 2;
            q ^= q << 4;
            if q & 0x80 != 0 {
                q ^= 0x09;
            }
            let x = q ^ q.rotate_left(1) ^ q.rotate_left(2) ^ q.rotate_left(3) ^ q.rotate_left(4);
            sbox[p as usize] = x ^ 0x63;
            if p == 1 {
                break;
            }
        }
        sbox[0] = 0x63;
        for (i, &s) in sbox.iter().enumerate() {
            inv[s as usize] = i as u8;
        }
        (sbox, inv)
    }

    fn xtime(x: u8) -> u8 {
        (x << 1) ^ if x & 0x80 != 0 { 0x1B } else { 0 }
    }

    fn mul(mut a: u8, mut b: u8) -> u8 {
        let mut r = 0;
        while b != 0 {
            if b & 1 != 0 {
                r ^= a;
            }
            a = xtime(a);
            b >>= 1;
        }
        r
    }

    pub struct RoundKeys {
        keys: [[u8; 16]; 11],
        inv_sbox: [u8; 256],
    }

    pub fn expand_key(key: &[u8; 16]) -> RoundKeys {
        let (sbox, inv_sbox) = build_sbox();
        let mut w = [[0u8; 4]; 44];
        for i in 0..4 {
            w[i].copy_from_slice(&key[i * 4..i * 4 + 4]);
        }
        let mut rcon = 1u8;
        for i in 4..44 {
            let mut t = w[i - 1];
            if i % 4 == 0 {
                t = [sbox[t[1] as usize] ^ rcon, sbox[t[2] as usize], sbox[t[3] as usize], sbox[t[0] as usize]];
                rcon = xtime(rcon);
            }
            for j in 0..4 {
                w[i][j] = w[i - 4][j] ^ t[j];
            }
        }
        let mut keys = [[0u8; 16]; 11];
        for (r, k) in keys.iter_mut().enumerate() {
            for c in 0..4 {
                k[c * 4..c * 4 + 4].copy_from_slice(&w[r * 4 + c]);
            }
        }
        RoundKeys { keys, inv_sbox }
    }

    fn add_round_key(s: &mut [u8; 16], k: &[u8; 16]) {
        for i in 0..16 {
            s[i] ^= k[i];
        }
    }

    fn inv_shift_rows(s: &mut [u8; 16]) {
        let t = *s;
        for c in 0..4 {
            for r in 0..4 {
                s[((c + r) % 4) * 4 + r] = t[c * 4 + r];
            }
        }
    }

    fn inv_mix_columns(s: &mut [u8; 16]) {
        for c in 0..4 {
            let a = [s[c * 4], s[c * 4 + 1], s[c * 4 + 2], s[c * 4 + 3]];
            s[c * 4] = mul(a[0], 14) ^ mul(a[1], 11) ^ mul(a[2], 13) ^ mul(a[3], 9);
            s[c * 4 + 1] = mul(a[0], 9) ^ mul(a[1], 14) ^ mul(a[2], 11) ^ mul(a[3], 13);
            s[c * 4 + 2] = mul(a[0], 13) ^ mul(a[1], 9) ^ mul(a[2], 14) ^ mul(a[3], 11);
            s[c * 4 + 3] = mul(a[0], 11) ^ mul(a[1], 13) ^ mul(a[2], 9) ^ mul(a[3], 14);
        }
    }

    pub fn decrypt_block(rk: &RoundKeys, s: &mut [u8; 16]) {
        add_round_key(s, &rk.keys[10]);
        for round in (1..10).rev() {
            inv_shift_rows(s);
            for b in s.iter_mut() {
                *b = rk.inv_sbox[*b as usize];
            }
            add_round_key(s, &rk.keys[round]);
            inv_mix_columns(s);
        }
        inv_shift_rows(s);
        for b in s.iter_mut() {
            *b = rk.inv_sbox[*b as usize];
        }
        add_round_key(s, &rk.keys[0]);
    }
}
