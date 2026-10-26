//! Showing the screensaver: one full-screen overlay surface per output, drawn in software, closed by any input. Or,
//! for the panel's live preview, one small surface at a given place on one output, which takes no input at all.

use std::os::fd::{AsFd, OwnedFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer::{self, WlBuffer},
    wl_callback::{self, WlCallback},
    wl_compositor::WlCompositor,
    wl_keyboard::{self, WlKeyboard},
    wl_output::{self, WlOutput},
    wl_pointer::{self, WlPointer},
    wl_region::WlRegion,
    wl_registry::{self, WlRegistry},
    wl_seat::{self, WlSeat},
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::{self, WlSurface},
    wl_touch::{self, WlTouch},
};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::{self, ZwpLinuxBufferParamsV1},
    zwp_linux_dmabuf_v1::{self, ZwpLinuxDmabufV1},
};
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

use crate::eval::{Evaluator, Frame, ImageSrc, Item};
use crate::flash;
use crate::pack::Pack;
use crate::raster::{Raster, View};

/// Pointer movement inside this window after opening doesn't close the screensaver, nor does a nudge smaller than
/// `MOVE_LIMIT` logical pixels afterwards.
const SETTLE: Duration = Duration::from_millis(750);
const MOVE_LIMIT: f64 = 24.0;
/// How far ahead, in seconds, images are decoded and resampled before they're shown.
const LOOK_AHEAD: f64 = 5.0;

/// Where a live preview goes: a rectangle in logical pixels from the top left of the output named `output` (or the
/// first, if none has that name).
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub output: Option<String>,
}

impl Region {
    /// "X,Y,W,H", each a whole number of logical pixels; at least 16 x 9 and at most 4096 a side.
    pub fn parse(text: &str, output: Option<String>) -> Result<Region, String> {
        let n: Vec<u32> = text
            .split([',', ' '])
            .filter(|p| !p.is_empty())
            .map(|p| p.parse::<u32>())
            .collect::<Result<_, _>>()
            .map_err(|_| "a region is X,Y,W,H in whole pixels")?;
        let [x, y, w, h] = n[..] else { return Err("a region is X,Y,W,H in whole pixels".into()) };
        if !(16..=4096).contains(&w) || !(9..=4096).contains(&h) || x > 16384 || y > 16384 {
            return Err("a region is at least 16 x 9 and at most 4096 a side".into());
        }
        Ok(Region { x, y, w, h, output })
    }
}

pub struct Options {
    pub seed: u64,
    pub exit_after: Option<Duration>,
    pub max_fps: f64,
    /// Where in the pack to begin, in seconds.
    pub start: f64,
    /// The wall clock packs read; the fixed one if none.
    pub clock: Option<crate::clock::Clock>,
    pub overlay: crate::overlay::Config,
}

struct Buffer {
    memfd: OwnedFd,
    wl: WlBuffer,
    busy: bool,
}

/// Pause the media clock until the surround has actually been submitted, then give the
/// unlit device a short settling interval. Loading speed never consumes the opening shot.
#[derive(Default)]
struct VideoStart {
    origin: Option<f64>,
    ready_at: Option<f64>,
}

impl VideoStart {
    fn clock(&mut self, time: f64) -> (f64, bool) {
        let origin = *self.origin.get_or_insert(time);
        match self.ready_at {
            Some(at) if time >= at => (origin + time - at, false),
            _ => (origin, true),
        }
    }

    fn presented(&mut self, time: f64, ready: bool) {
        if ready && self.ready_at.is_none() {
            self.ready_at = Some(time + 0.25);
        }
    }
}

struct Surface {
    wl: WlSurface,
    layer: ZwlrLayerSurfaceV1,
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
    configured: bool,
    logical: (u32, u32),
    scale120: u32,
    integer_scale: i32,
    size: (u32, u32),
    buffers: Vec<Buffer>,
    waiting: bool,
    /// A frame came due while the compositor wasn't ready for one, so the next is drawn as soon as it is.
    owed: bool,
    guard: flash::Guard,
    pixmap: Option<Pixmap>,
    /// The last frame shown, so an unchanged one isn't drawn again.
    shown: Option<Frame>,
    video_start: VideoStart,
}

struct Output {
    name: u32,
    wl: WlOutput,
    scale: i32,
    surface: Option<Surface>,
    /// The output's own name ("eDP-1"), once the compositor has said it.
    label: Option<String>,
    refresh: i32,
    normal_transform: bool,
}

struct Globals {
    compositor: WlCompositor,
    shm: WlShm,
    layer_shell: ZwlrLayerShellV1,
    viewporter: Option<WpViewporter>,
    fractional: Option<WpFractionalScaleManagerV1>,
    dmabuf: Option<ZwpLinuxDmabufV1>,
}

struct State {
    linear_xrgb: bool,
    globals: Globals,
    overlay: Option<crate::overlay::Overlay>,
    outputs: Vec<Output>,
    xbgr: bool,
    pointer: Option<WlPointer>,
    keyboard: Option<WlKeyboard>,
    touch: Option<WlTouch>,
    started: Instant,
    pointer_origin: Option<(f64, f64)>,
    dismissed: Option<&'static str>,
    scratch: Vec<u8>,
    /// For a live preview: where it goes. Surfaces are then made only for it, once the outputs' names are known.
    region: Option<Region>,
}

