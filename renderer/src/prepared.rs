//! Prepared-loop generation and offline flash approval. No codec or GPU device access.
use super::*;
use std::io::{IsTerminal, Read, Write};
use std::time::Instant;

fn ineligible(pack: &Pack) -> Option<&'static str> {
    if pack.uses_photos || pack.uses_videos {
        Some("photos and videos use live playback")
    } else if pack.cache_clock {
        Some("this screensaver reads the live clock")
    } else {
        None
    }
}

pub fn plan(args: &Args) -> Result<(), String> {
    args.only(&["pack", "theme", "settings", "seed", "branding", "logo", "badge", "photos", "videos"])?;
    let dir = args.path("pack").ok_or("cache-plan needs --pack")?;
    confine_reading(args, std::slice::from_ref(&dir))?;
    let pack = load_with(&dir, args, &settings(args)?, Some(1))?;
    sandbox::confine()?;
    let fingerprint: String = pack.cache_digest.iter().map(|b| format!("{b:02x}")).collect();
    let reason = ineligible(&pack);
    say!(
        "{{\"eligible\":{},\"reason\":{},\"fingerprint\":{},\"seconds\":{},\"fps\":24}}",
        reason.is_none(),
        json(reason.unwrap_or("")),
        json(&fingerprint),
        pack.length().clamp(16.0, 32.0)
    );
    Ok(())
}

fn dimensions(args: &Args) -> Result<(u32, u32), String> {
    let size = parse_size(args.get("size"), (1152, 720))?;
    if size.0 == 0 || size.1 == 0 || size.0 > 4096 || size.1 > 2160 || size.0 as u64 * size.1 as u64 > 4096 * 2160 {
        return Err("prepared canvas exceeds 4096x2160".into());
    }
    Ok(size)
}

fn cadence(args: &Args) -> Result<(f64, u32), String> {
    let fps = args.number("fps")?.unwrap_or(24.0);
    let seconds = args.number("seconds")?.unwrap_or(32.0);
    if !(1.0..=24.0).contains(&fps) || !(0.05..=120.0).contains(&seconds) {
        return Err("prepared fps must be 1..=24 and seconds 0.05..=120".into());
    }
    Ok((fps, (fps * seconds).ceil() as u32))
}

pub fn record(args: &Args) -> Result<(), String> {
    args.only(&["pack", "theme", "branding", "logo", "badge", "photos", "videos", "settings", "seed", "start", "size", "fps", "seconds"])?;
    if std::io::stdout().is_terminal() {
        return Err("RGBA output needs a pipe".into());
    }
    let dir = args.path("pack").ok_or("prepare-frames needs --pack")?;
    let (w, h) = dimensions(args)?;
    let (fps, total) = cadence(args)?;
    let start = args.number("start")?.unwrap_or(0.0);
    let seed = args.number("seed")?.unwrap_or(1.0) as u64;
    confine_reading(args, std::slice::from_ref(&dir))?;
    let pack = load_with(&dir, args, &settings(args)?, Some(seed))?;
    if let Some(reason) = ineligible(&pack) {
        return Err(reason.into());
    }
    let confinement = sandbox::confine()?;
    let mut raster = Raster::new();
    raster.largest = (w, h);
    raster.background_decoding = false;
    raster.load_fonts(&pack);
    let view = View::new(w, h, pack.stage);
    let mut eval = eval::Evaluator::new(&pack, seed, raster.metrics());
    eval.set_clock(clock::Clock::fixed());
    eval.set_pixel_scale(view.scale);
    let mut pixels = tiny_skia::Pixmap::new(w, h).ok_or("prepared playback pixmap allocation failed")?;
    let mut admitted = vec![0; w as usize * h as usize * 4];
    for pixel in admitted.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    let mut guard = flash::Guard::new();
    let mut rejects = 0;
    let began = Instant::now();
    let mut out = std::io::BufWriter::with_capacity(1024 * 1024, std::io::stdout().lock());
    for i in 0..total {
        let now = start + i as f64 / fps;
        let frame = eval.frame(&pack, now, view.aspect());
        wayland::DRAWS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        raster.draw(&pack, &frame, &mut pixels.as_mut(), view);
        wayland::DRAWS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let cells = flash::measure(pixels.data(), w as usize, h as usize, w as usize * 4, false);
        if guard.admit(&cells, now) {
            admitted.copy_from_slice(pixels.data());
        } else {
            rejects += 1;
        }
        out.write_all(&admitted).map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())?;
    let source_digest: String = pack.cache_digest.iter().map(|b| format!("{b:02x}")).collect();
    tell!(
        "{{\"rnd\":\"record\",\"frames\":{total},\"fps\":{fps},\"width\":{w},\"height\":{h},\"rejects\":{rejects},\"seconds\":{},\"sourceDigest\":\"{source_digest}\",\"confinement\":{}}}",
        began.elapsed().as_secs_f64(),
        json(confinement)
    );
    Ok(())
}

