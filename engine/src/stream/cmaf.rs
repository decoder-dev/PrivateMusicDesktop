//! Fragmented MP4 (CMAF) HLS audio: the `moov` of the init section tells us
//! the AAC configuration, each `moof`/`mdat` fragment holds raw AAC frames.
//! Both are parsed by `pm_native`; here the frames are wrapped in ADTS
//! headers so the rest of the engine sees one kind of stream.

use pm_native::media::{
    pm_cmaf_extract_fragment, pm_cmaf_init, pm_cmaf_parse_initialization, pm_cmaf_sample, pm_cmaf_status, PM_CMAF_OK,
    PM_CMAF_SAMPLE_LIMIT,
};

use crate::{Error, Result};

/// `mp4a`
const FOURCC_MP4A: u32 = 0x6D70_3461;

pub struct Cmaf {
    track_id: u32,
    default_duration: Option<u32>,
    default_size: Option<u32>,
    default_flags: Option<u32>,
    audio_object_type: u8,
    frequency_index: u8,
    channel_config: u8,
    decode_time: i64,
    has_decode_time: bool,
}

struct Bits<'a> {
    data: &'a [u8],
    position: usize,
}

impl Bits<'_> {
    fn read(&mut self, count: usize) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            let byte = *self.data.get(self.position / 8)?;
            value = (value << 1) | u32::from((byte >> (7 - self.position % 8)) & 1);
            self.position += 1;
        }
        Some(value)
    }
}

/// (audio object type, sampling frequency index, channel configuration)
fn parse_audio_specific_config(asc: &[u8]) -> Result<(u8, u8, u8)> {
    let bad = || Error::Stream("unsupported AAC configuration".into());
    let mut bits = Bits { data: asc, position: 0 };
    let mut object_type = bits.read(5).ok_or_else(bad)?;
    if object_type == 31 {
        object_type = 32 + bits.read(6).ok_or_else(bad)?;
    }
    let frequency_index = bits.read(4).ok_or_else(bad)?;
    if frequency_index == 15 {
        // An explicit rate cannot be signalled in an ADTS header.
        return Err(bad());
    }
    let channels = bits.read(4).ok_or_else(bad)?;
    // ADTS can only carry AAC Main, LC, SSR and LTP.
    if !(1..=4).contains(&object_type) || channels == 0 || channels > 7 {
        return Err(bad());
    }
    Ok((object_type as u8, frequency_index as u8, channels as u8))
}

impl Cmaf {
    pub fn parse_init(data: &[u8]) -> Result<Self> {
        if data.is_empty() || data.len() > i32::MAX as usize {
            return Err(Error::Stream("invalid initialization section".into()));
        }
        // SAFETY: pm_cmaf_init and pm_cmaf_status are plain C structs; the
        // zeroed value is what the native code expects to be overwritten.
        let mut init: pm_cmaf_init = unsafe { std::mem::zeroed() };
        let mut status = pm_cmaf_status::default();
        // SAFETY: `data` is valid for `data.len()` bytes; `init`/`status` are
        // valid for writes.
        let ok = unsafe { pm_cmaf_parse_initialization(data.as_ptr(), data.len() as i32, &mut init, &mut status) };
        if ok != 1 || status.code != PM_CMAF_OK {
            return Err(Error::Stream(format!("invalid fragmented MP4 initialization (code {})", status.code)));
        }
        if init.codec_fourcc != FOURCC_MP4A {
            return Err(Error::Stream("only AAC is supported in fragmented MP4".into()));
        }
        let start = usize::try_from(init.asc_offset).map_err(|_| Error::Stream("missing AAC configuration".into()))?;
        let length = usize::try_from(init.asc_length).map_err(|_| Error::Stream("missing AAC configuration".into()))?;
        let asc = start
            .checked_add(length)
            .and_then(|end| data.get(start..end))
            .ok_or_else(|| Error::Stream("AAC configuration is outside the section".into()))?;
        let (audio_object_type, frequency_index, channel_config) = parse_audio_specific_config(asc)?;
        Ok(Self {
            track_id: init.track_id,
            default_duration: init.has_default_duration.then_some(init.default_sample_duration),
            default_size: init.has_default_size.then_some(init.default_sample_size),
            default_flags: init.has_default_flags.then_some(init.default_sample_flags),
            audio_object_type,
            frequency_index,
            channel_config,
            decode_time: 0,
            has_decode_time: false,
        })
    }

