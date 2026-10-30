//! Experimental live capture. The existing stdout pipe is raw media IPC only;
//! neither filesystem authority nor a display connection reaches the encoder.
use std::fs::File;
use std::os::fd::AsFd;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::{Duration, Instant};

type Latest = Arc<(Mutex<Option<Arc<Vec<u8>>>>, Condvar)>;

/// Retain the dedicated pipe before any pack is read, then detach fd 1.
/// Dropping the retained writer produces EOF while live playback continues.
pub fn output() -> Result<File, String> {
    let out = std::io::stdout();
    let info = rustix::fs::fstat(&out).map_err(|e| e.to_string())?;
    if rustix::fs::FileType::from_raw_mode(info.st_mode) != rustix::fs::FileType::Fifo {
        return Err("capture requires a dedicated stdout pipe".into());
    }
    let retained = out.as_fd().try_clone_to_owned().map_err(|e| e.to_string())?;
    let null = File::options().write(true).open("/dev/null").map_err(|e| e.to_string())?;
    rustix::stdio::dup2_stdout(&null).map_err(|e| e.to_string())?;
    Ok(File::from(retained))
}

pub struct Capture {
    size: (u32, u32),
    pool: Vec<Arc<Vec<u8>>>,
    latest: Latest,
    previous: Option<Instant>,
    window: Option<Instant>,
    updates: u32,
    stop: Arc<AtomicBool>,
    origin: Option<Instant>,
    next: u32,
}