/// Offline-only approval of one captured GPU-rendered cycle. Playback is
/// intentionally over-tested at 64 distinct frames/second, above the permitted
/// display refresh. Holds are allowed; dropped/reordered/modified frames are
/// outside the experimental profile. No failing frame is silently filtered.
pub fn certify(args: &Args) -> Result<(), String> {
    args.only(&["size", "frames"])?;
    let (w, h) = dimensions(args)?;
    let total: usize = args.get("frames").ok_or("approve-frames needs --frames")?.parse().map_err(|_| "frames must be an integer")?;
    // Cells, rather than full frames, are retained: at most about 40 MiB.
    if !(1..=3600).contains(&total) {
        return Err("frames must be 1..=3600".into());
    }
    sandbox::confine()?;
    let mut input = std::io::stdin().lock();
    let mut pixels = vec![0; w as usize * h as usize * 4];
    let mut cycle = Vec::with_capacity(total);
    let mut original = blake3::Hasher::new();
    for i in 0..total {
        input.read_exact(&mut pixels).map_err(|e| format!("frame {i}: {e}"))?;
        if pixels.chunks_exact(4).any(|p| p[3] != 255) {
            return Err("approval requires opaque final pixels".into());
        }
        original.update(&pixels);
        cycle.push(flash::measure(&pixels, w as usize, h as usize, w as usize * 4, false));
    }
    // Verify the decoder's actual second loop, then establish the flash
    // guard's periodic state over the measured cycle. A fast portable hash
    // compares every output byte; media/authentication identities use SHA-256.
    let mut repeated = blake3::Hasher::new();
    for i in 0..total {
        input.read_exact(&mut pixels).map_err(|e| format!("repeat frame {i}: {e}"))?;
        repeated.update(&pixels);
    }
    let pixel_hash = original.finalize();
    if pixel_hash != repeated.finalize() {
        return Err("decoded loop changed on repeat".into());
    }
    let pixels_hex = pixel_hash.to_hex();
    let mut extra = [0];
    if input.read(&mut extra).map_err(|e| e.to_string())? != 0 {
        return Err("unexpected trailing frame data".into());
    }
    let loops = certify_cells(&cycle)?;
    say!("{{\"approved\":true,\"frames\":{total},\"tested_rate\":64,\"periodic_after_cycles\":{loops},\"rejects\":0,\"pixelsBlake3\":\"{pixels_hex}\"}}");
    Ok(())
}

fn certify_cells(cycle: &[Vec<flash::Cell>]) -> Result<usize, String> {
    let black = vec![flash::Cell::default(); flash::GRID_W * flash::GRID_H];
    let mut guard = flash::Guard::new();
    let mut tick = 0u64;
    // A new playback starts from black; all other transitions require their own
    // approval. Check ending in black at every possible stop point as well.
    assert!(guard.admit(&black, 0.0));
    let mut seen = Vec::new();
    let warmup_cycles = 128usize.div_ceil(cycle.len());
    for round in 1..=warmup_cycles + 8 {
        for (index, cells) in cycle.iter().enumerate() {
            tick += 1;
            if !guard.admit(cells, tick as f64 / 64.0) {
                return Err(format!("flash policy rejected cycle {round} frame {index}"));
            }
            if guard.clone().assess(&black, (tick + 1) as f64 / 64.0).over_limit {
                return Err(format!("flash policy rejected black exit at cycle {round} frame {index}"));
            }
        }
        if tick >= 128 {
            let state = guard.periodic_state(tick);
            if seen.contains(&state) {
                return Ok(round);
            }
            seen.push(state);
        }
    }
    Err("no periodic guard invariant within eight cycles".into())
}