    fn call(&mut self, data: &[u8], out: Option<&mut [pm_cmaf_sample]>) -> Result<i32> {
        let mut status = pm_cmaf_status::default();
        let (pointer, capacity) = match out {
            Some(slice) => (slice.as_mut_ptr(), slice.len() as i32),
            None => (std::ptr::null_mut(), 0),
        };
        // SAFETY: `data` and `out` are valid for the lengths passed; the
        // decode-time pointers refer to fields of `self`.
        let count = unsafe {
            pm_cmaf_extract_fragment(
                data.as_ptr(),
                data.len() as i32,
                self.track_id,
                self.default_duration.unwrap_or(0),
                self.default_duration.is_some(),
                self.default_size.unwrap_or(0),
                self.default_size.is_some(),
                self.default_flags.unwrap_or(0),
                self.default_flags.is_some(),
                &mut self.decode_time,
                &mut self.has_decode_time,
                pointer,
                capacity,
                &mut status,
            )
        };
        if status.code != PM_CMAF_OK || count < 0 {
            return Err(Error::Stream(format!("invalid media fragment (code {})", status.code)));
        }
        Ok(count)
    }

    /// One fragment's AAC frames as ADTS.
    pub fn fragment_to_adts(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 16 || data.len() > i32::MAX as usize {
            return Err(Error::Stream("media fragment is too small".into()));
        }
        // Two passes like the iOS code: count first, then fill. The decode
        // clock must not advance during the count.
        let (saved_time, saved_has) = (self.decode_time, self.has_decode_time);
        let total = self.call(data, None)?;
        self.decode_time = saved_time;
        self.has_decode_time = saved_has;
        if total <= 0 || total > PM_CMAF_SAMPLE_LIMIT {
            return Err(Error::Stream("fragment has no audio samples".into()));
        }
        let mut samples = vec![pm_cmaf_sample::default(); total as usize];
        let filled = self.call(data, Some(&mut samples))? as usize;
        let mut out = Vec::with_capacity(data.len() + filled * 7);
        for sample in samples.iter().take(filled) {
            let start = usize::try_from(sample.offset).map_err(|_| Error::Stream("bad sample offset".into()))?;
            let size = usize::try_from(sample.size).map_err(|_| Error::Stream("bad sample size".into()))?;
            let frame = start
                .checked_add(size)
                .and_then(|end| data.get(start..end))
                .ok_or_else(|| Error::Stream("sample outside the fragment".into()))?;
            let length = frame.len() + 7;
            if length > 0x1FFF {
                return Err(Error::Stream("AAC frame is too large for ADTS".into()));
            }
            out.extend_from_slice(&[
                0xFF,
                0xF1,
                ((self.audio_object_type - 1) << 6) | (self.frequency_index << 2) | (self.channel_config >> 2),
                ((self.channel_config & 3) << 6) | ((length >> 11) as u8 & 3),
                (length >> 3) as u8,
                (((length & 7) as u8) << 5) | 0x1F,
                0xFC,
            ]);
            out.extend_from_slice(frame);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_specific_config_aac_lc_44100_stereo() {
        // 00010 0100 0010 000 -> object 2, index 4 (44100), channels 2
        assert_eq!(parse_audio_specific_config(&[0x12, 0x10]).unwrap(), (2, 4, 2));
    }

    #[test]
    fn unsupported_configs_are_rejected() {
        // HE-AAC (object 5), explicit rate (index 15), empty, zero channels.
        for asc in [&[0x2B, 0x92][..], &[0x17, 0x80, 0x00][..], &[][..], &[0x12, 0x00][..]] {
            assert!(parse_audio_specific_config(asc).is_err(), "{asc:?}");
        }
    }

    #[test]
    fn garbage_init_is_an_error_not_a_crash() {
        assert!(Cmaf::parse_init(&[]).is_err());
        assert!(Cmaf::parse_init(&[0u8; 64]).is_err());
        assert!(Cmaf::parse_init(b"ftypisom\0\0\0\0").is_err());
    }
}
