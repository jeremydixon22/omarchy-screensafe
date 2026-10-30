# Security

## Reporting a problem

Please report a security problem privately, through this repository's [private vulnerability reporting](https://github.com/jeremydixon22/omarchy-screensafe/security/advisories/new), rather than in a public issue. Include the ScreenSafe version, what you saw, and a pack or steps that show it, if you have them.

## What ScreenSafe trusts

ScreenSafe treats every **pack** as untrusted, wherever it comes from: the built-in ones, a theme's, or one you were sent. The **plugin** (`Service.qml`, `Panel.qml`, `ShieldMark.qml`) and the **renderer** (`bin/screensafe-x86_64` or `bin/screensafe-aarch64`) are the trusted code. Like every Omarchy plugin, they run with your user's permissions, so review them before enabling. [BINARY-PROVENANCE.md](docs/BINARY-PROVENANCE.md) shows how to check that the renderer is built from the source here.

## What a pack can do

A pack can only describe pictures. Its format has no commands, no file paths outside its own folder, no network addresses, and no loops.

**Expressions are pure and bounded.** The [expression language](docs/EXPRESSIONS.md) has no loops, recursion, user-defined functions, state, input or output. Each expression is type-checked and compiled when the pack loads, to a short list of operations that an interpreter runs; nothing is compiled to machine code. Every result is a finite number. Lengths, nesting and the work a scene can do in a frame are all limited; see [the limits](docs/PACKS.md#limits).

**Files are read beneath the pack's folder only.** Each file is opened with `openat2` and `RESOLVE_BENEATH`, `RESOLVE_NO_SYMLINKS`, `RESOLVE_NO_MAGICLINKS` and `RESOLVE_NO_XDEV`. Where the kernel or a sandbox doesn't offer `openat2`, the path is walked one plain name at a time, each opened without following links: still no links and nothing outside the pack's folder, though the walk may cross into another mount. Only regular files of a limited size are read, with names of plain characters. The only other files a pack can ask for are the current theme's background images, your own screensaver art, and your own badge (`~/.config/omarchy/screensafe/badge.png`, `.webp`, `.jpg` or `.jpeg`), all by name rather than by path. Your badge is read with the same limits as a pack's images and decoded only by the confined renderer; the shell never decodes it.

**Your photos and the clock.** A pack that shows photos gets the PNG, JPEG and WebP files directly in `~/Pictures/Screensaver`: each opened beneath that folder without following links, any name that isn't hidden, up to 64 MiB, at most 200 at once. They're opened under the first layer's read-only confinement and read only when shown. These already-open file descriptors remain readable after the second layer prevents new file access. For the clock, the renderer reads the time zone once, before it confines itself: `/etc/localtime`, or the zone `TZ` names under `/usr/share/zoneinfo`.

**Untrusted content is decoded in the renderer, not the shell.** Images, fonts, art and the pack itself are parsed by the renderer, a separate short-lived process written in Rust with `#![forbid(unsafe_code)]`. Image decoding is limited to PNG, JPEG and WebP, 16,384 pixels a side, 64 megapixels, and 512 MiB of memory; sprite sheets to 4,096 pixels a side and 16 megapixels a pack. A pack's fonts (up to 4 TrueType or OpenType files of 4 MiB each) are checked with `ttf-parser` when the pack loads, and read in full only after the renderer has confined itself. The shell sees only the renderer's bounded JSON listing of each pack's name, author, license, description, and any error, which the panel shows as plain text. Its thumbnails are drawn by the renderer too, once it has confined itself, and reach the shell as plain pixels, three bytes each and at most 480 a side, with an exact size and payload-length check before display. The panel adds its own fixed, uncompressed PPM header to those checked RGB pixels and loads the result with Qt's asynchronous Image item. The renderer supplies neither that header nor compressed image data; original pack images and fonts never reach Qt's image loader.

**What the shell's image loader reads.** Qt picks the reader for an image by its first bytes, whatever type the data URL names. Those bytes are always the panel's own header (`P6`, the width and height it asked for, and a maximum value of 255), so the only reader that ever runs is Qt's binary PPM reader, and the only part a pack influences is the pixel values that follow, exactly width × height × 3 bytes. This reader runs in the shell's process, outside the renderer's confinement; loading asynchronously keeps it off the shell's main thread, but isn't a security boundary.

## The renderer confines itself

The renderer confines itself with [Landlock](https://docs.kernel.org/userspace-api/landlock.html), in two layers, so that it never reads a pack unconfined.

1. **Before it reads anything from a pack,** for every command that reads packs (`run`, `thumbnail`, `catalog`, `check` and `render`), it can only read files beneath the folders and files it was given: the packs, the theme, your screensaver art and Omarchy's logo, your photos folder, and the four files your badge may be (not the folder they're in). It can't write anything, run anything, reach the network, use abstract Unix sockets, or signal processes outside itself. (`render` creates its picture's file before confining itself, and writes to it afterwards.)
2. **Once the pack is loaded,** and for a screensaver once it has connected to the Wayland compositor, it removes the rest:
   - **new filesystem access:** it can't open files, write, list folders or run programs; the photo descriptors it already holds remain readable, as described above;
   - **TCP connections and listening;**
   - **abstract Unix sockets and signals** to processes outside itself.