pub const MAGIC: u32 = 0x31435053;
pub const XRGB: u32 = 0x34325258;

pub fn buffer_layout(words: &[u32], fd: &std::os::fd::OwnedFd, slot: usize, size: (u32, u32)) -> Result<u32, String> {
    if words.len() != 8
        || words[0] != MAGIC
        || words[1] != 1
        || words[2] != slot as u32
        || (words[3], words[4]) != size
        || words[5] < size.0 * 4
        || words[5] > 65536
        || !words[5].is_multiple_of(4)
        || words[6] != XRGB
        || words[7] != 0
    {
        return Err("invalid prepared buffer layout".into());
    }
    // Linux DMA_BUF_MAGIC. Validate the descriptor class before giving it to
    // the compositor: file size alone would also accept a file or memfd.
    let fs = rustix::fs::fstatfs(fd).map_err(|e| format!("can't identify GPU buffer: {e}"))?;
    if fs.f_type as u64 != 0x444d4142 {
        return Err("prepared descriptor is not a DMA-BUF".into());
    }
    let meta = rustix::fs::fstat(fd).map_err(|e| format!("can't inspect GPU buffer: {e}"))?;
    let required = words[5] as u64 * size.1 as u64;
    if meta.st_size < 0 || (meta.st_size as u64) < required || meta.st_size as u64 > 128 << 20 {
        return Err("unbounded prepared buffer".into());
    }
    Ok(words[5])
}

pub struct Packet {
    pub words: Vec<u32>,
    pub fds: Vec<std::os::fd::OwnedFd>,
}

/// Fixed-sized private SOCK_SEQPACKET channel, never a path or a pack stream.
/// Received descriptors are owned immediately, including on every failure path.
pub fn receive() -> Result<Option<Packet>, String> {
    use rustix::net::{recvmsg, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags};
    use std::mem::MaybeUninit;
    let mut data = [0u8; 32];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let result = match recvmsg(std::io::stdin(), &mut [std::io::IoSliceMut::new(&mut data)], &mut control, RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC) {
        Ok(value) => value,
        Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => return Ok(None),
        Err(e) => return Err(format!("prepared channel failed: {e}")),
    };
    let mut fds = Vec::new();
    for item in control.drain() {
        match item {
            RecvAncillaryMessage::ScmRights(rights) => fds.extend(rights),
            _ => return Err("unexpected prepared channel metadata".into()),
        }
    }
    if result.flags.intersects(ReturnFlags::TRUNC | ReturnFlags::CTRUNC) || ![16, 32].contains(&result.bytes) {
        return Err("invalid prepared packet length".into());
    }
    let words: Vec<u32> = data[..result.bytes].chunks_exact(4).map(|v| u32::from_le_bytes(v.try_into().unwrap())).collect();
    if words[0] != MAGIC {
        return Err("invalid prepared packet".into());
    }
    Ok(Some(Packet { words, fds }))
}

pub fn request(frame: u32, slot: usize) -> Result<(), String> {
    use rustix::net::{send, SendFlags};
    let data: Vec<u8> = [MAGIC, 2, frame, slot as u32].iter().flat_map(|v| v.to_le_bytes()).collect();
    let count = send(std::io::stdin(), &data, SendFlags::DONTWAIT | SendFlags::NOSIGNAL).map_err(|e| format!("can't request prepared frame: {e}"))?;
    if count != data.len() {
        return Err("short prepared request".into());
    }
    Ok(())
}