impl State {
    fn add_output(&mut self, registry: &WlRegistry, name: u32, version: u32, qh: &QueueHandle<State>) {
        let wl = registry.bind::<WlOutput, _, _>(name, version.clamp(2, 4), qh, name);
        let surface = if self.region.is_some() { None } else { Some(self.make_surface(&wl, name, qh)) };
        self.outputs.push(Output { name, wl, scale: 1, surface, label: None, refresh: 0, normal_transform: true });
    }

    fn make_surface(&self, output: &WlOutput, name: u32, qh: &QueueHandle<State>) -> Surface {
        let g = &self.globals;
        let wl = g.compositor.create_surface(qh, name);
        let layer = g.layer_shell.get_layer_surface(&wl, Some(output), Layer::Overlay, "screensafe".into(), qh, name);
        layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_size(0, 0);
        let (viewport, fractional) = match (&g.viewporter, &g.fractional) {
            (Some(v), Some(f)) => (Some(v.get_viewport(&wl, qh, ())), Some(f.get_fractional_scale(&wl, qh, name))),
            _ => (None, None),
        };
        wl.commit();
        Surface {
            wl,
            layer,
            viewport,
            fractional,
            configured: false,
            logical: (0, 0),
            scale120: 0,
            integer_scale: 1,
            size: (0, 0),
            buffers: Vec::new(),
            waiting: false,
            owed: false,
            guard: flash::Guard::new(),
            pixmap: None,
            shown: None,
            video_start: VideoStart::default(),
        }
    }

    /// The live preview's surface: a small overlay at the region, taking no keyboard focus, with an empty input
    /// region so clicks and the pointer pass through to the panel beneath it.
    fn make_region_surface(&self, output: &WlOutput, name: u32, region: &Region, qh: &QueueHandle<State>) -> Surface {
        let g = &self.globals;
        let wl = g.compositor.create_surface(qh, name);
        let layer = g.layer_shell.get_layer_surface(&wl, Some(output), Layer::Overlay, "screensafe-preview".into(), qh, name);
        layer.set_anchor(Anchor::Top | Anchor::Left);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_margin(region.y as i32, 0, 0, region.x as i32);
        layer.set_size(region.w, region.h);
        let nothing = g.compositor.create_region(qh, ());
        wl.set_input_region(Some(&nothing));
        nothing.destroy();
        let (viewport, fractional) = match (&g.viewporter, &g.fractional) {
            (Some(v), Some(f)) => (Some(v.get_viewport(&wl, qh, ())), Some(f.get_fractional_scale(&wl, qh, name))),
            _ => (None, None),
        };
        wl.commit();
        Surface {
            wl,
            layer,
            viewport,
            fractional,
            configured: false,
            logical: (0, 0),
            scale120: 0,
            integer_scale: 1,
            size: (0, 0),
            buffers: Vec::new(),
            waiting: false,
            owed: false,
            guard: flash::Guard::new(),
            pixmap: None,
            shown: None,
            video_start: VideoStart::default(),
        }
    }

    /// Makes the live preview's surface on its output, once the outputs have been named.
    fn place_preview(&mut self, qh: &QueueHandle<State>) -> Result<(), String> {
        let Some(region) = self.region.clone() else { return Ok(()) };
        let index = self
            .outputs
            .iter()
            .position(|o| region.output.is_some() && o.label == region.output)
            .or(if self.outputs.is_empty() { None } else { Some(0) })
            .ok_or("no screens are connected")?;
        let (wl, name) = (self.outputs[index].wl.clone(), self.outputs[index].name);
        let surface = self.make_region_surface(&wl, name, &region, qh);
        self.outputs[index].surface = Some(surface);
        Ok(())
    }

    /// Moves or resizes the live preview.
    fn move_preview(&mut self, region: Region) {
        for s in self.outputs.iter_mut().filter_map(|o| o.surface.as_mut()) {
            s.layer.set_margin(region.y as i32, 0, 0, region.x as i32);
            s.layer.set_size(region.w, region.h);
            s.wl.commit();
        }
        self.region = Some(region);
    }

    fn surface_mut(&mut self, name: u32) -> Option<&mut Surface> {
        self.outputs.iter_mut().find(|o| o.name == name).and_then(|o| o.surface.as_mut())
    }

    fn dismiss(&mut self, why: &'static str) {
        if self.dismissed.is_none() {
            self.dismissed = Some(why);
        }
    }

    /// How the stage sits on each screen being drawn.
    fn views(&self, pack: &Pack) -> Vec<View> {
        self.outputs
            .iter()
            .filter_map(|o| o.surface.as_ref())
            .filter(|s| s.size.0 > 0 && s.size.1 > 0)
            .map(|s| View::new(s.size.0, s.size.1, pack.stage))
            .collect()
    }

    fn settled(&self) -> bool {
        self.started.elapsed() >= SETTLE
    }

    /// Whether a surface owed a frame is now ready for one.
    fn owed_ready(&self) -> bool {
        self.outputs.iter().filter_map(|o| o.surface.as_ref()).any(|s| s.owed && !s.waiting && s.buffers.iter().filter(|b| b.busy).count() < 2)
    }

    /// Draws the frame for `time` on every surface that's ready for one, or only on those owed one.
    fn draw(&mut self, pack: &Pack, eval: &mut Evaluator, raster: &mut Raster, time: f64, only_owed: bool, qh: &QueueHandle<State>) -> Result<(), String> {
        DRAWS.fetch_add(1, Ordering::SeqCst);
        let result = self.draw_surfaces(pack, eval, raster, time, only_owed, qh);
        DRAWS.fetch_add(1, Ordering::SeqCst);
        result
    }

