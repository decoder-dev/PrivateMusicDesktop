//! Выбирает два цвета из обложки, чтобы их можно было носить в полноэкранном режиме.
//!
//! Веб-представление не может считывать пиксели изображения из разных источников, поэтому движок
//! загружает обложку (только https, никогда не локальный адрес, с ограничением по размеру), сжимает ее
//! и смотрит на цвета, которые являются как общими, так и яркими.

use std::io::Read;
use std::time::Duration;

use image::imageops::FilterType;
use serde::Serialize;
use url::Url;

use crate::stream::fetch::{is_local_host, PublicResolver};
use crate::{Error, Result};

const MAX_BYTES: u64 = 3 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Palette {
    /// Самый заметный яркий цвет, например `#rrggbb`.
    pub primary: String,
    /// Второй цвет, явно отличающийся от первого.
    pub secondary: String,
}

pub fn palette_from_url(raw: &str) -> Result<Palette> {
    palette_from_image(&image_bytes(raw)?)
}

fn http_client() -> Result<&'static reqwest::blocking::Client> {
    static CLIENT: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .dns_resolver(std::sync::Arc::new(PublicResolver { allow_loopback: false }))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 || attempt.url().scheme() != "https" || is_local_host(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()?;
    Ok(CLIENT.get_or_init(|| built))
}

fn image_bytes(raw: &str) -> Result<Vec<u8>> {
    let url = Url::parse(raw).map_err(|_| Error::InvalidRequest)?;
    if url.scheme() != "https" || is_local_host(&url) || !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InvalidRequest);
    }
    let response = http_client()?.get(url).send()?;
    if !response.status().is_success() {
        return Err(Error::Http(response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    response.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|e| crate::net::read_error(&e))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::Format("cover is too large".into()));
    }
    Ok(bytes)
}

/// Bounded, metadata-free JPEG for embedding in an exported audio file.
pub fn export_jpeg(raw:&str)->Result<Vec<u8>> {
    let bytes=image_bytes(raw)?;
    let decoded=decode_image(&bytes)?;
    let image=decoded.thumbnail(800,800).to_rgb8();
    let mut result=Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut result,85).encode_image(&image).map_err(|_|Error::Unavailable)?;
    Ok(result)
}

fn decode_image(bytes:&[u8])->Result<image::DynamicImage> {
    let mut reader=image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|_|Error::InvalidRequest)?;
    let mut limits=image::Limits::default();limits.max_image_width=Some(8192);limits.max_image_height=Some(8192);limits.max_alloc=Some(64*1024*1024);reader.limits(limits);
    reader.decode().map_err(|_|Error::Format("cover is not a supported image".into()))
}

pub fn palette_from_image(bytes: &[u8]) -> Result<Palette> {
    let decoded = decode_image(bytes)?;
    let small = decoded.resize_exact(24, 24, FilterType::Triangle).to_rgb8();
    Ok(pick(small.pixels().map(|p| (p[0], p[1], p[2]))))
}

