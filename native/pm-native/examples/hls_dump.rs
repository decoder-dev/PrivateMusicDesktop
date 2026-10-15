//! Prints the same normalized dump as tools/hls_oracle's Swift program so the
//! two parsers can be diffed: `cargo run --example hls_dump -- files...`.

use pm_native::hls::*;

const BASE: &str = "https://h/p/";

fn resolve(s: &str) -> String {
    // Absolute iff it starts with a URL scheme (letter, then letters, digits, + - .).
    let scheme = s.find(':').is_some_and(|i| {
        let head = &s[..i];
        head.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && head.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    });
    if scheme {
        s.to_string()
    } else {
        format!("{BASE}{s}")
    }
}

fn text<'a>(t: &'a str, s: pm_hls_slice) -> &'a str {
    &t[s.offset as usize..(s.offset + s.length) as usize]
}

fn hex(iv: Option<[u8; 16]>) -> String {
    iv.map_or("nil".into(), |b| b.iter().map(|x| format!("{x:02x}")).collect())
}

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(t) = String::from_utf8(bytes) else { continue };
        println!("== {path}");
        let (code, found) = choose_audio(&t);
        if code == PM_HLS_CHOOSE_FOUND {
            println!("choose {code} {}", resolve(text(&t, found)));
        } else {
            println!("choose {code} -");
        }
        let m = parse_media(&t, 4000);
        match m.code {
            PM_HLS_OK => {
                println!("media ok seq={} n={}", m.media_sequence, m.segments.len());
                let range = |has: bool, len: i64, has_off: bool, off: i64| {
                    if !has {
                        "-".to_string()
                    } else if has_off {
                        format!("{len}@{off}")
                    } else {
                        format!("{len}@nil")
                    }
                };
                let key = |i: i32| {
                    if i < 0 {
                        "-".to_string()
                    } else {
                        let k = &m.keys[i as usize];
                        format!("{}#{}", resolve(text(&t, k.uri)), hex(k.has_iv.then_some(k.iv)))
                    }
                };
                for s in &m.segments {
                    let init = if s.map_index < 0 {
                        "-".to_string()
                    } else {
                        let mp = &m.maps[s.map_index as usize];
                        format!(
                            "{}|{}|{}",
                            resolve(text(&t, mp.uri)),
                            range(mp.has_range, mp.range_length, mp.has_range_offset, mp.range_offset),
                            key(mp.key_index)
                        )
                    };
                    println!(
                        "seg {} r={} k={} m={} d={}",
                        resolve(text(&t, s.uri)),
                        range(s.has_range, s.range_length, s.has_range_offset, s.range_offset),
                        key(s.key_index),
                        init,
                        s.discontinuity as i32
                    );
                }
            }
            PM_HLS_UNSUPPORTED_ENCRYPTION_METHOD => {
                println!("media err unsupportedEncryptionMethod {}", text(&t, m.detail))
            }
            c => println!(
                "media err {}",
                match c {
                    PM_HLS_INVALID_ENCRYPTION_KEY => "invalidEncryptionKey",
                    PM_HLS_INVALID_INITIALIZATION_SECTION => "invalidInitializationSection",
                    PM_HLS_INVALID_BYTE_RANGE => "invalidByteRange",
                    PM_HLS_ENCRYPTED_INIT_NEEDS_IV => "encryptedInitializationRequiresExplicitIV",
                    PM_HLS_TOO_MANY_SEGMENTS => "tooManySegments",
                    _ => "?",
                }
            ),
        }
    }
}