    fn draw_surfaces(
        &mut self,
        pack: &Pack,
        eval: &mut Evaluator,
        raster: &mut Raster,
        time: f64,
        only_owed: bool,
        qh: &QueueHandle<State>,
    ) -> Result<(), String> {
        let xbgr = self.xbgr;
        let shm = self.globals.shm.clone();
        raster.poll_images();
        let arrived = std::mem::take(&mut raster.images_arrived);
        for output in &mut self.outputs {
            let output_scale = output.scale;
            let name = output.name;
            let Some(s) = output.surface.as_mut() else { continue };
            if !s.configured || s.logical.0 == 0 || s.logical.1 == 0 || (only_owed && !s.owed) {
                continue;
            }
            if s.waiting {
                s.owed = true;
                continue;
            }
            s.owed = false;
            let (w, h) = if s.viewport.is_some() && s.scale120 > 0 {
                (((s.logical.0 as u64 * s.scale120 as u64 + 60) / 120) as u32, ((s.logical.1 as u64 * s.scale120 as u64 + 60) / 120) as u32)
            } else {
                let k = s.integer_scale.max(output_scale).clamp(1, 8) as u32;
                (s.logical.0 * k, s.logical.1 * k)
            };
            let physical_scale = View::new(w, h, pack.stage).scale;
            let (w, h) = if pack.uses_videos && s.viewport.is_some() { crate::video::render_size(w, h) } else { (w, h) };
            if (w, h) != s.size {
                for b in s.buffers.drain(..) {
                    b.wl.destroy();
                }
                s.size = (w, h);
                s.pixmap = Pixmap::new(w, h);
                s.shown = None;
                raster.largest = (raster.largest.0.max(w), raster.largest.1.max(h));
            }
            let Some(pixmap) = s.pixmap.as_mut() else { return Err(format!("can't draw a {w} x {h} screen")) };
            let view = View::new(w, h, pack.stage);
            let (media_time, warming) = if pack.uses_videos { s.video_start.clock(time) } else { (time, false) };
            eval.set_pixel_scale(physical_scale);
            let mut frame = eval.frame(pack, media_time, view.aspect());
            let surround_ready = !pack.uses_videos || raster.video_surround_ready(pack, &frame, view);
            if warming || !surround_ready {
                // Keep the device and its black glass; don't queue or reveal the picture yet.
                frame.items.retain(|item| !matches!(item, Item::Image { source, .. } if matches!(source.src, ImageSrc::Video { .. })));
            }
            if let Some(overlay) = &self.overlay {
                overlay.append(&mut frame, time, view, physical_scale, pack.foreground, raster.metrics()[0]);
            }
            if s.shown.as_ref() == Some(&frame) && !arrived {
                // Nothing has changed: keep what's on screen, and spend nothing on it.
                continue;
            }
            raster.draw(pack, &frame, &mut pixmap.as_mut(), view);
            let cells = flash::measure(pixmap.data(), w as usize, h as usize, w as usize * 4, false);
            if !s.guard.admit(&cells, time) {
                // Keep the last frame up; ask to be told when the next one may be drawn.
                s.wl.frame(qh, name);
                s.wl.commit();
                s.waiting = true;
                continue;
            }
            let index = match s.buffers.iter().position(|b| !b.busy) {
                Some(i) => i,
                None if s.buffers.len() < 2 => {
                    s.buffers.push(new_buffer(&shm, w, h, xbgr, name, s.buffers.len(), qh)?);
                    s.buffers.len() - 1
                }
                None => {
                    s.owed = true;
                    continue;
                }
            };
            let bytes = if xbgr {
                pixmap.data()
            } else {
                self.scratch.clear();
                self.scratch.extend_from_slice(pixmap.data());
                for p in self.scratch.chunks_exact_mut(4) {
                    p.swap(0, 2);
                }
                &self.scratch
            };
            let buffer = &mut s.buffers[index];
            write_all_at(&buffer.memfd, bytes)?;
            buffer.busy = true;
            if let Some(v) = &s.viewport {
                v.set_destination(s.logical.0 as i32, s.logical.1 as i32);
                s.wl.set_buffer_scale(1);
            } else {
                s.wl.set_buffer_scale((w / s.logical.0.max(1)).max(1) as i32);
            }
            s.wl.attach(Some(&buffer.wl), 0, 0);
            s.wl.damage_buffer(0, 0, w as i32, h as i32);
            s.wl.frame(qh, name);
            s.wl.commit();
            if pack.uses_videos {
                s.video_start.presented(time, surround_ready);
            }
            s.waiting = true;
            s.shown = Some(frame);
        }
        Ok(())
    }
}

fn write_all_at(fd: &OwnedFd, mut bytes: &[u8]) -> Result<(), String> {
    let mut offset = 0u64;
    while !bytes.is_empty() {
        let n = rustix::io::pwrite(fd, bytes, offset).map_err(|e| format!("can't write a frame: {e}"))?;
        if n == 0 {
            return Err("can't write a frame".into());
        }
        bytes = &bytes[n..];
        offset += n as u64;
    }
    Ok(())
}

fn new_buffer(shm: &WlShm, w: u32, h: u32, xbgr: bool, name: u32, index: usize, qh: &QueueHandle<State>) -> Result<Buffer, String> {
    let stride = w as i32 * 4;
    let len = stride as u64 * h as u64;
    let memfd = rustix::fs::memfd_create("screensafe", rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING)
        .map_err(|e| format!("can't allocate a frame: {e}"))?;
    rustix::fs::ftruncate(&memfd, len).map_err(|e| format!("can't allocate a frame: {e}"))?;
    let pool = shm.create_pool(memfd.as_fd(), len as i32, qh, ());
    let format = if xbgr { wl_shm::Format::Xbgr8888 } else { wl_shm::Format::Xrgb8888 };
    let wl = pool.create_buffer(0, w as i32, h as i32, stride, format, qh, (name, index));
    pool.destroy();
    Ok(Buffer { memfd, wl, busy: false })
}

