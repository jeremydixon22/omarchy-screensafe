//! Prepared video: bounded, independently decoded JPEG frames behind a retained read-only descriptor.
//! General media containers are normalized by the isolated importer, never parsed by the shell or this reader.
use crate::safefs::Dir;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Arc;

pub const FOLDER: &str = "Videos/Screensaver";
pub const FILE_LIMIT: u64 = 512 << 20;
const MAX_FRAMES: usize = 7200;
pub const MAX_COMPRESSED_FRAMES: usize = u16::MAX as usize;
const MAX_FRAME_BYTES: u32 = 2 << 20;
const MAX_VIDEOS: usize = 32;
const HEADER: usize = 32;
const ENTRY: usize = 12;

/// A canvas showing video remains bounded; the compositor scales its already-guarded pixels to the display.
pub fn render_size(w: u32, h: u32) -> (u32, u32) {
    let scale = (1280.0 / w.max(1) as f64).min(720.0 / h.max(1) as f64).min(1.0);
    (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1))
}

#[derive(Clone)]
pub struct Video {
    pub name: String,
    file: Arc<File>,
    pub size: u64,
    pub modified: (i64, i64),
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    frames: Arc<[(u64, u32)]>,
    compressed: bool,
}

pub enum FrameBytes {
    Jpeg(Vec<u8>),
    Rgba(Vec<u8>),
}

impl Video {
    fn open(name: String, opened: crate::safefs::OpenFile) -> Result<Self, String> {
        let mut header = [0; HEADER];
        opened.file.read_exact_at(&mut header, 0).map_err(|e| e.to_string())?;
        let compressed = &header[..8] == b"SSVID002";
        if (!compressed && &header[..8] != b"SSVID001") || (!compressed && header[24..].iter().any(|b| *b != 0)) {
            return Err("not a ScreenSafe prepared video (version 1)".into());
        }
        let word = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        let (width, height, fps, count) = (word(8), word(12), word(16), word(20) as usize);
        let maximum = if compressed { MAX_COMPRESSED_FRAMES } else { MAX_FRAMES };
        if width == 0
            || width > 1280
            || height == 0
            || height > 720
            || fps == 0
            || fps > 24
            || count == 0
            || count > maximum
            || (!compressed && count > fps as usize * 300)
        {
            return Err("video dimensions, rate or duration exceed prototype limits".into());
        }
        let (header_size, stride) = if compressed {
            let mut extra = [0u8; 32];
            opened.file.read_exact_at(&mut extra, 32).map_err(|e| e.to_string())?;
            let extra_word = |at| u32::from_le_bytes(extra[at..at + 4].try_into().unwrap());
            if width % 2 != 0
                || height % 2 != 0
                || fps != 24
                || ![1, 2].contains(&word(24))
                || word(28) != 24
                || extra_word(0) != 1
                || extra[4..].iter().any(|b| *b != 0)
            {
                return Err("unsupported compressed video profile".into());
            }
            (64usize, 16usize)
        } else {
            (HEADER, ENTRY)
        };
        let mut index = vec![0; count * stride];
        opened.file.read_exact_at(&mut index, header_size as u64).map_err(|e| e.to_string())?;
        let mut frames = Vec::with_capacity(count);
        let mut end = 0u64;
        for (i, entry) in index.chunks_exact(stride).enumerate() {
            let offset = u64::from_le_bytes(entry[..8].try_into().unwrap());
            let length = u32::from_le_bytes(entry[8..12].try_into().unwrap());
            if length == 0
                || length > MAX_FRAME_BYTES
                || offset < (header_size + index.len()) as u64
                || (frames.is_empty() && offset > (header_size + maximum * stride) as u64)
                || (!frames.is_empty() && offset != end)
                || (compressed && frames.is_empty() && offset != (header_size + index.len()) as u64)
                || (compressed && u32::from_le_bytes(entry[12..16].try_into().unwrap()) != u32::from(i % 24 == 0))
            {
                return Err("invalid video frame index".into());
            }
            end = offset.checked_add(length as u64).filter(|n| *n <= opened.size).ok_or("video frame extends beyond its file")?;
            frames.push((offset, length));
        }
        if end != opened.size {
            return Err("video has unexpected trailing data".into());
        }
        Ok(Self { name, file: Arc::new(opened.file), size: opened.size, modified: opened.modified, width, height, fps, frames: frames.into(), compressed })
    }