`no_new_privs` is set first, and core dumps are turned off. The renderer reports how fully the kernel enforced this: `fully confined`, or `partly confined (older kernel)` when the kernel enforces only the kinds of access it knows, which always include reading files. On a kernel without Landlock, or if confining fails, the renderer refuses to read any pack, and the panel says the screensaver couldn't be shown. The settings page's live preview (`preview`) stays at the first layer: it reads its pack again whenever a setting changes, so it keeps read-only access to that pack's folder, the theme, art and badge paths, and the selected Photos/Videos folders, and nothing else. To see the confinement work on your machine, run:

```sh
bin/screensafe-$(uname -m) sandbox-test
```

It applies the first layer to its own folder, checks that it can read there but can't read elsewhere or write, then applies the second, tries to read a file, list a folder, start a program and open a network connection, and reports that each was refused.

## What the plugin does

- **Programs:** the service selects fixed absolute program paths with a cleared environment. Packs supply data and numeric settings, never commands, executable paths or shell fragments:
  - ScreenSafe's static renderer for ordinary drawing, system readings and screensaver takeover;
  - Omarchy's `omarchy-shell lock isLocked` for lock checks;
  - `/usr/bin/python3` with the bundled compressed-video broker for catalog, thumbnails and Videos playback, or the prepared-loop controller when requested. A Python broker being present does not imply a codec or GPU worker has started.

  The video broker starts the bundled media-resource launcher, bundled Bubblewrap and system FFmpeg only when compressed frames are needed. Explicit MP4 import also uses system ffprobe. Prepared playback uses the bundled native GPU worker; preparation uses FFmpeg, and cache identity checks use fixed read-only `hyprctl`, `ldd` and `pacman -Q` calls. See [reviewer notes](docs/REVIEW-NOTES.md) and [native packaging](docs/NATIVE-PACKAGE.md) for each path and dependency.

  The normal service environment contains `HOME`, `XDG_RUNTIME_DIR`, `WAYLAND_DISPLAY`, `PYTHONDONTWRITEBYTECODE=1` and optional `XDG_STATE_HOME`/`TZ`. Lock checks also receive `OMARCHY_PATH` and a fixed `PATH`; prepared control additionally receives the exact shell parent PID and validated Hyprland instance signature. Media cages get their own fixed environment and no compositor connection.

  Short service calls are cancelled after ten seconds. Preparation has a twenty-minute service deadline and bounded stage deadlines. Playback ends on dismissal, lock or plugin unload; parent-lifetime guards prevent controllers from surviving the shell. A live frame still being drawn after five seconds ends the renderer and selects the built-in fallback. Prepared playback additionally bounds outstanding frame work and falls back on worker/profile failure; GPU driver hangs remain a platform risk.