pub fn run(args: &Args) -> Result<(), String> {
    args.only(&["size", "frames", "fps", "role", "exit-after"])?;
    let size = dimensions(args)?;
    let frames: u32 = args.get("frames").ok_or("missing prepared frame count")?.parse().map_err(|_| "invalid frame count")?;
    let fps: u32 = args.get("fps").ok_or("missing prepared cadence")?.parse().map_err(|_| "invalid cadence")?;
    if !(1..=3600).contains(&frames) || !(1..=24).contains(&fps) {
        return Err("prepared limits exceeded".into());
    }
    sandbox::confine_reading(&[])?;
    let display = wayland::connect()?;
    sandbox::confine()?;
    let limit = args.number("exit-after")?.map(|s| Duration::from_secs_f64(s.clamp(0.01, 86400.0)));
    let why = display.prepared(size, frames, fps, limit)?;
    say!("{why}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cells(luminance: f32, red: f32) -> Vec<flash::Cell> {
        vec![flash::Cell { luminance, red }; flash::GRID_W * flash::GRID_H]
    }
    #[test]
    fn approval_rejects_repeat_seam_and_saturated_red() {
        let seam: Vec<_> = (0..6).map(|i| cells((i % 2) as f32, 0.0)).collect();
        assert!(certify_cells(&seam).unwrap_err().contains("cycle 2"));
        let red: Vec<_> = (0..24).map(|i| cells(0.04, (i % 2) as f32 * 80.0)).collect();
        assert!(certify_cells(&red).is_err());
        assert!(certify_cells(&[cells(0.4, 0.0)]).is_ok());
    }

    #[test]
    fn ordinary_memfd_cannot_be_imported_as_gpu_buffer() {
        let fd = rustix::fs::memfd_create("not-a-gpu-buffer", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
        rustix::fs::ftruncate(&fd, 64 * 64 * 4).unwrap();
        let words = [MAGIC, 1, 0, 64, 64, 256, XRGB, 0];
        assert!(buffer_layout(&words, &fd, 0, (64, 64)).unwrap_err().contains("not a DMA-BUF"));
    }

    #[test]
    fn compiled_clock_dependencies_and_implicit_clock_text_stay_live() {
        for (name, extra, draw) in [
            ("direct", "", "kind='rect'\nx='second'"),
            ("binding", "let=['a=second','b=a*2']\n", "kind='rect'\nx='b'"),
            ("predicate", "", "kind='rect'\nvisible='minute>0'"),
            ("clock-text", "", "kind='text'\ntext='{zone_name}'"),
        ] {
            let manifest = format!("format=2\nname='Test'\n[[scene]]\nduration=5\n{extra}[[scene.draw]]\n{draw}\n");
            let dir = pack::tests::temp_pack(&format!("cache-{name}"), &manifest, &[]);
            let p = pack::load(&dir, &Environment::default()).unwrap();
            std::fs::remove_dir_all(dir).unwrap();
            assert!(p.cache_clock, "{name}");
            assert!(ineligible(&p).is_some(), "{name}");
        }
    }

    #[test]
    fn prepared_source_identity_covers_resolved_settings_and_asset_bytes() {
        use image::ImageEncoder;
        let manifest = "format=2\nname='Test'\n[[setting]]\nname='speed'\nlabel='Speed'\nkind='choice'\nchoices=['Slow','Fast']\nvalues=[1,2]\ndefault=1\n[[scene]]\nduration=5\n[[scene.draw]]\nkind='image'\nfile='image.png'\nx='speed*t'\n";
        let png = |v| {
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes).write_image(&[v; 12], 2, 2, image::ExtendedColorType::Rgb8).unwrap();
            bytes
        };
        let dir = pack::tests::temp_pack("cache-identity", manifest, &[("image.png", &png(17))]);
        let original = pack::load(&dir, &Environment { cache_identity: true, ..Environment::default() }).unwrap();
        let changed = pack::load(&dir, &Environment { cache_identity: true, settings: &[("speed".into(), 2.0)], ..Environment::default() }).unwrap();
        assert_ne!(original.cache_digest, changed.cache_digest);
        std::fs::write(dir.join("image.png"), png(23)).unwrap();
        let asset = pack::load(&dir, &Environment { cache_identity: true, ..Environment::default() }).unwrap();
        assert_ne!(original.cache_digest, asset.cache_digest);
        assert!(ineligible(&original).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