/// Opens the Wayland connection and checks the compositor has what ScreenSafe needs. Done before confinement,
/// since connecting opens the compositor's socket.
pub struct Display {
    queue: EventQueue<State>,
    state: State,
    conn: Connection,
}

pub fn connect() -> Result<Display, String> {
    connect_to(None)
}

/// Connects for a live preview at `region`.
pub fn connect_preview(region: Region) -> Result<Display, String> {
    connect_to(Some(region))
}

fn connect_to(region: Option<Region>) -> Result<Display, String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("can't reach the Wayland compositor: {e}"))?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| format!("can't list the compositor's features: {e}"))?;
    let qh = queue.handle();
    let compositor = globals.bind::<WlCompositor, _, _>(&qh, 4..=6, ()).map_err(|_| "the compositor is too old (needs wl_compositor 4)")?;
    let shm = globals.bind::<WlShm, _, _>(&qh, 1..=1, ()).map_err(|_| "the compositor has no shared memory buffers")?;
    let layer_shell = globals.bind::<ZwlrLayerShellV1, _, _>(&qh, 1..=4, ()).map_err(|_| "the compositor doesn't offer layer surfaces (wlr-layer-shell)")?;
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    let fractional = globals.bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok();
    let dmabuf = globals.bind::<ZwpLinuxDmabufV1, _, _>(&qh, 3..=3, ()).ok();
    let mut state = State {
        linear_xrgb: false,
        overlay: None,
        globals: Globals { compositor, shm, layer_shell, viewporter, fractional, dmabuf },
        outputs: Vec::new(),
        xbgr: false,
        pointer: None,
        keyboard: None,
        touch: None,
        started: Instant::now(),
        pointer_origin: None,
        dismissed: None,
        scratch: Vec::new(),
        region,
    };
    if let Ok(seat) = globals.bind::<WlSeat, _, _>(&qh, 1..=7, ()) {
        let _ = seat;
    }
    let registry = globals.registry().clone();
    for global in globals.contents().clone_list() {
        if global.interface == "wl_output" {
            state.add_output(&registry, global.name, global.version, &qh);
        }
    }
    if state.outputs.is_empty() {
        return Err("no screens are connected".into());
    }
    queue.roundtrip(&mut state).map_err(|e| format!("the compositor stopped responding: {e}"))?;
    if state.region.is_some() {
        state.place_preview(&qh)?;
        queue.roundtrip(&mut state).map_err(|e| format!("the compositor stopped responding: {e}"))?;
    }
    Ok(Display { queue, state, conn })
}

/// Counts the starts and ends of drawing, so it's odd while a frame is being drawn. Input is read between frames, so
/// a frame that never finishes would leave the screen covered and deaf to keys: `watch_draws` ends the process then.
pub static DRAWS: AtomicU64 = AtomicU64::new(0);