    pub fn duration(&self) -> f64 {
        self.frames.len() as f64 / self.fps as f64
    }

    /// Time within the selected clip. A short clip holds its final frame until the pack advances.
    pub fn frame_at(&self, seconds: f64) -> u32 {
        ((seconds.max(0.0) * self.fps as f64).floor() as usize).min(self.frames.len() - 1) as u32
    }

    pub fn frame_count(&self) -> u32 {
        self.frames.len() as u32
    }

    /// Adjacent film cells share a bounded six-frame cache. Offsets are at most four frames either way.
    /// Clamp the base position first so a short clip still holds; wrapping applies only to its neighbors.
    pub fn frame_offset(&self, seconds: f64, offset: f64, wrap: bool) -> u32 {
        let offset = if offset.is_finite() { offset.round().clamp(-4.0, 4.0) } else { 0.0 };
        let frame = self.frame_at(seconds) as f64 + offset;
        if wrap {
            frame.rem_euclid(self.frames.len() as f64) as u32
        } else {
            frame.clamp(0.0, (self.frames.len() - 1) as f64) as u32
        }
    }

    pub fn read_frame(&self, frame: u32) -> Result<Vec<u8>, String> {
        if self.compressed {
            return Err("compressed video needs the isolated decoder".into());
        }
        let &(offset, length) = self.frames.get(frame as usize).ok_or("video frame out of range")?;
        let mut bytes = vec![0; length as usize];
        self.file.read_exact_at(&mut bytes, offset).map_err(|e| e.to_string())?;
        if !bytes.starts_with(&[255, 216]) || crate::raster::probe_image(&bytes)? != (self.width, self.height) {
            return Err("video frame format or dimensions changed".into());
        }
        Ok(bytes)
    }

    pub fn read_pixels(&self, frame: u32) -> Result<FrameBytes, String> {
        if frame as usize >= self.frames.len() {
            return Err("video frame out of range".into());
        }
        if self.compressed {
            let index = u16::try_from(frame).map_err(|_| "compressed frame exceeds 16-bit reference")?;
            crate::video_decoder::frame(&self.file, index, self.width, self.height).map(FrameBytes::Rgba)
        } else {
            self.read_frame(frame).map(FrameBytes::Jpeg)
        }
    }
}

