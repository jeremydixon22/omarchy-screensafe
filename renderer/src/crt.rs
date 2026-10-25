//! Bounded, cached inverse barrel sampling for the native video CRT surround.
//! Geometry follows Multimillion's CRT opening: n -> n * (1 + curve * dot(n,n)).

use crate::pack::Fit;
use std::sync::{Arc, Weak};
use tiny_skia::Pixmap;

const MAP_BYTES: usize = 16 << 20;
const PIXEL_BYTES: usize = 8 << 20;
const CLEAR: u32 = u32::MAX;
const BLACK: u32 = u32::MAX - 1;

#[derive(Clone, Copy, PartialEq)]
struct Key {
    source: (u32, u32),
    size: (u32, u32),
    cover: bool,
    curve: u32,
    zoom: u32,
}

struct Tap {
    offset: u32,
    fx: u16,
    fy: u16,
    shade: [u8; 3],
    alpha: u8,
}
struct Map {
    key: Key,
    taps: Vec<Tap>,
}
struct Output {
    key: Key,
    input: Weak<Pixmap>,
    pixels: Arc<Pixmap>,
}

#[derive(Default)]
pub struct Cache {
    maps: Vec<Map>,
    outputs: Vec<Output>,
}

impl Cache {
    pub fn sample(&mut self, input: &Arc<Pixmap>, size: (u32, u32), fit: Fit, curve: f32, zoom: f32) -> Option<Arc<Pixmap>> {
        let key = Key { source: (input.width(), input.height()), size, cover: matches!(fit, Fit::Cover), curve: curve.to_bits(), zoom: zoom.to_bits() };
        let weak = Arc::downgrade(input);
        if let Some(i) = self.outputs.iter().position(|o| o.key == key && o.input.ptr_eq(&weak)) {
            let output = self.outputs.remove(i);
            let pixels = Arc::clone(&output.pixels);
            self.outputs.push(output);
            return Some(pixels);
        }
        let bytes = size.0 as usize * size.1 as usize * std::mem::size_of::<Tap>();
        if size.0 == 0 || size.1 == 0 || size.0 > 1280 || size.1 > 720 || bytes > MAP_BYTES {
            return None;
        }
        let map = match self.maps.iter().position(|m| m.key == key) {
            Some(i) => self.maps.remove(i),
            None => Map::new(key),
        };
        let mut output = Pixmap::new(size.0, size.1)?;
        let src = input.data();
        let dx = usize::from(input.width() > 1) * 4;
        let dy = usize::from(input.height() > 1) * input.width() as usize * 4;
        for (dst, tap) in output.data_mut().chunks_exact_mut(4).zip(&map.taps) {
            if tap.offset == CLEAR {
                continue;
            }
            if tap.offset == BLACK {
                dst[3] = tap.alpha;
                continue;
            }
            let at = tap.offset as usize;
            let (fx, fy) = (tap.fx as u32, tap.fy as u32);
            for c in 0..3 {
                let top = src[at + c] as u32 * (256 - fx) + src[at + dx + c] as u32 * fx;
                let bottom = src[at + dy + c] as u32 * (256 - fx) + src[at + dy + dx + c] as u32 * fx;
                let value = (top * (256 - fy) + bottom * fy + 32768) >> 16;
                dst[c] = ((value * tap.shade[c] as u32 + 127) / 255) as u8;
            }
            dst[3] = tap.alpha; // Opaque JPEG, with cached coverage of the curved glass edge.
        }
        while self.maps.len() >= 4 || self.maps.iter().map(|m| m.taps.len() * std::mem::size_of::<Tap>()).sum::<usize>() + bytes > MAP_BYTES {
            self.maps.remove(0);
        }
        self.maps.push(map);
        let pixels = Arc::new(output);
        // A weak input identity permits reuse of a held frame without retaining extra decoded video frames.
        self.outputs.retain(|o| o.key != key);
        while !self.outputs.is_empty()
            && (self.outputs.len() >= 2 || self.outputs.iter().map(|o| o.pixels.data().len()).sum::<usize>() + pixels.data().len() > PIXEL_BYTES)
        {
            self.outputs.remove(0);
        }
        self.outputs.push(Output { key, input: weak, pixels: Arc::clone(&pixels) });
        Some(pixels)
    }
}