impl Display {
    /// Presents one approved immutable loop. The worker has no Wayland socket;
    /// only this confined Rust process imports its three fixed linear buffers.
    /// There is one frame in flight, no catch-up, and no live composition.
    pub fn prepared(mut self, size: (u32, u32), frames: u32, fps: u32, limit: Option<Duration>) -> Result<&'static str, String> {
        use crate::prepared::{receive, request, MAGIC, XRGB};
        if self.state.outputs.len() != 1 || !self.state.linear_xrgb {
            return Err("prepared display format unavailable".into());
        }
        let dmabuf = self.state.globals.dmabuf.clone().ok_or("prepared buffers unavailable")?;
        let qh = self.queue.handle();
        let input = std::io::stdin();
        let started = Instant::now();
        self.state.started = started;
        let mut expected = 0;
        let mut outstanding: Option<(u32, usize, Instant)> = None;
        let mut ready: Option<(u32, usize)> = None;
        let mut next = started;
        loop {
            self.queue.dispatch_pending(&mut self.state).map_err(|e| format!("Wayland error: {e}"))?;
            if let Some(why) = self.state.dismissed {
                return Ok(why);
            }
            if limit.is_some_and(|d| started.elapsed() >= d) {
                return Ok("time limit");
            }
            if self.state.outputs.len() != 1 {
                return Err("prepared output changed".into());
            }
            let output = &mut self.state.outputs[0];
            if !output.normal_transform || !(1..=64000).contains(&output.refresh) {
                return Err("prepared display cadence changed".into());
            }
            let Some(s) = output.surface.as_mut() else {
                return Ok("no screens left");
            };
            if s.configured {
                let scale = if s.scale120 > 0 { s.scale120 } else { (s.integer_scale.max(output.scale).clamp(1, 8) * 120) as u32 };
                let actual = (((s.logical.0 as u64 * scale as u64 + 60) / 120) as u32, ((s.logical.1 as u64 * scale as u64 + 60) / 120) as u32);
                if actual != size || s.viewport.is_none() {
                    return Err("prepared output size changed".into());
                }
            }
            // Bounded work per iteration; a malformed/flooded channel cannot
            // starve keyboard, pointer or compositor events.
            for _ in 0..4 {
                let Some(mut packet) = receive()? else { break };
                if packet.words == [MAGIC, 4, 0, 0] && packet.fds.is_empty() {
                    return Ok("asked to stop");
                }
                if s.buffers.len() < 3 {
                    let w = &packet.words;
                    let index = s.buffers.len();
                    if packet.fds.len() != 1 {
                        return Err("invalid prepared buffer layout".into());
                    }
                    let fd = packet.fds.pop().unwrap();
                    let stride = crate::prepared::buffer_layout(w, &fd, index, size)?;
                    let meta = rustix::fs::fstat(&fd).map_err(|e| format!("can't inspect GPU buffer: {e}"))?;
                    for previous in &s.buffers {
                        let other = rustix::fs::fstat(&previous.memfd).map_err(|e| format!("can't inspect buffer identity: {e}"))?;
                        if (meta.st_dev, meta.st_ino) == (other.st_dev, other.st_ino) {
                            return Err("prepared pool aliases an existing buffer".into());
                        }
                    }
                    let params = dmabuf.create_params(&qh, ());
                    params.add(fd.as_fd(), 0, 0, stride, 0, 0);
                    let wl = params.create_immed(size.0 as i32, size.1 as i32, XRGB, zwp_linux_buffer_params_v1::Flags::empty(), &qh, (output.name, index));
                    params.destroy();
                    s.buffers.push(Buffer { memfd: fd, wl, busy: false });
                } else {
                    let Some((frame, slot, _)) = outstanding else {
                        return Err("unsolicited prepared frame".into());
                    };
                    if packet.words != [MAGIC, 3, frame, slot as u32] || !packet.fds.is_empty() || ready.is_some() {
                        return Err("prepared frame sequence mismatch".into());
                    }
                    outstanding = None;
                    ready = Some((frame, slot));
                }
            }
            if s.buffers.len() < 3 && started.elapsed() > Duration::from_secs(5) {
                return Err("prepared buffer setup timed out".into());
            }
            if outstanding.is_some_and(|(_, _, at)| at.elapsed() > Duration::from_secs(5)) {
                return Err("prepared frame timed out".into());
            }
            let now = Instant::now();
            if ready.is_some() && s.configured && !s.waiting && now >= next {
                let (frame, slot) = ready.take().unwrap();
                if frame != expected || s.buffers[slot].busy {
                    return Err("prepared ownership mismatch".into());
                }
                let buffer = &mut s.buffers[slot];
                buffer.busy = true;
                s.viewport.as_ref().unwrap().set_destination(s.logical.0 as i32, s.logical.1 as i32);
                s.wl.set_buffer_scale(1);
                s.wl.attach(Some(&buffer.wl), 0, 0);
                s.wl.damage_buffer(0, 0, size.0 as i32, size.1 as i32);
                s.wl.frame(&qh, output.name);
                s.wl.commit();
                s.waiting = true;
                expected = (expected + 1) % frames;
                // A delayed frame slows playback, never compresses the tested
                // sequence into a burst to catch up with a wall clock.
                next = now + Duration::from_secs_f64(1.0 / fps as f64);
            }
            if s.buffers.len() == 3 && outstanding.is_none() && ready.is_none() {
                if let Some(slot) = s.buffers.iter().position(|b| !b.busy) {
                    request(expected, slot)?;
                    outstanding = Some((expected, slot, Instant::now()));
                }
            }
            let wait = if ready.is_some() && !s.waiting { next.saturating_duration_since(Instant::now()) } else { Duration::from_millis(250) }
                .min(Duration::from_millis(250));
            self.queue.flush().map_err(|e| format!("Wayland error: {e}"))?;
            if let Some(guard) = self.conn.prepare_read() {
                let fd = guard.connection_fd();
                let mut fds = [rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN), rustix::event::PollFd::new(&input, rustix::event::PollFlags::IN)];
                let timeout = rustix::event::Timespec { tv_sec: 0, tv_nsec: wait.as_nanos() as i64 };
                match rustix::event::poll(&mut fds, Some(&timeout)) {
                    Ok(_) => {
                        if fds[0].revents().contains(rustix::event::PollFlags::IN) {
                            guard.read().map_err(|e| format!("Wayland error: {e}"))?;
                        }
                    }
                    Err(rustix::io::Errno::INTR) => {}
                    Err(e) => return Err(format!("prepared poll failed: {e}")),
                }
            }
        }
    }

    /// The pixel format frames are sent in.
    pub fn format(&self) -> &'static str {
        if self.state.xbgr {
            "XBGR8888, sent as drawn"
        } else {
            "XRGB8888, channels swapped"
        }
    }

    /// Shows the pack until input dismisses it, returning why it closed.
    pub fn run(mut self, pack: &Pack, options: &Options, raster: &mut Raster) -> Result<&'static str, String> {
        let qh = self.queue.handle();
        self.state.overlay = Some(crate::overlay::Overlay::new(options.overlay.clone(), options.clock.clone().unwrap_or_else(crate::clock::Clock::fixed)));
        let mut control = crate::overlay::Input::default();
        let mut eval = Evaluator::new(pack, options.seed, raster.metrics());
        if let Some(clock) = &options.clock {
            eval.set_clock(clock.clone());
        }
        let start = Instant::now();
        self.state.started = start;
        let mut next = start;
        let mut looked_ahead = start;
        loop {
            self.queue.dispatch_pending(&mut self.state).map_err(|e| format!("Wayland error: {e}"))?;
            if let Some(why) = self.state.dismissed {
                return Ok(why);
            }
            if self.state.outputs.iter().all(|o| o.surface.is_none()) {
                return Ok("no screens left");
            }
            if options.exit_after.is_some_and(|d| start.elapsed() >= d) {
                return Ok("time limit");
            }
            let now = Instant::now();
            for line in control.poll() {
                if line.trim() == "quit" {
                    return Ok("asked to stop");
                }
                if let Some(overlay) = &mut self.state.overlay {
                    overlay.control(&line, options.start + start.elapsed().as_secs_f64());
                }
            }
            if now >= looked_ahead && pack.prefetches_images() {
                // Start decoding and resampling the images the pack will show a little later, so they're ready in time.
                let ahead = options.start + start.elapsed().as_secs_f64() + LOOK_AHEAD;
                let frame = eval.frame(pack, ahead, 16.0 / 9.0);
                raster.prefetch(pack, &frame, &self.state.views(pack));
                looked_ahead = now + Duration::from_millis(500);
            }
            if now >= next {
                let time = options.start + start.elapsed().as_secs_f64();
                self.state.draw(pack, &mut eval, raster, time, false, &qh)?;
                let (scene, _, _) = Evaluator::locate(pack, time);
                let fps = pack.scenes[scene].fps.min(options.max_fps).max(1.0);
                next += Duration::from_secs_f64(1.0 / fps);
                if next < now {
                    next = now + Duration::from_secs_f64(1.0 / fps);
                }
            } else if self.state.owed_ready() {
                // A frame that came due while the compositor was busy: draw it now rather than skip it.
                let time = options.start + start.elapsed().as_secs_f64();
                self.state.draw(pack, &mut eval, raster, time, true, &qh)?;
            }
            self.queue.flush().map_err(|e| format!("Wayland error: {e}"))?;
            let wait = next.saturating_duration_since(Instant::now()).min(Duration::from_millis(250));
            if let Some(guard) = self.conn.prepare_read() {
                let ready = {
                    let fd = guard.connection_fd();
                    let mut fds = [rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN)];
                    let timeout = rustix::event::Timespec { tv_sec: wait.as_secs() as i64, tv_nsec: wait.subsec_nanos() as i64 };
                    match rustix::event::poll(&mut fds, Some(&timeout)) {
                        Ok(n) => n > 0,
                        Err(rustix::io::Errno::INTR) => false,
                        Err(e) => return Err(format!("can't wait for the compositor: {e}")),
                    }
                };
                if ready {
                    guard.read().map_err(|e| format!("Wayland error: {e}"))?;
                }
            }
        }
    }
}