pub fn open(folder: &Path, warnings: &mut Vec<String>) -> Vec<Video> {
    let Ok(dir) = Dir::open(folder) else { return Vec::new() };
    let Ok((names, _)) = dir.user_files(4096) else {
        warnings.push("can't list videos folder".into());
        return Vec::new();
    };
    let mut videos = Vec::new();
    for name in names.into_iter().filter(|n| n.to_ascii_lowercase().ends_with(".ssv")).take(MAX_VIDEOS) {
        let result = dir.open_user_file(&name, FILE_LIMIT).map_err(|e| e.0).and_then(|file| Video::open(name.clone(), file));
        match result {
            Ok(video) if video.compressed && !crate::video_decoder::available() => {
                warnings.push(format!("{name}: SSVv2 needs the explicit prototype decoder wrapper"));
            }
            Ok(video) => videos.push(video),
            Err(e) => warnings.push(format!("{name}: {e}")),
        }
    }
    videos
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    fn compressed_archive(count: usize) -> Vec<u8> {
        let mut bytes = b"SSVID002".to_vec();
        for word in [2u32, 2, 24, count as u32, 2, 24, 1, 0, 0, 0, 0, 0, 0, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let start = 64 + count * 16;
        for i in 0..count {
            bytes.extend_from_slice(&((start + i) as u64).to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&u32::from(i % 24 == 0).to_le_bytes());
        }
        bytes.resize(start + count, 0);
        bytes
    }

    #[test]
    fn compressed_video_keeps_16_bit_references_and_exact_index_bounds() {
        let video = load(&compressed_archive(MAX_COMPRESSED_FRAMES), "compressed-video-max").unwrap();
        assert_eq!(video.frame_count(), 65535);
        assert_eq!(video.frame_at(f64::MAX), 65534);
        assert_eq!(video.frame_offset(f64::MAX, 4.0, true), 3);
        assert!(load(&compressed_archive(MAX_COMPRESSED_FRAMES + 1), "compressed-video-over").is_err());
        let valid = compressed_archive(25);
        for (at, value) in [(24usize, 3u32), (28, 0), (32, 0), (36, 1), (40, 1), (64, 0), (64 + 12, 0), (64 + 24 * 16 + 12, 0)] {
            let mut bad = valid.clone();
            bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(load(&bad, "compressed-video-bad").is_err(), "byte {at}");
        }
        let mut trailing = valid;
        trailing.push(0);
        assert!(load(&trailing, "compressed-video-trailing").is_err());
    }
    pub(crate) fn archive_colors(colors: &[[u8; 3]]) -> Vec<u8> {
        archive_sized(colors, 3, 2)
    }

    fn archive_sized(colors: &[[u8; 3]], width: u32, height: u32) -> Vec<u8> {
        let frames: Vec<Vec<u8>> = colors
            .iter()
            .map(|color| {
                let mut jpeg = Vec::new();
                let pixels: Vec<u8> = (0..width * height).flat_map(|_| *color).collect();
                image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode(&pixels, width, height, image::ExtendedColorType::Rgb8).unwrap();
                jpeg
            })
            .collect();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"SSVID001");
        for value in [width, height, 24, colors.len() as u32, 0, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut offset = (HEADER + ENTRY * colors.len()) as u64;
        for frame in &frames {
            bytes.extend_from_slice(&offset.to_le_bytes());
            bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            offset += frame.len() as u64;
        }
        for frame in frames {
            bytes.extend_from_slice(&frame);
        }
        bytes
    }
    fn archive() -> Vec<u8> {
        archive_colors(&[[20; 3]; 2])
    }
    fn load(bytes: &[u8], name: &str) -> Result<Video, String> {
        let dir = crate::pack::tests::temp_pack(name, "", &[("clip.ssv", bytes)]);
        let opened = Dir::open(&dir).unwrap().open_user_file("clip.ssv", FILE_LIMIT).unwrap();
        let result = Video::open("clip.ssv".into(), opened);
        std::fs::remove_dir_all(dir).unwrap();
        result
    }
    #[test]
    fn video_canvas_keeps_aspect_and_bounds_without_upscaling() {
        assert_eq!(render_size(2304, 1440), (1152, 720));
        assert_eq!(render_size(1920, 1080), (1280, 720));
        assert_eq!(render_size(720, 1280), (405, 720));
        assert_eq!(render_size(640, 400), (640, 400));
    }

    #[test]
    fn prepared_video_keeps_descriptor_and_bounds_time() {
        let video = load(&archive(), "prepared-video-good").unwrap();
        assert_eq!(video.frame_at(-1.0), 0);
        assert_eq!(video.frame_at(0.05), 1);
        assert_eq!(video.frame_at(1e20), 1);
        assert_eq!(video.frame_at(f64::NAN), 0);
        assert_eq!(video.duration(), 2.0 / 24.0);
        assert!(video.read_frame(0).is_ok());
        assert!(video.read_frame(1).is_ok());
        assert!(video.read_frame(2).is_err());
    }

    #[test]
    fn adjacent_frames_wrap_only_when_requested_and_offsets_stay_bounded() {
        let video = load(&archive_colors(&[[20; 3]; 24]), "video-frame-offset").unwrap();
        assert_eq!(video.frame_offset(0.0, -1.0, true), 23);
        assert_eq!(video.frame_offset(0.0, -1.0, false), 0);
        assert_eq!(video.frame_offset(0.99, 2.0, true), 1);
        assert_eq!(video.frame_offset(0.99, 2.0, false), 23);
        assert_eq!(video.frame_offset(0.5, 1000.0, false), 16);
        assert_eq!(video.frame_offset(0.5, f64::NAN, false), 12);
        for offset in [-1e20, -4.0, 0.0, 4.0, 1e20, f64::NAN, f64::INFINITY] {
            assert!(video.frame_offset(1e20, offset, true) < video.frame_count());
        }
    }
    fn builtin_pack(name: &str, clips: usize, style: f64, looping: f64) -> crate::pack::Pack {
        let colors: Vec<_> = (0..24).map(|i| if i < 12 { [220, 20, 20] } else { [20, 20, 220] }).collect();
        let archive = archive_colors(&colors);
        let dir = crate::pack::tests::temp_pack(name, "", &[("a.ssv", &archive)]);
        if clips == 2 {
            std::fs::write(dir.join("b.ssv"), &archive).unwrap();
        }
        let settings = [("style".into(), style), ("loop".into(), looping), ("order".into(), 1.0)];
        let pack = crate::pack::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../packs/videos"),
            &crate::pack::Environment { videos: Some(&dir), settings: &settings, ..Default::default() },
        )
        .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        pack
    }

    #[test]
    fn flatscreen_has_an_unlit_first_frame_and_distinct_logo_input_and_playback_stages() {
        use crate::eval::{Evaluator, ImageSrc, Item};
        use crate::raster::{Raster, View};
        let pack = builtin_pack("flat-boot-stages", 1, 4.0, 1.0);
        let mut raster = Raster::new();
        raster.largest = (800, 500);
        let view = View::new(800, 500, pack.stage);
        let mut eval = Evaluator::new(&pack, 1, raster.metrics());
        let mut target = tiny_skia::Pixmap::new(800, 500).unwrap();
        let mut led = Vec::new();
        for time in [0.0, 1.15, 2.5, 5.5, 6.1, 6.5, 7.3, 8.3, 20.0] {
            let frame = eval.frame(&pack, time, view.aspect());
            let text: String = frame
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Glyphs { glyphs, .. } => Some(glyphs.iter().map(|g| g.ch).collect::<String>()),
                    _ => None,
                })
                .collect();
            if [2.5, 5.5].contains(&time) {
                assert!(text.contains("DISPLAYSYSTEMS"));
            }
            if [6.5, 7.3].contains(&time) {
                assert!(text.contains("COMP1"));
            }
            if time == 6.1 || time >= 8.3 {
                assert!(text.is_empty(), "boot/OSD should be gone at {time}: {text}");
            }
            assert!(!(text.contains("DISPLAYSYSTEMS") && text.contains("COMP1")));
            let source = frame
                .items
                .iter()
                .find_map(|i| match i {
                    Item::Image { source, .. } if matches!(source.src, ImageSrc::Video { .. }) => Some(source.src),
                    _ => None,
                })
                .unwrap();
            if time < 6.8 {
                assert_eq!(source, ImageSrc::Video { video: 0, frame: 0 }, "opening frame is held for the whole boot");
            }
            raster.draw(&pack, &frame, &mut target.as_mut(), view);
            let (dx, dy) = crate::overlay::drift(time);
            // Away from the centered logo and corner label, the glass stays black until playback.
            let x = (400.0 + (-0.5 * 0.95 + dx / 720.0) * 500.0).round() as u32;
            let y = (250.0 + (-0.060 * 0.95 + dy / 720.0) * 500.0).round() as u32;
            let pixel = target.pixel(x, y).unwrap();
            if time < 6.8 {
                assert!(pixel.red() < 5 && pixel.green() < 5 && pixel.blue() < 5, "video leaked before startup at {time}");
            }
            if time == 7.3 {
                assert!(pixel.blue() > pixel.red() + 50, "playback advances once boot is complete");
            }
            if time == 6.1 {
                // The logo has fully disappeared, and even the input label has not begun yet.
                assert!(target.pixel(400, 220).unwrap().red() < 5);
                assert!(target.pixel(400, 265).unwrap().red() < 5);
            }
            if time <= 1.15 {
                let x = (400.0 + (0.674 * 0.95 + dx / 720.0) * 500.0).round() as u32;
                let y = (250.0 + (0.3035 * 0.95 + dy / 720.0) * 500.0).round() as u32;
                led.push(target.pixel(x, y).unwrap());
            }
        }
        assert!(led[0].blue().abs_diff(led[0].red()) < 15, "initial LED must be neutral/unlit");
        assert!(led[1].blue() > led[1].red() + 50, "the power indicator lights before the logo");
        assert!(raster.warnings.is_empty(), "{:?}", raster.warnings);
    }

    #[test]
    fn flatscreen_input_uses_checked_720_line_resolution_and_an_exact_32_clip_mask() {
        use crate::eval::{Evaluator, Item};
        let high = archive_sized(&[[20; 3]], 960, 720);
        let low = archive_sized(&[[20; 3]], 640, 360);
        let dir = crate::pack::tests::temp_pack("flat-hd-metadata", "", &[]);
        let settings = [("style".into(), 4.0), ("order".into(), 1.0)];
        for (name, archive) in [("a.ssv", &high), ("b.ssv", &low)] {
            std::fs::write(dir.join(name), archive).unwrap();
        }
        let load = || {
            crate::pack::load(
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("../packs/videos"),
                &crate::pack::Environment { videos: Some(&dir), settings: &settings, ..Default::default() },
            )
            .unwrap()
        };
        let text = |pack: &crate::pack::Pack| {
            Evaluator::new(pack, 1, crate::raster::Raster::new().metrics())
                .frame(pack, 6.5, 1.6)
                .items
                .into_iter()
                .filter_map(|item| match item {
                    Item::Glyphs { glyphs, .. } => Some(glyphs.into_iter().map(|g| g.ch).collect::<String>()),
                    _ => None,
                })
                .collect::<String>()
        };
        let pack = load();
        assert_eq!(pack.initial[pack.builtins.video_hd_mask as usize], 1.0);
        assert_eq!(text(&pack), "HDMI1");
        std::fs::write(dir.join("a.ssv"), &low).unwrap();
        assert_eq!(text(&load()), "COMP1");
        for i in 0..32 {
            std::fs::write(dir.join(format!("{i:02}.ssv")), if i == 0 { &low } else { &high }).unwrap();
        }
        std::fs::remove_file(dir.join("a.ssv")).unwrap();
        std::fs::remove_file(dir.join("b.ssv")).unwrap();
        let pack = load();
        assert_eq!(pack.videos.len(), 32);
        assert_eq!(pack.initial[pack.builtins.video_hd_mask as usize], (u32::MAX - 1) as f64);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn builtin_video_loop_restarts_playlist_and_off_holds_final_frame() {
        use crate::eval::{Evaluator, ImageSrc, Item};
        for clips in [1, 2] {
            for looping in [0.0, 1.0] {
                for style in 0..6 {
                    let pack = builtin_pack(&format!("video-loop-{clips}-{looping}-{style}"), clips, style as f64, looping);
                    let mut eval = Evaluator::new(&pack, 1, crate::raster::Raster::new().metrics());
                    for playback in [0.25, 0.75, 1.25, 1.75, 2.25, 7.75] {
                        let time = playback
                            + if matches!(style, 3 | 5) {
                                0.625
                            } else if style == 4 {
                                6.8
                            } else {
                                0.0
                            };
                        let items = eval.frame(&pack, time, 1.6).items;
                        let videos: Vec<_> = items
                            .iter()
                            .filter_map(|item| match item {
                                Item::Image { source, .. } if matches!(source.src, ImageSrc::Video { .. }) => Some(source.src),
                                _ => None,
                            })
                            .collect();
                        assert_eq!(videos.len(), if style == 2 { 4 } else { 1 }, "only intended playback cells: {clips}/{looping}/{style}/{time}");
                        let held = looping == 0.0 && playback >= clips as f64;
                        let slot = if held { clips - 1 } else { playback as usize % clips };
                        let frame = if held { 23 } else { ((playback % 1.0) * 24.0) as u16 };
                        for (i, src) in videos.iter().enumerate() {
                            let offset = if style == 2 { i as i32 - 1 } else { 0 };
                            let frame = if looping > 0.0 { (frame as i32 + offset).rem_euclid(24) } else { (frame as i32 + offset).clamp(0, 23) } as u16;
                            assert_eq!(*src, ImageSrc::Video { video: slot as u16, frame });
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn loop_off_with_short_interval_holds_at_that_interval_not_the_clip_end() {
        use crate::eval::{Evaluator, ImageSrc, Item};
        let archive = archive_colors(&[[20; 3]; 240]);
        let dir = crate::pack::tests::temp_pack("video-loop-off-interval", include_str!("../../packs/videos/screensafe.toml"), &[("a.ssv", &archive)]);
        let settings = [("loop".into(), 0.0), ("each".into(), 5.0)];
        let pack = crate::pack::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../packs/videos"),
            &crate::pack::Environment { videos: Some(&dir), settings: &settings, ..Default::default() },
        )
        .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        let mut eval = Evaluator::new(&pack, 1, crate::raster::Raster::new().metrics());
        for (time, expected) in [(0.25, 6), (4.75, 114), (5.25, 120), (15.25, 120)] {
            let items = eval.frame(&pack, time, 1.6).items;
            let frames: Vec<_> = items
                .iter()
                .filter_map(|item| match item {
                    Item::Image { source, .. } if matches!(source.src, ImageSrc::Video { .. }) => Some(source.src),
                    _ => None,
                })
                .collect();
            assert_eq!(frames, vec![ImageSrc::Video { video: 0, frame: expected }]);
        }
    }

    #[test]
    fn builtin_video_pixels_advance_after_multiple_loops_with_async_decode() {
        use crate::eval::Evaluator;
        use crate::raster::{Raster, View};
        use std::time::{Duration, Instant};
        for style in 0..6 {
            let pack = builtin_pack(&format!("video-loop-pixels-{style}"), 1, style as f64, 1.0);
            let mut raster = Raster::new();
            raster.background_decoding = true;
            raster.largest = (192, 120);
            let mut eval = Evaluator::new(&pack, 1, raster.metrics());
            let view = View::new(192, 120, pack.stage);
            let mut pixels = tiny_skia::Pixmap::new(192, 120).unwrap();
            for playback in [0.25, 0.75, 1.25, 1.75, 2.25, 2.75] {
                let time = playback
                    + if matches!(style, 3 | 5) {
                        0.625
                    } else if style == 4 {
                        6.8
                    } else {
                        0.0
                    };
                let frame = eval.frame(&pack, time, view.aspect());
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    raster.poll_images();
                    raster.draw(&pack, &frame, &mut pixels.as_mut(), view);
                    let (x, y) = frame
                        .items
                        .iter()
                        .filter_map(|item| match item {
                            crate::eval::Item::Image { source, transform, .. } if matches!(source.src, crate::eval::ImageSrc::Video { .. }) => {
                                Some((transform.e, transform.f))
                            }
                            _ => None,
                        })
                        .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                        .unwrap();
                    let center = pixels.pixel((96.0 + x * view.scale).round() as u32, (60.0 + y * view.scale).round() as u32).unwrap();
                    let correct =
                        if playback % 1.0 < 0.5 { center.red() as u16 > center.blue() as u16 + 50 } else { center.blue() as u16 > center.red() as u16 + 50 };
                    if correct {
                        break;
                    }
                    assert!(Instant::now() < deadline, "video stopped advancing at style {style}, time {time}");
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            assert!(raster.warnings.is_empty(), "{:?}", raster.warnings);
        }
    }

    #[test]
    fn film_loop_off_holds_playback_and_weave_but_preserves_oled_drift() {
        use crate::eval::Item;
        let pack = builtin_pack("film-off-holds-all-cells", 1, 2.0, 0.0);
        let mut eval = crate::eval::Evaluator::new(&pack, 1, crate::raster::Raster::new().metrics());
        let early = eval.frame(&pack, 1.25, 1.6);
        let late = eval.frame(&pack, 20.25, 1.6);
        let images = |frame: crate::eval::Frame| {
            frame
                .items
                .into_iter()
                .filter_map(|i| match i {
                    Item::Image { source, alpha, transform, .. } => Some((source, alpha, transform)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let (a, b) = (images(early), images(late));
        assert_eq!(a.len(), b.len());
        let delta = (b[0].2.e - a[0].2.e, b[0].2.f - a[0].2.f);
        assert!(delta.0.abs() > 0.0001 && delta.1.abs() > 0.0001, "the held picture still shifts slowly for OLED protection");
        for (a, b) in a.iter().zip(&b) {
            assert_eq!((a.0, a.1), (b.0, b.1), "source frames and flicker hold");
            assert!((b.2.e - a.2.e - delta.0).abs() < 1e-10 && (b.2.f - a.2.f - delta.1).abs() < 1e-10, "only a shared whole-surround translation remains");
        }
    }

    #[test]
    fn film_future_frames_enter_at_top_and_drift_stays_narrow() {
        use crate::eval::{Evaluator, ImageSrc, Item};
        let pack = builtin_pack("film-order-drift", 1, 2.0, 1.0);
        let mut eval = Evaluator::new(&pack, 1, crate::raster::Raster::new().metrics());
        let mut xs = Vec::new();
        for n in 0..120 {
            let time = n as f64 / 2.0 + 0.25;
            let frame = eval.frame(&pack, time, 1.6);
            let mut cells: Vec<_> = frame
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Image { source, transform, .. } => match source.src {
                        ImageSrc::Video { frame, .. } => Some((transform.f, transform.e, frame)),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            assert_eq!(cells.len(), 4);
            assert!(cells.windows(2).all(|w| (w[1].2 + 24 - w[0].2) % 24 == 1), "decode requests keep their ascending order");
            cells.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!(cells.windows(2).all(|w| (w[0].2 + 24 - w[1].2) % 24 == 1), "later video frames belong above earlier frames on a descending strip");
            assert!(cells.windows(2).all(|w| (w[1].0 - w[0].0 - 0.34).abs() < 1e-6));
            assert!(cells.iter().all(|c| c.1.abs() <= 0.011001 + 10.0 / 720.0));
            xs.push(cells[0].1);
        }
        let low = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let high = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(high - low > 0.012, "the film actually drifts slowly across its narrow band");
    }

    #[test]
    fn surround_drift_tracks_output_pixels_and_preserves_unframed_video() {
        use crate::eval::{Evaluator, Item};
        for style in 0..6 {
            let pack = builtin_pack("oled-device-drift", 1, style as f64, 0.0);
            for height in [720.0, 1440.0, 2160.0] {
                let mut eval = Evaluator::new(&pack, 1, crate::raster::Raster::new().metrics());
                eval.set_pixel_scale(height);
                let time = 255.0;
                let frame = eval.frame(&pack, time, 1.6);
                let first = frame
                    .items
                    .iter()
                    .find_map(|i| match i {
                        Item::Image { transform, .. } => Some(transform),
                        _ => None,
                    })
                    .unwrap();
                let base_x = if style == 2 { 0.435 } else { 0.0 };
                let expected = if style >= 2 { crate::overlay::drift(time) } else { (0.0, 0.0) };
                assert!(((first.e - base_x) * height - expected.0).abs() < 1e-6);
                assert!((first.f * height - expected.1).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn crt_box_preserves_saved_value_and_fit_crops_in_the_native_map() {
        use crate::eval::{Evaluator, ImageSrc, Item};
        for (style, expected) in [(3.0, crate::pack::Fit::Contain), (5.0, crate::pack::Fit::Cover)] {
            let pack = builtin_pack("crt-fit-box", 1, style, 1.0);
            let mut eval = Evaluator::new(&pack, 1, crate::raster::Raster::new().metrics());
            let frame = eval.frame(&pack, 0.25, 1.6);
            let videos: Vec<_> =
                frame.items.iter().filter(|item| matches!(item,Item::Image { source, .. } if matches!(source.src,ImageSrc::Video { .. }))).collect();
            assert_eq!(videos.len(), 1);
            let Item::Image { source, fit, curve, .. } = videos[0] else { unreachable!() };
            assert_eq!(*fit, expected);
            assert_eq!(source.crop, 0, "reuse original decoded frames; crop only through the curved sampling map");
            assert_eq!(*curve, 0.045);
        }
    }

    #[test]
    fn crt_powers_on_once_and_tunes_only_when_the_clip_changes() {
        use crate::eval::{Evaluator, ImageSrc, Item};
        for clips in [1, 2] {
            let pack = builtin_pack("crt-signal-events", clips, 3.0, 1.0);
            let mut eval = Evaluator::new(&pack, 1, crate::raster::Raster::new().metrics());
            let picture = |eval: &mut Evaluator, time| {
                eval.frame(&pack, time, 1.6).items.into_iter().find_map(|item| match item {
                    Item::Image { source, alpha, .. } if matches!(source.src, ImageSrc::Video { .. }) => Some((source.src, alpha)),
                    _ => None,
                })
            };
            assert!(picture(&mut eval, 0.0).is_none(), "unlit glass before the center line");
            assert!(
                matches!(picture(&mut eval, 0.3), Some((ImageSrc::Video { frame: 0, .. }, a)) if a > 0.0 && a < 1.0),
                "opening shot waits while the tube powers on"
            );
            let (clip, alpha) = picture(&mut eval, 1.625 + 0.05).unwrap();
            assert_eq!(clip, ImageSrc::Video { video: (clips - 1) as u16, frame: 1 });
            assert_eq!(alpha < 1.0, clips == 2, "one clip looping has no channel-change dip");
            assert_eq!(picture(&mut eval, 1.625 + 0.4).unwrap().1, 1.0);
        }
    }

    #[test]
    fn device_startup_and_repeated_channel_changes_pass_the_unchanged_flash_guard() {
        use crate::eval::Evaluator;
        use crate::pack::{load, Environment};
        use crate::raster::{Raster, View};
        let archive = archive_colors(&[[255; 3]; 24]);
        let dir = crate::pack::tests::temp_pack("crt-signal-white-guard", "", &[("a.ssv", &archive), ("b.ssv", &archive)]);
        for style in [3.0, 4.0, 5.0] {
            let settings = [("style".into(), style), ("order".into(), 1.0)];
            let pack = load(
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("../packs/videos"),
                &Environment { videos: Some(&dir), settings: &settings, ..Default::default() },
            )
            .unwrap();
            let mut raster = Raster::new();
            raster.largest = (320, 200);
            let view = View::new(320, 200, pack.stage);
            let mut eval = Evaluator::new(&pack, 1, raster.metrics());
            let mut target = tiny_skia::Pixmap::new(320, 200).unwrap();
            let mut guard = crate::flash::Guard::new();
            for n in 0..264 {
                let time = n as f64 / 24.0;
                raster.draw(&pack, &eval.frame(&pack, time, view.aspect()), &mut target.as_mut(), view);
                let cells = crate::flash::measure(target.data(), 320, 200, 320 * 4, false);
                assert!(guard.admit(&cells, time), "bright footage and tuning effects exceeded the guard at style {style}, t={time}");
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prepared_video_rejects_unbounded_and_inconsistent_headers() {
        let original = archive();
        for (i, at, value) in [(0, 8, 1281), (1, 12, 721), (2, 16, 25), (3, 16, 0), (4, 20, 7201), (5, 20, 0), (6, 24, 1)] {
            let mut bytes = original.clone();
            bytes[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
            assert!(load(&bytes, &format!("prepared-header-{i}")).is_err());
        }
        for (i, at, value) in [(0, HEADER, 0u64), (1, HEADER, u64::MAX), (2, HEADER + ENTRY, 1)] {
            let mut bytes = original.clone();
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            assert!(load(&bytes, &format!("prepared-index-{i}")).is_err());
        }
        let mut bytes = original.clone();
        bytes.push(0);
        assert!(load(&bytes, "prepared-trailing").is_err());
        assert!(load(&original[..original.len() - 1], "prepared-truncated").is_err());
    }
    #[test]
    fn prepared_video_checks_actual_frame_dimensions() {
        let mut bytes = archive();
        bytes[8..12].copy_from_slice(&4u32.to_le_bytes());
        assert!(load(&bytes, "prepared-frame-size").unwrap().read_frame(0).is_err());
    }
}