- **Output:** the renderer bounds its listing to 64 KiB, and a thumbnail is at most 922 KB; its other output is a few lines. The plugin reads at most 64 listed packs, and stops any short call that prints more than it can legitimately print.
- **No text fields:** the panel has none, so nothing reads the clipboard, and everything from a pack is shown as plain text.
- **Files it changes:**
  - Omarchy's `~/.local/state/omarchy/toggles/screensaver-off`: created only while you have **Enable** on, and only if it wasn't already there; removed only if ScreenSafe created it.
  - Its own record of that, in `~/.local/state/screensafe/` (folder mode 0700, files 0600, written without following links).
  - `~/Pictures/Screensaver`, made only when you choose **Your photos** on a photo pack's settings page, if it isn't there (mode 0755, without following links). That folder is then opened in your file manager, the way About opens its links in your browser.
  - `~/Videos/Screensaver`, made only when the Videos folder action is chosen, using the same checked folder-opening path as Photos. Explicit import writes a new `.ssv` file only to the destination the user selects.
  - Private prepared loops, approval records and temporary preparation files beneath `~/.cache/omarchy/screensafe/prepared/` (directory mode 0700, completed media/records mode 0600). Status may create the empty directory; Preview can reuse approved loops but never generates one. Creation requires **Prepare selected screensaver**.
  - Its settings (whether it's on, the pack, its own idle time, the favorite packs, the values chosen for packs' settings, whether settings pages show a live preview, and optional overlay/performance preferences) in its own entry in `~/.config/omarchy/shell.json`, through Omarchy's plugin interface. It reads Omarchy's idle and lock times there but never changes them.
- **No network requests.** Runtime never downloads, installs packages, builds executables or starts a system service. The package manager is queried only for installed versions. No runtime path needs elevated privileges.

## Videos

An explicit isolated importer normalizes MP4s into `.ssv` archives. JPEG v1 frames use the renderer's image path; compressed v2 frames use the separate confined decoder described below. Both become renderer-owned pixels before final composition and runtime flash guarding. The shell does not decode video. [VIDEO-PROTOTYPE.md](docs/VIDEO-PROTOTYPE.md) documents its bounds and explicit import workflow; all application helpers are included.

The development renderer supplements Landlock with a seccomp filter: non-Unix socket creation (including UDP) and io_uring setup are denied across threads; after Wayland connects, all new socket creation and connection attempts are denied. Existing Wayland communication remains available. Watchdog threads in media-reading commands acknowledge startup only after giving up new filesystem access themselves, since Landlock is per-thread.

## Flashing

Flashing images can cause seizures in people with photosensitive epilepsy. [WCAG 2.2 success criterion 2.3.1](https://www.w3.org/TR/WCAG22/#three-flashes-or-below-threshold) allows no more than three general flashes and three red flashes in any one second, over an area larger than about a quarter of a 10° field of view. ScreenSafe measures every frame against that limit.

**The measure.** A frame is averaged into a 48 by 27 grid.

- **General transitions:** a cell makes one when its relative luminance changes by 0.1 or more from its last turning point, in the opposite direction to its previous one, and the darker of the two is below 0.8.
- **Red transitions:** these use WCAG's saturated-red measure, with a step of 20.
- **When a cell flashes:** a flash is two transitions, so a cell flashes when it makes more than six transitions in a second.
- **Too large an area:** this is more than a quarter of the cells in any 16 by 9 window. That window is a third of the screen's width and height, taking WCAG's 341 by 256 pixels on a 1024 by 768 screen as the 10° field.

**While showing.** The renderer won't show a frame that would cross that line. The previous frame stays up until a new one would be within it. So no pack can make ScreenSafe flash more than the limit as measured.

**When checking.** `screensafe check` draws every frame of a pack's loop, by default at 960 by 540 or at a chosen `--size`. It fails the pack, with the times, if any frame would cross the line.

This is an automated approximation of WCAG's test, not a certification. It measures averages over cells, so a pattern finer than a cell is judged by its average brightness.

## Limits of all this

The 0.6.0 testing candidate also has a trusted compressed-video broker. It retains read-only SSVv2 descriptors and keeps codec parsing inside a resource-limited user/mount/PID/network namespace cage. Fixed seccomp policy denies new sockets, processes, namespaces, ptrace and io_uring. The caller may explicitly enable H.264 hardware decoding, adding one identity-checked PCI render node and eight pinned read-only metadata files; no display connection, primary GPU node or whole sysfs is exposed. Software mode receives no GPU authority.

Copied pixels become renderer-owned before tone, surrounds, overlays and final runtime flash guarding. No GPU-writable buffer is admitted. Hardware startup verifies a full frame before streaming. The inherited pipe for fullscreen and preview text controls is retained before confinement and stays separate from the private decoder channel; only pipe/regular-file controls are accepted. Resource limits also constrain coded dimensions and individual allocations. GPU rendering ioctls still add kernel/driver attack surface and do not come with a global GPU-memory quota or guaranteed recovery from driver hangs. See [the decoder boundary](docs/SSV2-PROTOTYPE.md). These constraints are not a claim of new battery savings or medical certification.

These measures make a pack's reach small and checkable. They don't make bugs impossible. The renderer depends on Rust libraries for TOML, fonts, images and Wayland, listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). A bug in one of them could still crash the renderer or make it draw the wrong thing, though not reach beyond the renderer's confinement. Please report anything you find.