/// The longest line a live preview reads from its standard input; longer ones are dropped whole.
const CONTROL_LINE: usize = 1024;

impl Display {
    /// Shows the pack as a live preview at its region until told to stop, or until its standard input closes. Lines
    /// on standard input change it while it runs, without starting it over: `settings NAME=VALUE,...` loads the pack
    /// again with those settings (through `reload`), `region X,Y,W,H` moves it, and `quit` ends it.
    pub fn preview(
        mut self,
        mut pack: Pack,
        options: &Options,
        raster: &mut Raster,
        reload: &mut dyn FnMut(&str) -> Result<Pack, String>,
    ) -> Result<&'static str, String> {
        let qh = self.queue.handle();
        self.state.overlay = Some(crate::overlay::Overlay::new(options.overlay.clone(), options.clock.clone().unwrap_or_else(crate::clock::Clock::fixed)));
        let stdin = std::io::stdin();
        let input = stdin.as_fd();
        let flags = rustix::fs::fcntl_getfl(input).map_err(|e| format!("can't read standard input: {e}"))?;
        rustix::fs::fcntl_setfl(input, flags | rustix::fs::OFlags::NONBLOCK).map_err(|e| format!("can't read standard input: {e}"))?;
        let mut line: Vec<u8> = Vec::new();
        let mut overlong = false;
        let mut eval = Evaluator::new(&pack, options.seed, raster.metrics());
        if let Some(clock) = &options.clock {
            eval.set_clock(clock.clone());
        }
        let start = Instant::now();
        self.state.started = start;
        let mut next = start;
        let mut looked_ahead = start;
        loop {
            self.queue.dispatch_pending(&mut self.state).map_err(|e| format!("Wayland error: {e}"))?;
            if self.state.outputs.iter().all(|o| o.surface.is_none()) {
                return Ok("closed");
            }
            let now = Instant::now();
            if now >= looked_ahead && pack.prefetches_images() {
                let ahead = options.start + start.elapsed().as_secs_f64() + LOOK_AHEAD;
                let frame = eval.frame(&pack, ahead, 16.0 / 9.0);
                raster.prefetch(&pack, &frame, &self.state.views(&pack));
                looked_ahead = now + Duration::from_millis(500);
            }
            if now >= next {
                let time = options.start + start.elapsed().as_secs_f64();
                self.state.draw(&pack, &mut eval, raster, time, false, &qh)?;
                let (scene, _, _) = Evaluator::locate(&pack, time);
                let fps = pack.scenes[scene].fps.min(options.max_fps).max(1.0);
                next += Duration::from_secs_f64(1.0 / fps);
                if next < now {
                    next = now + Duration::from_secs_f64(1.0 / fps);
                }
            } else if self.state.owed_ready() {
                let time = options.start + start.elapsed().as_secs_f64();
                self.state.draw(&pack, &mut eval, raster, time, true, &qh)?;
            }
            self.queue.flush().map_err(|e| format!("Wayland error: {e}"))?;
            let wait = next.saturating_duration_since(Instant::now()).min(Duration::from_millis(250));
            let mut commands = Vec::new();
            if let Some(guard) = self.conn.prepare_read() {
                let (wayland, typed) = {
                    let fd = guard.connection_fd();
                    let mut fds =
                        [rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN), rustix::event::PollFd::new(&input, rustix::event::PollFlags::IN)];
                    let timeout = rustix::event::Timespec { tv_sec: wait.as_secs() as i64, tv_nsec: wait.subsec_nanos() as i64 };
                    match rustix::event::poll(&mut fds, Some(&timeout)) {
                        Ok(_) => (!fds[0].revents().is_empty(), !fds[1].revents().is_empty()),
                        Err(rustix::io::Errno::INTR) => (false, false),
                        Err(e) => return Err(format!("can't wait for the compositor: {e}")),
                    }
                };
                if wayland {
                    guard.read().map_err(|e| format!("Wayland error: {e}"))?;
                } else {
                    drop(guard);
                }
                if typed {
                    let mut chunk = [0u8; 512];
                    // At most a few kilobytes a turn, so a writer that never stops can't keep frames from being drawn.
                    let mut taken = 0;
                    while taken < 8 * CONTROL_LINE {
                        match rustix::io::read(input, &mut chunk) {
                            Ok(0) => return Ok("standard input closed"),
                            Ok(n) => {
                                taken += n;
                                for &b in &chunk[..n] {
                                    if b == b'\n' {
                                        if !overlong {
                                            commands.push(String::from_utf8_lossy(&line).into_owned());
                                        }
                                        line.clear();
                                        overlong = false;
                                    } else if line.len() < CONTROL_LINE {
                                        line.push(b);
                                    } else {
                                        overlong = true;
                                    }
                                }
                            }
                            Err(rustix::io::Errno::AGAIN) => break,
                            Err(rustix::io::Errno::INTR) => continue,
                            Err(e) => return Err(format!("can't read standard input: {e}")),
                        }
                    }
                }
            }
            // Only the last of several settings read at once is loaded: each would replace the one before.
            let last_settings = commands.iter().rposition(|c| c.trim().starts_with("settings "));
            for (i, command) in commands.iter().enumerate() {
                let (verb, rest) = command.trim().split_once(' ').unwrap_or((command.trim(), ""));
                if verb == "settings" && Some(i) != last_settings {
                    continue;
                }
                match verb {
                    "quit" => return Ok("asked to stop"),
                    "overlay" | "stats" => {
                        if let Some(overlay) = &mut self.state.overlay {
                            overlay.control(command.trim(), options.start + start.elapsed().as_secs_f64());
                        }
                    }
                    "settings" => match reload(rest) {
                        Ok(p) => {
                            // The same moment of the pack, as it now is: a new evaluator and raster for its new
                            // values, and every surface drawn again.
                            pack = p;
                            eval = Evaluator::new(&pack, options.seed, raster.metrics());
                            if let Some(clock) = &options.clock {
                                eval.set_clock(clock.clone());
                            }
                            let background = raster.background_decoding;
                            *raster = Raster::new();
                            raster.background_decoding = background;
                            raster.load_fonts(&pack);
                            for s in self.state.outputs.iter_mut().filter_map(|o| o.surface.as_mut()) {
                                s.shown = None;
                                s.video_start = VideoStart::default();
                            }
                            next = Instant::now();
                        }
                        Err(e) => tell!("screensafe: {e}"),
                    },
                    "region" => {
                        let output = self.state.region.as_ref().and_then(|r| r.output.clone());
                        match Region::parse(rest, output) {
                            Ok(region) => self.state.move_preview(region),
                            Err(e) => tell!("screensafe: {e}"),
                        }
                    }
                    _ => tell!("screensafe: unknown command"),
                }
            }
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(state: &mut Self, registry: &WlRegistry, event: wl_registry::Event, _: &GlobalListContents, _: &Connection, qh: &QueueHandle<Self>) {
        match event {
            // A screen connected while a live preview shows gets nothing: the preview stays where it is.
            wl_registry::Event::Global { interface, .. } if interface == "wl_output" && state.region.is_some() => {}
            wl_registry::Event::Global { name, interface, version } if interface == "wl_output" => {
                state.add_output(registry, name, version, qh);
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(i) = state.outputs.iter().position(|o| o.name == name) {
                    let output = state.outputs.remove(i);
                    if let Some(s) = output.surface {
                        destroy_surface(s);
                    }
                    if output.wl.version() >= 3 {
                        output.wl.release();
                    }
                }
            }
            _ => {}
        }
    }
}