fn hsv((r, g, b): (u8, u8, u8)) -> (f32, f32, f32) {
    let (r, g, b) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    (hue.rem_euclid(360.0), if max == 0.0 { 0.0 } else { delta / max }, max)
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs();
    d.min(360.0 - d)
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Цветовые сегменты (4 бита на канал), каждый из которых взвешивается по тому, насколько яркими и
/// яркими являются его пиксели, поэтому серая или черная рамка не может превзойти изображение.
fn pick(pixels: impl Iterator<Item = (u8, u8, u8)>) -> Palette {
    struct Bucket {
        weight: f32,
        sum: [f32; 3],
        count: f32,
    }
    let mut buckets: Vec<Bucket> = (0..4096).map(|_| Bucket { weight: 0.0, sum: [0.0; 3], count: 0.0 }).collect();
    let mut all = [0.0f32; 3];
    let mut total = 0.0f32;
    for pixel in pixels {
        let (_, saturation, value) = hsv(pixel);
        let index = (usize::from(pixel.0 >> 4) << 8) | (usize::from(pixel.1 >> 4) << 4) | usize::from(pixel.2 >> 4);
        let weight = 0.05 + saturation * saturation * value.max(0.2);
        let bucket = &mut buckets[index];
        bucket.weight += weight;
        bucket.count += 1.0;
        bucket.sum[0] += f32::from(pixel.0);
        bucket.sum[1] += f32::from(pixel.1);
        bucket.sum[2] += f32::from(pixel.2);
        all[0] += f32::from(pixel.0);
        all[1] += f32::from(pixel.1);
        all[2] += f32::from(pixel.2);
        total += 1.0;
    }
    let mean = |b: &Bucket| ((b.sum[0] / b.count) as u8, (b.sum[1] / b.count) as u8, (b.sum[2] / b.count) as u8);
    let mut ranked: Vec<&Bucket> = buckets.iter().filter(|b| b.count > 0.0).collect();
    ranked.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    let Some(first) = ranked.first() else {
        return Palette { primary: "#3385fa".into(), secondary: "#1a6be0".into() };
    };
    let primary = mean(first);
    let (primary_hue, primary_saturation, _) = hsv(primary);
    let secondary = ranked
        .iter()
        .skip(1)
        .map(|b| mean(b))
        .find(|&c| {
            let (hue, saturation, value) = hsv(c);
            saturation > 0.25 && value > 0.25 && (hue_distance(hue, primary_hue) > 35.0 || primary_saturation < 0.2)
        })
        .unwrap_or_else(|| {
            // Одноцветная обложка: используйте средний цвет обложки, который все еще соответствует
            // теме.
            let average = ((all[0] / total) as u8, (all[1] / total) as u8, (all[2] / total) as u8);
            if average == primary { (primary.2, primary.0, primary.1) } else { average }
        });
    Palette { primary: hex(primary), secondary: hex(secondary) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder, RgbImage};

    fn png(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut image = RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = image::Rgb(paint(x, y));
        }
        let mut out = Vec::new();
        PngEncoder::new(&mut out).write_image(image.as_raw(), width, height, ExtendedColorType::Rgb8).unwrap();
        out
    }

    fn rgb(hex: &str) -> (u8, u8, u8) {
        let channel = |i: usize| u8::from_str_radix(&hex[1 + i * 2..3 + i * 2], 16).unwrap();
        (channel(0), channel(1), channel(2))
    }

    #[test]
    fn a_vivid_colour_beats_a_big_grey_border() {
        // Серая рамка вокруг красного центра.
        let cover = png(96, 96, |x, y| if (24..72).contains(&x) && (24..72).contains(&y) { [220, 30, 40] } else { [120, 120, 120] });
        let palette = palette_from_image(&cover).unwrap();
        let (hue, saturation, _) = hsv(rgb(&palette.primary));
        assert!(saturation > 0.6 && !(20.0..340.0).contains(&hue), "{palette:?}");
    }

    #[test]
    fn two_halves_give_two_different_colours() {
        let cover = png(64, 64, |x, _| if x < 32 { [230, 40, 40] } else { [40, 90, 230] });
        let palette = palette_from_image(&cover).unwrap();
        assert_ne!(palette.primary, palette.secondary);
        let both = [rgb(&palette.primary), rgb(&palette.secondary)];
        let reds = both.iter().filter(|c| c.0 > 150).count();
        let blues = both.iter().filter(|c| c.2 > 150).count();
        assert_eq!((reds, blues), (1, 1), "{palette:?}");
    }

    #[test]
    fn a_flat_cover_still_gets_two_colours_and_garbage_is_an_error() {
        let flat = palette_from_image(&png(32, 32, |_, _| [30, 160, 90])).unwrap();
        assert_ne!(flat.primary, flat.secondary);
        assert!(palette_from_image(b"not an image").is_err());
        assert!(palette_from_image(&[]).is_err());
    }

    #[test]
    fn only_public_https_covers_are_fetched() {
        for bad in ["http://cdn.example/a.jpg", "https://127.0.0.1/a.jpg", "https://192.168.0.2/a.jpg", "file:///etc/passwd", "https://u:p@cdn.example/a.jpg", "nonsense"] {
            assert!(matches!(palette_from_url(bad), Err(Error::InvalidRequest)), "{bad}");
        }
    }
}