## Optional trusted information overlay (0.5.0)

The global overlay is off by default. Its clock/date and optional battery, CPU and memory readings are rendered by ScreenSafe after evaluating the pack and before the existing combined-frame flash guard. None of these system readings is inserted into pack expressions, pack settings, thumbnails, files or network messages. The overlay accepts only bounded, finite numeric controls from the service; unknown fields, duplicate keys and out-of-range values are rejected. Notification text, hostnames, window titles, network addresses and arbitrary strings are not accepted.

The service uses the shell's existing UPower module for a battery percentage. CPU/memory polling runs only while selected and a full-screen or live preview renderer is active, at most once every two seconds. The fixed `system-info` command takes no arguments. It opens only `/proc/stat` and `/proc/meminfo` with read-only, no-follow descriptors, applies the final Landlock/seccomp confinement, and then reads at most 4 KiB and 16 KiB through those descriptors. It returns three anonymous numeric aggregates (total/idle CPU counters and memory usage), bounded to 512 bytes in the service with a ten-second process deadline. The service computes CPU percentage from successive samples. Missing or expired readings show as unavailable. Rendering processes receive only selected numeric percentages over their bounded stdin control channel and gain no new filesystem, network or device access.

Only overlay display preferences are saved, in the plugin's own `shell.json` entry under `overlay`; readings are held in memory. Controls cover clock/date, three optional system readings, nine-position placement, size, opacity, colors and slow drift. Existing pack settings and their limits are unchanged.

## Theme/custom color controls (0.5.0)

Color controls use numeric theme IDs or an exact 24-bit RGB encoding; arbitrary strings, non-finite values and out-of-range codes are rejected. The palette's 27 validated names and hex swatches are included once in the existing bounded catalog response. The shared QML picker uses swatches, HSV geometry/sliders and a read-only hex label, with no text/clipboard input or external dialog. Drag changes are coalesced to at most one commit per 150 ms plus the final release; queued stale thumbnails for the same pack/size are replaced. Existing one-thumbnail-per-pack/size storage and native preview/decode limits remain.

Recoloring is prepared at pack load, preserves each colored palette ink's relative luminance and alpha, and leaves backgrounds and black/white fixed. The existing theme image filter uses that palette; Original images are unaffected. This adds no filesystem paths, processes, network access, runtime dependencies or change to the combined-frame flash guard.


## Prepared playback (0.5.0)

Prepared playback is opt-in, with live playback remaining the default and fallback. A trusted Python controller creates a local loop from the same confined Rust pack evaluator/rasterizer. It does not accept pack-provided videos, commands, shaders, approvals or checksums. Live clock dependencies are detected in compiled expressions, including bindings and visibility predicates, and in implicit clock text. Photos and Videos remain live. The overlay also selects normal combined-frame guarding.

Preparation runs only on known external power and is cancellable. Source dimensions, frame count, work time, storage and process outputs are bounded. The producer reports the SHA-256 identity of its actual loaded source snapshot; the controller compares that with the requested plan and checks the source/display again before publishing. Strong source hashing runs during preparation and approval lookups. Ordinary live playback, catalog and thumbnail loads retain their separate lightweight fingerprints.

All native helpers are included for both supported architectures; none is built, downloaded or installed separately at runtime. Omarchy supplies the media/graphics libraries. The trusted Python controllers watch their exact parent's pidfd without polling; parent death requests cleanup and enforces an emergency exit within three seconds if cleanup is stuck. Shell-managed playback also exits on control-pipe EOF, while standalone CLI playback may have a closed stdin. The renderer and namespace workers independently bind their own parent lifetime. Every short service process, including cache status, independently activates its deadline timer.

The separate arbitrary-MP4 importer and raw-frame preparation encoder have a 64 MiB private tmpfs, 2 GiB address-space limit, 180 CPU-second limit, 64-file-descriptor limit, 128 MiB per-file limit and no core dumps. A sealed, fixed seccomp policy is installed before FFmpeg/ffprobe starts. It allows threads but denies new processes, namespace changes, ptrace, cross-process memory access, execveat and io_uring. execve remains allowed for the initial program launch; replacing that same already-confined process cannot add mounts, descendants or CPU budget. No media bytes or author-provided options choose these policies. Neither media cage has a graphics device or compositor connection; the encoder alone receives its bounded output staging directory. GPU playback retains its additional all-thread post-startup exec prohibition.