fn destroy_surface(s: Surface) {
    for b in s.buffers {
        b.wl.destroy();
    }
    if let Some(f) = s.fractional {
        f.destroy();
    }
    if let Some(v) = s.viewport {
        v.destroy();
    }
    s.layer.destroy();
    s.wl.destroy();
}

impl Dispatch<WlOutput, u32> for State {
    fn event(state: &mut Self, _: &WlOutput, event: wl_output::Event, name: &u32, _: &Connection, _: &QueueHandle<Self>) {
        let Some(o) = state.outputs.iter_mut().find(|o| o.name == *name) else { return };
        match event {
            wl_output::Event::Mode { flags: WEnum::Value(flags), refresh, .. } if flags.contains(wl_output::Mode::Current) => o.refresh = refresh,
            wl_output::Event::Geometry { transform, .. } => o.normal_transform = transform == WEnum::Value(wl_output::Transform::Normal),
            wl_output::Event::Scale { factor } => o.scale = factor,
            wl_output::Event::Name { name } => o.label = Some(name.chars().take(64).collect()),
            _ => {}
        }
    }
}

impl Dispatch<ZwpLinuxDmabufV1, ()> for State {
    fn event(state: &mut Self, _: &ZwpLinuxDmabufV1, event: zwp_linux_dmabuf_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zwp_linux_dmabuf_v1::Event::Modifier { format, modifier_hi: 0, modifier_lo: 0 } = event {
            if format == crate::prepared::XRGB {
                state.linear_xrgb = true;
            }
        }
    }
}
delegate_noop!(State: ignore ZwpLinuxBufferParamsV1);