fn write(out: &impl std::os::fd::AsFd, mut bytes: &[u8], stop: &AtomicBool, deadline: Instant) -> Result<(), String> {
    while !bytes.is_empty() {
        if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
            return Err("capture cancelled or encoder stalled".into());
        }
        match rustix::io::write(out, bytes) {
            Ok(0) => return Err("capture pipe made no progress".into()),
            Ok(n) => bytes = &bytes[n..],
            Err(rustix::io::Errno::INTR) => {}
            Err(rustix::io::Errno::AGAIN) => {
                let mut poll = [rustix::event::PollFd::new(out, rustix::event::PollFlags::OUT)];
                let wait = rustix::event::Timespec { tv_sec: 0, tv_nsec: 10_000_000 };
                rustix::event::poll(&mut poll, Some(&wait)).map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

impl Capture {
    pub fn new(size: (u32, u32), frames: u32, digest: [u8; 32], seed: u64, out: File) -> Result<Self, String> {
        let (w, h) = size;
        if !(16..=4096).contains(&w) || !(9..=2160).contains(&h) || !(384..=768).contains(&frames) {
            return Err("live capture exceeds its bounds".into());
        }
        let info = rustix::fs::fstat(&out).map_err(|e| e.to_string())?;
        if rustix::fs::FileType::from_raw_mode(info.st_mode) != rustix::fs::FileType::Fifo {
            return Err("live capture needs a dedicated stdout pipe".into());
        }
        let flags = rustix::fs::fcntl_getfl(&out).map_err(|e| e.to_string())?;
        rustix::fs::fcntl_setfl(&out, flags | rustix::fs::OFlags::NONBLOCK).map_err(|e| e.to_string())?;
        let mut pool = Vec::new();
        for _ in 0..3 {
            let mut pixels = Vec::new();
            pixels.try_reserve_exact(w as usize * h as usize * 4).map_err(|_| "capture allocation failed")?;
            pixels.resize(w as usize * h as usize * 4, 0);
            pool.push(Arc::new(pixels));
        }
        let latest = Arc::new((Mutex::new(None::<Arc<Vec<u8>>>), Condvar::new()));
        let handoff = latest.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let digest: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        let header = format!("{{\"capture\":1,\"sourceDigest\":\"{digest}\",\"seed\":\"{seed}\",\"width\":{w},\"height\":{h},\"frames\":{frames},\"fps\":24,\"sampling\":\"latest-admitted\"}}\n");
        std::thread::Builder::new()
            .name("live-capture".into())
            .spawn(move || {
                // Created only after the renderer's final confinement layer: this
                // thread inherits Landlock and all-thread network restrictions.
                let result = (|| -> Result<(), String> {
                    let (slot, changed) = &*handoff;
                    let (first, _) = changed
                        .wait_timeout_while(slot.lock().map_err(|e| e.to_string())?, Duration::from_secs(5), |v| {
                            v.is_none() && !stopped.load(Ordering::Relaxed)
                        })
                        .map_err(|e| e.to_string())?;
                    let mut pixels = first.as_ref().ok_or("no admitted frame")?.clone();
                    drop(first);
                    write(&out, header.as_bytes(), &stopped, Instant::now() + Duration::from_secs(1))?;
                    let mut origin = Instant::now();
                    for index in 0..frames {
                        let due = origin + Duration::from_secs_f64(index as f64 / 24.0);
                        if let Some(wait) = due.checked_duration_since(Instant::now()) {
                            std::thread::sleep(wait);
                        }
                        if index > 3 && Instant::now().saturating_duration_since(due) > Duration::from_millis(84) {
                            return Err("encoder cannot keep up with live capture".into());
                        }
                        if let Some(newer) = slot.lock().map_err(|e| e.to_string())?.as_ref() {
                            pixels = newer.clone();
                        }
                        let budget = if index <= 3 { Duration::from_secs(1) } else { Duration::from_millis(84) };
                        write(&out, &pixels, &stopped, Instant::now() + budget)?;
                        if index <= 3 {
                            origin = Instant::now() - Duration::from_secs_f64(index as f64 / 24.0);
                        }
                    }
                    tell!("{{\"captureComplete\":true,\"frames\":{frames}}}");
                    Ok(())
                })();
                stopped.store(true, Ordering::Relaxed);
                if let Err(e) = result {
                    tell!("screensafe: live capture abandoned: {e}");
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { size, pool, latest, stop, origin: None, previous: None, window: None, updates: 0, next: 0 })
    }

    /// Only called with the exact admitted RGBA pixels submitted to Wayland.
    /// Slow consumers abandon capture; this method never waits for the encoder.
    pub fn offer(&mut self, size: (u32, u32), pixels: &[u8], fps: f64) {
        if self.stop.load(Ordering::Relaxed) {
            self.pool.clear();
            return;
        }
        if size != self.size || pixels.len() != size.0 as usize * size.1 as usize * 4 {
            self.stop.store(true, Ordering::Relaxed);
            return;
        }
        let now = Instant::now();
        let window = *self.window.get_or_insert(now);
        self.updates += 1;
        let gap = self.previous.replace(now).map(|p| now.duration_since(p).as_secs_f64()).unwrap_or(0.0);
        let elapsed = now.duration_since(window).as_secs_f64();
        let budget = 1.0 / fps.clamp(1.0, 24.0);
        if gap > budget * 2.5 || (elapsed >= 2.0 && (self.updates as f64) < elapsed / budget * 0.9) {
            self.stop.store(true, Ordering::Relaxed);
            tell!("screensafe: live capture abandoned: live frame budget exceeded");
            return;
        }
        if elapsed >= 2.0 {
            self.window = Some(now);
            self.updates = 0;
        }
        let origin = *self.origin.get_or_insert(now);
        if now.duration_since(origin).as_secs_f64() * 24.0 < self.next as f64 {
            return;
        }
        self.next = (now.duration_since(origin).as_secs_f64() * 24.0).floor() as u32 + 1;
        let Some(buffer) = self.pool.iter_mut().find(|b| Arc::strong_count(b) == 1) else {
            self.stop.store(true, Ordering::Relaxed);
            return;
        };
        Arc::get_mut(buffer).unwrap().copy_from_slice(pixels);
        // A single replaceable latest frame, never a growing backlog.
        // Contention skips this update rather than stalling the display.
        if let Ok(mut slot) = self.latest.0.try_lock() {
            *slot = Some(buffer.clone());
            self.latest.1.notify_one();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.latest.1.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    #[test]
    fn capture_pipe_reaches_eof_while_producer_remains_alive() {
        if std::env::var_os("SCREENSAFE_CAPTURE_EOF_CHILD").is_some() {
            let out = output().unwrap();
            write(&out, b"capture-eof", &AtomicBool::new(false), Instant::now() + Duration::from_secs(1)).unwrap();
            drop(out);
            std::thread::sleep(Duration::from_secs(2));
            return;
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "live_capture::tests::capture_pipe_reaches_eof_while_producer_remains_alive", "--nocapture"])
            .env("SCREENSAFE_CAPTURE_EOF_CHILD", "1")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut bytes = Vec::new();
        child.stdout.take().unwrap().read_to_end(&mut bytes).unwrap();
        let alive = child.try_wait().unwrap().is_none();
        if alive {
            child.kill().unwrap();
        }
        child.wait().unwrap();
        assert!(alive, "capture EOF waited for the live producer to exit");
        assert!(bytes.ends_with(b"capture-eof"));
    }
    #[test]
    fn nonreading_encoder_cancels_without_blocking_rendering() {
        let (_reader, writer) = std::io::pipe().unwrap();
        let flags = rustix::fs::fcntl_getfl(&writer).unwrap();
        rustix::fs::fcntl_setfl(&writer, flags | rustix::fs::OFlags::NONBLOCK).unwrap();
        let start = Instant::now();
        assert!(write(&writer, &vec![0; 1 << 20], &AtomicBool::new(false), start + Duration::from_millis(40)).is_err());
        assert!(start.elapsed() < Duration::from_millis(250));
    }
    #[test]
    fn inherited_pipe_preserves_bytes_and_checks_cancellation() {
        let (mut reader, writer) = std::io::pipe().unwrap();
        let stop = AtomicBool::new(false);
        write(&writer, b"admitted pixels", &stop, Instant::now() + Duration::from_secs(1)).unwrap();
        let mut data = [0; 15];
        reader.read_exact(&mut data).unwrap();
        assert_eq!(&data, b"admitted pixels");
        stop.store(true, Ordering::Relaxed);
        assert!(write(&writer, b"x", &stop, Instant::now() + Duration::from_secs(1)).is_err());
    }
}