The encoder sees only fixed-size raw pixels inside a private Bubblewrap namespace. Its H.264 output has fixed timing, no B frames, a single video stream and explicit SDR color handling. For verification and playback, the controller copies the encoded bytes to a sealed memfd (write/grow/shrink/seal seals), hashes that snapshot with SHA-256, and gives the worker a private read-only copy of those exact bytes. A pathname change cannot change the admitted input. Cache/approval directories are user-owned mode 0700 and files mode 0600. Pack data and codec workers cannot write the approval store. The worker applies its all-thread execution restrictions before opening the media, after loading its trusted libraries and GPU context.

The native codec/GPU worker runs in private user, mount, PID and network namespaces with capabilities dropped. The shared [GPU authority](screensafe_gpu.py) retains one PCI render-node identity and eight read-only metadata descriptors. Only those files and the selected render node are mounted; unrelated sysfs, primary/card nodes, home and display/session paths are absent. A fixed bootstrap checks the bound device's inode/device identity before executing the worker. Software and hardware-copy verification share the same retained authority. Playback reacquires the node and checks its stable PCI identity against the approval before any decode; the per-launch inode check remains separate so ordinary device recreation does not itself authorize a different device.

After trusted graphics initialization and before opening media, an all-thread seccomp filter denies new sockets/socketpairs, connect/bind/listen/accept, program execution, new processes other than threads, namespace changes, ptrace, cross-process memory access and io_uring. Existing private packet channels and codec/driver threads remain usable. Native self-tests exercise both existing and newly created threads, socket operations and retained IPC. Worker resource limits are checked before initialization; failure refuses the worker. Unsupported devices or failed policy checks use live rendering. The GPU/codec libraries still add a separate native dependency/driver surface and no global GPU-memory quota is promised; they are never loaded in Quickshell or the Rust presenter.

The Rust presenter retains both existing Landlock layers and `#![forbid(unsafe_code)]`. Only it connects to Wayland. It accepts exactly three distinct, bounded, single-plane, linear XRGB DMA-BUFs over an inherited private `SOCK_SEQPACKET` channel. It checks message size, descriptor count, DMA-BUF filesystem identity, buffer identity, dimensions, format, stride and size before importing. It owns input dismissal, output compatibility, buffer release and timing. Requests permit one frame ahead; a late frame slows playback, with no catch-up burst. Decoder failure, unexpected packets, changed output geometry/cadence, lost approval, or incompatible preferences select normal guarded playback. Compositor backpressure stops further decoding after the bounded look-ahead.

Before approval, the Rust verifier reads every byte of two complete loops from the same GPU framebuffer path used for presentation. BLAKE3 compares full pixel streams for exact repeat and decoder fidelity; artifact/source/authentication identities use SHA-256. The unchanged flash cell measurement/admission policy is tested at a conservative 64 changes per second, including repeat seams and black entry/exit, until its relevant state repeats. A failing frame rejects the artifact; it is never silently skipped during verification. The hardware-copy output must match the software-decoded GPU reference exactly, or a passing software profile is used. Approval binds the media hash, source plan, renderer/worker/controller and graphics/codec code identities, kernel/GPU, geometry, cadence and fixed composition. Updated identities require fresh preparation/approval. The shared GPU policy code and stable PCI location/IDs are part of that identity. The hardened approval format rejects previous-profile records. An empty store can select live playback/empty status before runtime-library hashing; a possible cache hit still requires the complete source/profile/record/media checks.

The offscreen GPU target uses top-left image orientation without MPV's default-window vertical flip. A physical GPU regression checks asymmetric reference colors independently in readback and the actual linear DMA-BUF, for software and hardware-copy decoding. Orientation is part of the approval profile; the corrected worker requires fresh preparation.

Prepared playback omits runtime pixel readback/flash admission. Its approval assumes the approved decoder/presenter reproduces the validated output; a video hash cannot constrain arbitrary fabricated output from a compromised worker or driver. It is an implementation of the existing approximate flash policy, not medical certification. Arbitrary live overlays, tones, device surrounds, post-validation drift, speed changes, seeks and cross-video transitions are outside this profile. User/lock dismissal and profile failure retain the normal renderer path. The plugin still treats every pack as untrusted data.