impl Dispatch<WlSurface, u32> for State {
    fn event(state: &mut Self, _: &WlSurface, event: wl_surface::Event, name: &u32, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_surface::Event::PreferredBufferScale { factor } = event {
            if let Some(s) = state.surface_mut(*name) {
                s.integer_scale = factor;
            }
        }
    }
}

impl Dispatch<WpFractionalScaleV1, u32> for State {
    fn event(state: &mut Self, _: &WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, name: &u32, _: &Connection, _: &QueueHandle<Self>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if let Some(s) = state.surface_mut(*name) {
                s.scale120 = scale.clamp(60, 120 * 8);
            }
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, u32> for State {
    fn event(state: &mut Self, layer: &ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, name: &u32, _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                layer.ack_configure(serial);
                if let Some(s) = state.surface_mut(*name) {
                    s.configured = true;
                    s.logical = (width.min(16384), height.min(16384));
                    s.waiting = false;
                }
            }
            zwlr_layer_surface_v1::Event::Closed => {
                if let Some(o) = state.outputs.iter_mut().find(|o| o.name == *name) {
                    if let Some(s) = o.surface.take() {
                        destroy_surface(s);
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, u32> for State {
    fn event(state: &mut Self, _: &WlCallback, event: wl_callback::Event, name: &u32, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { .. } = event {
            if let Some(s) = state.surface_mut(*name) {
                s.waiting = false;
            }
        }
    }
}

impl Dispatch<WlBuffer, (u32, usize)> for State {
    fn event(state: &mut Self, buffer: &WlBuffer, event: wl_buffer::Event, data: &(u32, usize), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = event {
            if let Some(s) = state.surface_mut(data.0) {
                if let Some(b) = s.buffers.iter_mut().find(|b| &b.wl == buffer) {
                    b.busy = false;
                }
            }
        }
    }
}

impl Dispatch<WlShm, ()> for State {
    fn event(state: &mut Self, _: &WlShm, event: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_shm::Event::Format { format: WEnum::Value(wl_shm::Format::Xbgr8888) } = event {
            state.xbgr = true;
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(state: &mut Self, seat: &WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = event {
            if caps.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
            }
            if caps.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(qh, ()));
            }
            if caps.contains(wl_seat::Capability::Touch) && state.touch.is_none() {
                state.touch = Some(seat.get_touch(qh, ()));
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn event(state: &mut Self, pointer: &WlPointer, event: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_pointer::Event::Enter { serial, surface_x, surface_y, .. } => {
                // No cursor over the screensaver. Positions are relative to the surface entered, so movement is
                // measured afresh from here; a surface appearing under a still pointer then can't dismiss it.
                pointer.set_cursor(serial, None, 0, 0);
                state.pointer_origin = state.settled().then_some((surface_x, surface_y));
            }
            wl_pointer::Event::Leave { .. } => state.pointer_origin = None,
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                if !state.settled() {
                    return;
                }
                let (ox, oy) = *state.pointer_origin.get_or_insert((surface_x, surface_y));
                if (surface_x - ox).hypot(surface_y - oy) > MOVE_LIMIT {
                    state.dismiss("pointer moved");
                }
            }
            wl_pointer::Event::Button { state: WEnum::Value(wl_pointer::ButtonState::Pressed), .. } => state.dismiss("button pressed"),
            wl_pointer::Event::Axis { .. } if state.settled() => state.dismiss("scrolled"),
            _ => {}
        }
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn event(state: &mut Self, _: &WlKeyboard, event: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_keyboard::Event::Key { state: WEnum::Value(wl_keyboard::KeyState::Pressed), .. } = event {
            state.dismiss("key pressed");
        }
    }
}

impl Dispatch<WlTouch, ()> for State {
    fn event(state: &mut Self, _: &WlTouch, event: wl_touch::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_touch::Event::Down { .. } = event {
            state.dismiss("touched");
        }
    }
}

delegate_noop!(State: ignore WlRegion);
delegate_noop!(State: ignore WlCompositor);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore ZwlrLayerShellV1);
delegate_noop!(State: ignore WpViewporter);
delegate_noop!(State: ignore WpViewport);
delegate_noop!(State: ignore WpFractionalScaleManagerV1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_clock_waits_for_a_presented_surround_without_consuming_media_time() {
        let mut start = VideoStart::default();
        assert_eq!(start.clock(0.0), (0.0, true));
        start.presented(0.0, false);
        assert_eq!(start.clock(12.0), (0.0, true), "slow loading must not skip the opening");
        start.presented(12.0, true);
        assert_eq!(start.clock(12.125), (0.0, true), "device exists before playback");
        assert_eq!(start.clock(12.25), (0.0, false));
        assert_eq!(start.clock(15.0), (2.75, false));
        start.presented(15.0, true);
        assert_eq!(start.clock(16.0), (3.75, false), "later submissions don't restart playback");
    }
}