impl Map {
    fn new(key: Key) -> Self {
        let (sw, sh) = (key.source.0 as f64, key.source.1 as f64);
        let (w, h) = (key.size.0 as f64, key.size.1 as f64);
        let scale = if key.cover { (w / sw).max(h / sh) } else { (w / sw).min(h / sh) } * f32::from_bits(key.zoom) as f64;
        let (iw, ih) = (sw * scale, sh * scale);
        let curve = f32::from_bits(key.curve) as f64;
        let mut taps = Vec::with_capacity((w * h) as usize);
        for y in 0..key.size.1 {
            for x in 0..key.size.0 {
                let (nx, ny) = (2.0 * (x as f64 + 0.5) / w - 1.0, 2.0 * (y as f64 + 0.5) / h - 1.0);
                let r2 = nx * nx + ny * ny;
                let k = 1.0 + curve * r2;
                let (u, v) = (nx * k, ny * k);
                let (sx, sy) = ((u * w / 2.0 + iw / 2.0) / scale - 0.5, (v * h / 2.0 + ih / 2.0) / scale - 0.5);
                // Signed distance in output pixels, including each axis's barrel derivative.
                // Cache edge coverage too: both the tube outline and a letterboxed picture stay smooth
                // when the bounded video canvas is enlarged by the compositor.
                let (du, dv) = (1.0 + curve * (3.0 * nx * nx + ny * ny), 1.0 + curve * (nx * nx + 3.0 * ny * ny));
                let tube = ((1.0 - u.abs()) * w / (2.0 * du) + 0.5).min((1.0 - v.abs()) * h / (2.0 * dv) + 0.5).clamp(0.0, 1.0);
                let picture = ((sx + 0.5) * scale / du + 0.5)
                    .min((sw - 0.5 - sx) * scale / du + 0.5)
                    .min((sy + 0.5) * scale / dv + 0.5)
                    .min((sh - 0.5 - sy) * scale / dv + 0.5)
                    .clamp(0.0, 1.0);
                let mut tap = Tap { offset: CLEAR, fx: 0, fy: 0, shade: [255; 3], alpha: (tube * 255.0).round() as u8 };
                if tap.alpha > 0 {
                    tap.offset = BLACK;
                    if picture > 0.0 {
                        let (sx, sy) = (sx.clamp(0.0, sw - 1.0), sy.clamp(0.0, sh - 1.0));
                        let (ix, iy) = ((sx.floor() as u32).min(key.source.0.saturating_sub(2)), (sy.floor() as u32).min(key.source.1.saturating_sub(2)));
                        tap.offset = (iy * key.source.0 + ix) * 4;
                        tap.fx = ((sx - ix as f64) * 256.0).round() as u16;
                        tap.fy = ((sy - iy as f64) * 256.0).round() as u16;
                        // Fixed faint scanlines, aperture mask and edge falloff; no temporal CRT flicker.
                        let light = tube.min(picture) * (1.0 - 0.055 * r2.min(1.5)) * if y % 3 == 2 { 0.94 } else { 1.0 };
                        tap.shade = std::array::from_fn(|c| (255.0 * light * if x as usize % 3 == c { 1.0 } else { 0.975 }).round() as u8);
                    }
                }
                taps.push(tap);
            }
        }
        Self { key, taps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curved_map_bows_edges_and_reuses_held_pixels_without_pinning_input() {
        let mut image = Pixmap::new(64, 48).unwrap();
        image.fill(tiny_skia::Color::WHITE);
        let image = Arc::new(image);
        let mut cache = Cache::default();
        let a = cache.sample(&image, (64, 48), Fit::Contain, 0.045, 1.0).unwrap();
        assert_eq!(a.pixel(0, 0).unwrap().alpha(), 0);
        assert_eq!(a.pixel(32, 24).unwrap().alpha(), 255);
        let b = cache.sample(&image, (64, 48), Fit::Contain, 0.045, 1.0).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(Arc::strong_count(&image), 1);
        assert_eq!(cache.maps.len(), 1);
        assert!(a.data().chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255), "curved outline has antialiased coverage");
        assert_eq!(std::mem::size_of::<Tap>(), 12, "edge coverage must not enlarge the map cache");
        let map = &cache.maps[0];
        let center = map.taps[24 * 64 + 48].offset;
        let edge = map.taps[4 * 64 + 48].offset;
        assert!(edge % (64 * 4) >= center % (64 * 4), "vertical grid line bows out toward the edge");
        for n in 0..8 {
            cache.sample(&image, (100 + n, 80), Fit::Cover, 0.045, 1.0).unwrap();
        }
        assert!(cache.maps.len() <= 4 && cache.outputs.len() <= 2);
        assert!(cache.sample(&image, (1281, 720), Fit::Contain, 0.045, 1.0).is_none());
    }
    #[test]
    fn single_pixel_sources_and_contain_bars_are_safe() {
        let mut image = Pixmap::new(1, 1).unwrap();
        image.fill(tiny_skia::Color::WHITE);
        let image = Arc::new(image);
        let mut cache = Cache::default();
        let p = cache.sample(&image, (80, 40), Fit::Contain, 0.045, 1.0).unwrap();
        assert_eq!(p.pixel(4, 20).unwrap().red(), 0);
        assert!(p.pixel(40, 20).unwrap().red() > 200);
        assert!(p.data().chunks_exact(4).all(|p| p[..3].iter().all(|c| *c <= p[3])));
    }

    #[test]
    fn fit_fills_the_tube_and_crops_sides_while_box_keeps_them() {
        let mut image = Pixmap::new(16, 9).unwrap();
        image.fill(tiny_skia::Color::WHITE);
        let image = Arc::new(image);
        let mut cache = Cache::default();
        let boxed = cache.sample(&image, (64, 48), Fit::Contain, 0.045, 1.0).unwrap();
        let fitted = cache.sample(&image, (64, 48), Fit::Cover, 0.045, 1.0).unwrap();
        assert_eq!(boxed.pixel(32, 3).unwrap().red(), 0, "wide content has letterbox bars");
        assert!(fitted.pixel(32, 3).unwrap().red() > 200, "fit fills the same tube opening");
        let mut bands = Pixmap::new(16, 9).unwrap();
        for (i, px) in bands.data_mut().chunks_exact_mut(4).enumerate() {
            px.copy_from_slice(if i % 16 < 2 || i % 16 >= 14 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
        }
        let bands = Arc::new(bands);
        let boxed = cache.sample(&bands, (64, 48), Fit::Contain, 0.045, 1.0).unwrap();
        let fitted = cache.sample(&bands, (64, 48), Fit::Cover, 0.045, 1.0).unwrap();
        assert!(boxed.pixel(4, 24).unwrap().red() > 200, "box preserves the source's left edge");
        assert!(fitted.pixel(4, 24).unwrap().blue() > 200, "fit crops away that edge");
        assert_eq!(cache.maps.len(), 2, "each fitting mode reuses its geometry");
    }
}
