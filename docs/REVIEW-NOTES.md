# Notes for reviewers

A summary of what ScreenSafe does on a user's machine, for marketplace and other reviewers. [SECURITY.md](../SECURITY.md) has the full account.

## Expected baseline result

The 0.4.8 release's baseline was `review-required`, with no findings and one capability: `bundled-executable-binary`. The 0.5.0 candidate adds six native helper executables to the two static Rust renderers, so a fresh exact-commit scan and human review are required. All eight binaries are included and have rebuild/attestation recipes; see [BINARY-PROVENANCE.md](BINARY-PROVENANCE.md) and [native packaging](NATIVE-PACKAGE.md). Installation never downloads dependencies, builds source or runs privileged setup. Omarchy supplies the existing media/graphics libraries.

To reproduce the result, `tests/marketplace-baseline.mjs` runs the marketplace's own `runSecurityBaseline` on a commit of this repository, serving the files from the local Git store:

```sh
node tests/marketplace-baseline.mjs --marketplace=path/to/omarchy-plugin-marketplace
```

## What runs

For this update, distinguish three paths: ordinary guarded Rust rendering (including `.ssv` Videos), explicit MP4 import, and opt-in prepared procedural playback. The MP4 importer uses Omarchy's FFmpeg/ffprobe through the bundled resource-limit launcher and bundled Bubblewrap. Prepared playback uses the trusted controller, Waylandless GPU worker and Rust-only Wayland presenter. Both Python controllers follow parent pidfds; shell-managed playback also ends on control EOF. No component receives author-supplied commands, native shaders or approvals.

| Program | Started by | When | Arguments | Environment | Ends |
|---|---|---|---|---|---|
| `bin/screensafe-$ARCH run` | Service | At the idle timeout, if Enable is on; or a preview | the pack's folder, the theme's folder, the user's screensaver art, Omarchy's logo, the user's badge (a fixed path, `~/.config/omarchy/screensafe/badge`, without its extension), the user's photos folder (a fixed path, `~/Pictures/Screensaver`), and the marker `--role org.omarchy.screensaver` | `HOME`, `XDG_RUNTIME_DIR`, `WAYLAND_DISPLAY`, optional `XDG_STATE_HOME` and `TZ` | On any input, or when the session locks, or the plugin unloads; or when a frame takes more than 5 s to draw, when the built-in pack is shown instead |
| `bin/screensafe-$ARCH preview` | Service | While a pack's settings page is open with Live preview on, and its preview box is wholly in view | the pack's folder (one the catalog lists), the box as whole logical pixels, the panel's screen name (letters, digits, `.`, `_`, `-`), the same theme, art, badge, photos and videos paths, and the pack's settings; on standard input, only `settings NAME=VALUE,...` (when a setting changes), `region X,Y,W,H` (when the box moves), bounded numeric `overlay`/`stats` controls and `quit`, a line each, at most 1 KiB | as for `run` | When the page closes, the panel closes, the box leaves view, Live preview is turned off, or the plugin unloads; its parent's exit ends it too, as does a frame taking more than 5 s. Started only once a page has been open for 350 ms; one that fails isn't started again until a setting changes or the page is left |
| `bin/screensafe-$ARCH catalog` | Service | When the panel opens, the theme changes, or settings first load | three pack folders, and the same theme, art, badge, photos and videos paths | as above | 10 s deadline |
| `bin/screensafe-$ARCH thumbnail` | Service | When a card enters the visible area or its settings page opens, and when a setting changes, one at a time | the pack's folder, a size of at most 480 x 480, and the same theme, art, badge, photos and videos paths | as above | 10 s deadline |
| `bin/screensafe-$ARCH takeover claim/release/status` | Service | When Enable changes, at load, at each idle start, and on unload (`execDetached`) | an instance token | as above | 10 s deadline |
| `bin/screensafe-$ARCH photos-folder` | Service | When someone chooses **Your photos** on a photo pack's settings page | none | as above | 10 s deadline. Makes `~/Pictures/Screensaver` if it isn't there, without following links, and prints its path; the Service then opens exactly that folder with `Qt.openUrlExternally` |
| `bin/screensafe-$ARCH videos-folder` | Service | When someone chooses **Your videos** | none | as above | Same bounded folder-opening behavior as `photos-folder`, at the fixed `~/Videos/Screensaver` path |
| `bin/screensafe-$ARCH system-info` | Service | At most once every 2 s, only with CPU/memory readings selected and an overlay renderer active | none | as above | 10 s deadline; at most 512 bytes of anonymous numeric aggregates, no arbitrary paths |
| `$OMARCHY_PATH/bin/omarchy-shell lock isLocked` | Service | Before showing, and every 5 s while showing | fixed | as above, plus `OMARCHY_PATH` and `PATH=$OMARCHY_PATH/bin:/usr/bin:/bin` | 10 s deadline; the screensaver shows only on a plain `false` |

All are started by absolute path with `clearEnvironment: true`. None receives private data. Every short call has its own deadline, and all are stopped in `Component.onDestruction`.

Every process's output is read as it arrives and limited in size, with room to spare over the most it can legitimately print: a thumbnail 1 MiB (it's at most 922 KB), the catalog 128 KiB (the renderer stops it at 64 KiB), takeover and status 16 KiB, the lock check 4 KiB, and the screensaver's own output 64 KiB of errors and 4 KiB else. A short call that goes over is stopped, and what it said is dropped. The screensaver's own output is cut off instead, and it keeps running.

**Photos.** A pack with `source = "photos"` shows the image files directly in `~/Pictures/Screensaver`. The renderer adds that folder to its read-only confinement, lists it, and opens each PNG, JPEG or WebP file in it (any name that isn't hidden, beneath the folder without following links, at most 64 MiB, at most 200 at once) before it confines itself fully; it reads each only when it's shown, through the descriptor it already holds, and decodes it with the same limits as any image. Its fingerprint covers the photos' names, sizes and times changed, not their contents. Nothing in the folder is written.

**The clock.** Packs can show the local time. The renderer reads the time zone once, before it confines itself: `/etc/localtime`, or the zone `TZ` names (a plain name, checked) under `/usr/share/zoneinfo`; the Service passes `TZ` only if the shell has it set. `check`, `render` and thumbnails use a fixed moment.

The badge is the only image a person supplies directly. The renderer tries just `badge.png`, `.webp`, `.jpg` and `.jpeg` at that path, adds only those four files to its read-only confinement, reads at most 8 MiB through the same no-links, regular-files-only reader as packs, and decodes it with the same limits on dimensions and allocation as a pack's images, inside the sandbox. The shell never decodes it: the panel sees only the plain-pixel thumbnail.

The only pack path the renderer is ever given is one the catalog lists. A chosen pack in `shell.json` that the catalog doesn't list (moved, removed, or edited by hand) is shown as Automatic would be, and the panel says so. When a pack's thumbnail finds it changed, the catalog is read again at most once every 5 seconds.

The live preview is a small overlay surface laid over the settings page's preview box: it takes no keyboard focus and has an empty input region, so the panel beneath keeps every click and key. It reads its pack again when a setting changes, so it stays confined at the first layer (reading only its pack's folder and the theme, art and badge files it was given; no writing, running programs, network, abstract sockets or signals) rather than going on to the second, which prevents new file access while retained photo descriptors remain readable.

The renderer adds its own guards, in case the shell stops answering or dies without cleaning up:

- it asks Linux to end it when the process that started it exits (`PR_SET_PDEATHSIG`), so neither a screensaver nor a pending call outlives the shell. `takeover` is the exception, because its release runs detached as the plugin unloads;
- `catalog`, `thumbnail` and `takeover` exit on their own after 20 seconds, even when blocked writing to a pipe nobody reads;
- `run` and `preview` exit, as for a pack that can't be used, if a frame is still being drawn after 5 seconds: input is read between frames, so a pack too heavy to draw would otherwise leave the screen covered and deaf to keys;
- `preview` reads at most 8 KiB of standard input between frames, drops lines over 1 KiB, and loads only the last of several `settings` lines read at once;
- memory kept between frames is capped: glyphs at 64 MiB, and glows for the first 32 in a frame.

**Output.**

- **The listing:** `catalog` limits itself to 64 KiB, and the Service keeps at most 64 entries.
- **Thumbnails:** `thumbnail` prints two lines. The first is the pack's fingerprint: a 64-bit FNV-1a digest of its manifest and files, the theme's colors and backgrounds, the art it shows, and the renderer's version. The second is the frame as plain pixels, `pixels W H` and three bytes a pixel in base64, or `unchanged` when the Service passes the fingerprint of the thumbnail it already has (`--unless`). In that case the renderer stops before drawing, a check of a few milliseconds. The renderer loads the pack, confines itself and only then draws. The Service accepts only output of that shape, exactly the size it asked for and three bytes a pixel, and the panel adds its own fixed, uncompressed PPM header before displaying the pixels with Qt's asynchronous `Image` item. Padding before the header's fixed maximum value aligns it to whole base64 groups, so the RGB payload follows unchanged. No header, compression, URL or file path from the renderer reaches the image loader. The Service keeps thumbnails in memory only, never on disk: at most one per listed pack and screen shape, dropped when a pack leaves the list.
- **The other calls:** `takeover` prints one short line or a small JSON object, and `run` prints at most a few lines.

**The renderer's own limits.** It reads only beneath the pack's folder (`openat2` with `RESOLVE_BENEATH` and no links, or a component-by-component walk refusing links where `openat2` is unavailable), plus the named theme backgrounds, art and photos, with size limits. It confines itself with Landlock in two layers. Before it parses anything from a pack, in every command that reads packs, it can only read beneath the paths it was given, with no writing, running, TCP, abstract sockets or signals. Once loaded, it cannot open new files; photo files already opened while loading remain readable. The live `preview` stays at the first layer (see above). `bin/screensafe-$ARCH sandbox-test` demonstrates this. `$ARCH` is the kernel's architecture, read from `/proc/sys/kernel/arch`; on any other architecture the plugin runs nothing and says so.

## What changes on disk

- **Omarchy's screensaver switch.** `~/.local/state/omarchy/toggles/screensaver-off` is created only after the user turns on **Enable**, and only if it doesn't already exist. It is removed only if ScreenSafe created it, and only by the instance that holds the claim. If the user removes it themselves, ScreenSafe turns its own option off rather than recreate it.
- **The photos folder.** `~/Pictures/Screensaver` is made, mode 0755, only when someone chooses **Your photos** on a photo pack's settings page and it isn't there.
- **ScreenSafe's own state.** `~/.local/state/screensafe/` holds a claim marker and a lock file: folder mode 0700, files 0600, created without following links.
- **Settings.** The plugin's own `shell.json` entry, through `shell.updateEntryInline`: `onIdle`, `pack`, `idleSeconds`, `favorites` (at most 64 pack paths), `settings` (values chosen for packs' settings, by pack path) `livePreview` (whether settings pages show a live preview) and `overlay` (validated global display preferences); anything else in it is ignored. A setting's value is saved only for a pack the catalog lists, only for a setting that pack declares, and only as a value its declaration allows; the renderer checks each again, giving the default for anything else. Values equal to the default aren't kept, and values for packs no longer listed are dropped as settings are saved. Omarchy's `idle` settings are read, never written: ScreenSafe's own idle time works by opening ScreenSafe on its own idle timer while it holds Omarchy's screensaver off.

## How it fits with Omarchy's idle service

Omarchy's idle service keeps the lock timer and stay-awake.

- **Showing:** while **Enable** is on, Omarchy's own screensaver launch is switched off, and ScreenSafe opens one second after it would have, or after the user's own idle time, timed by its own `IdleMonitor` (respecting idle inhibitors).
- **Locking:** the renderer's command line carries `org.omarchy.screensaver`, so Omarchy's lock command (`pkill -f '[o]rg.omarchy.screensaver'`) closes it. The Service also closes it if it finds the session locked.
- **Dismissing:** input to the renderer's surface counts as activity for both idle monitors, so dismissing it before the lock deadline cancels the lock, as with Omarchy's screensaver.

## QML

- **No text fields:** there is no `TextInput`, `TextField` or `TextEdit`, so the plugin never reads the clipboard.
- **Plain text:** every string from a pack is rendered with `Text.PlainText`. Every other `Text` shows a fixed label or icon.
- **Media folders:** **Your photos** and **Your videos** open `file://` plus the fixed paths `~/Pictures/Screensaver` and `~/Videos/Screensaver`, respectively, only after the renderer has printed the exact expected path. These are the only folder-opening `Qt.openUrlExternally` calls.
- **Links:** About opens four fixed pages in the browser with `Qt.openUrlExternally`: three of this repository, and the front page of its catalog of shared packs, `jeremydixon22/omarchy-screensafe-packs`. Each URL is fixed in `Panel.qml`, and `openLink` refuses anything that isn't one of those two repositories exactly, or a page beneath one. No URL comes from a pack, and the plugin fetches nothing from the catalog.
- **Work on the shell's thread:** there is no JavaScript loop converting every thumbnail pixel. The service checks the exact requested size (352 pixels wide, at most 352 tall) and RGB payload length. The panel constructs a short fixed-format header; Qt loads the bounded raw image asynchronously, with image caching disabled. Thumbnails are requested one at a time, and one is kept per pack and size. The live preview's box is checked every 300 ms while a page is open; its process is restarted only when the page shows another pack. Nothing foreign (clipboard, window titles, other applications' encoded images) reaches this path.
- **Images:** the only images the plugin shows are its thumbnails: checked raw RGB from the renderer with a plugin-owned header (see Thumbnails above). Pack-provided image headers and compressed data stay in the renderer; nothing reaches the network. Qt chooses the reader from the image's first bytes, which are always the panel's header, so Qt's binary PPM reader is the only image reader that runs in the shell, and a pack influences only the pixel values. That reader runs in the shell's process, outside the renderer's confinement. The bar and panel mark (`ShieldMark.qml`) is drawn with Qt Quick Shapes in the theme's accent, with a § cut through it as a hole, from the shell's own font.
- **Descriptions:** a pack's description shows beside its thumbnail in its always-expanded card, as plain text. Clicking an entry scrolls its full card into view; hovering does not scroll.
- **Reopening after a preview:** the panel closes to show a preview started from it, and opens again when that preview is dismissed, so packs can be tried one after another. It doesn't reopen when the preview fails to start or fails, when the session locks during it (the renderer is then stopped by the lock watch), for a screensaver started by idle, or for a preview started by IPC or Omarchy's menu. It never stays open behind the screensaver, which keeps keyboard input for the screensaver alone.
- **Settings pages:** the back row and preview stay fixed above independently scrolling controls, with the first active Color control pinned below them. Its compact custom view puts the color square beside the HSV sliders; a short display scrolls only that control region if needed. Overlay similarly pins its Position grid below its scrolling options. Photos/Videos end with Tone, followed by Color when Theme is selected. A pack's settings are changed with toggles, choice buttons, steppers and bounded theme/custom color controls. There are no text fields: no value is ever typed. Declarations come from the catalog's output, checked again in `Service.qml` (names of lowercase letters, digits and `_`; labels up to 40 characters; 2 to 6 choices; at most 8 settings) before the panel shows them.
- **The cursor:** opening the panel puts its cursor where it last was, or else on the chosen pack, or else at the top, already showing. That's state in memory only, never written anywhere.

## Other files

- **`skills/screensafe-packs/SKILL.md`:** instructions for a user's coding agent on making and sharing packs. It's documentation only: the plugin never installs, reads or runs it, and nothing is written to any agent's folders.

## Optional trusted information overlay (0.5.0)

The global overlay is off by default. Its clock/date and optional battery, CPU and memory readings are rendered by ScreenSafe after evaluating the pack and before the existing combined-frame flash guard. None of these system readings is inserted into pack expressions, pack settings, thumbnails, files or network messages. The overlay accepts only bounded, finite numeric controls from the service; unknown fields, duplicate keys and out-of-range values are rejected. Notification text, hostnames, window titles, network addresses and arbitrary strings are not accepted.

The service uses the shell's existing UPower module for a battery percentage. CPU/memory polling runs only while selected and a full-screen or live preview renderer is active, at most once every two seconds. The fixed `system-info` command takes no arguments. It opens only `/proc/stat` and `/proc/meminfo` with read-only, no-follow descriptors, applies the final Landlock/seccomp confinement, and then reads at most 4 KiB and 16 KiB through those descriptors. It returns three anonymous numeric aggregates (total/idle CPU counters and memory usage), bounded to 512 bytes in the service with a ten-second process deadline. The service computes CPU percentage from successive samples. Missing or expired readings show as unavailable. Rendering processes receive only selected numeric percentages over their bounded stdin control channel and gain no new filesystem, network or device access.

Only overlay display preferences are saved, in the plugin's own `shell.json` entry under `overlay`; readings are held in memory. Controls cover clock/date, three optional system readings, nine-position placement, size, opacity, colors and slow drift. Existing pack settings and their limits are unchanged.

## Videos (0.5.0)

The fixed videos folder holds prepared `.ssv` files. Video is normalized by a separate, isolated importer; the shell never invokes a video decoder on a contributed pack or plays arbitrary URLs. The retained-descriptor reader, decode/cache limits, device art, CRT sampling and unchanged combined-frame flash guard are described in [VIDEO-PROTOTYPE.md](VIDEO-PROTOTYPE.md). Videos metadata and its fixed folder join the existing photo inputs; neither introduces system readings into pack expressions.

## Theme/custom color controls (0.5.0)

Color controls use numeric theme IDs or an exact 24-bit RGB encoding; arbitrary strings, non-finite values and out-of-range codes are rejected. The palette's 27 validated names and hex swatches are included once in the existing bounded catalog response. The shared QML picker uses swatches, HSV geometry/sliders and a read-only hex label, with no text/clipboard input or external dialog. Drag changes are coalesced to at most one commit per 150 ms plus the final release; queued stale thumbnails for the same pack/size are replaced. Existing one-thumbnail-per-pack/size storage and native preview/decode limits remain.

Recoloring is prepared at pack load, preserves each colored palette ink's relative luminance and alpha, and leaves backgrounds and black/white fixed. The existing theme image filter uses that palette; Original images are unaffected. This adds no filesystem paths, processes, network access, runtime dependencies or change to the combined-frame flash guard.

## Optional prepared playback (0.5.0 development)

The former Overlay tab is now **Settings**, with **Overlay** and **Performance** sub-tabs. Prepared playback is off by default, defaults to use on battery only when enabled, and never prepares automatically. The user starts preparation on known external power; it is cancellable. Storage choices are 256 MB, 512 MB (default), 1 GB and 2 GB, with oldest-used loops evicted and a clear button. Preferences stay in the plugin's `shell.json` entry under `performance`.

When used, the service invokes the trusted `cache/manager.py` through absolute `/usr/bin/python3` paths with a cleared environment plus the validated Hyprland instance signature. The controller calls fixed `hyprctl`, `pacman` and `ldd` commands for the display and local execution profile. It writes only its private prepared-loop store under `~/.cache/omarchy/screensafe/prepared`. Status has a ten-second deadline; preparation has a twenty-minute outer deadline and bounded per-stage work. Service output is bounded to 2 KiB stdout and 8 KiB stderr for preparation, 2 KiB each for status. Unload stops the jobs.

Preparation uses the confined Rust evaluator to supply bounded raw pixels to a namespace-isolated FFmpeg encoder. A separate, Waylandless libmpv/GBM worker verifies the exact encoded snapshot over two loops; the Rust flash checker consumes the final GPU pixels directly. Playback uses that worker and the confined Rust renderer's three-buffer DMA-BUF presenter, with private fixed-size IPC. Only the Rust presenter connects to the compositor. Source, settings, theme, display, binaries, decoder/driver dependencies and kernel identify a local approval; encoded bytes must match its SHA-256. The snapshot is sealed against modification before decode. The codec worker has no home, network or compositor socket and applies its execution restrictions to all threads before media parsing. See [SECURITY.md](../SECURITY.md) for the profile, timing rules, fallback and decoder-fidelity assumptions.

Live playback remains the default and fallback, including clock-dependent packs, Photos, Videos, overlays, unsupported displays, missing approvals and worker failures. Live previews continue through the ordinary guarded renderer. Both architecture helper sets are included; users do not compile or install a native component separately. Native ARM/x86 CI checks the worker API/seccomp and the controller/importer/settings tests, while actual GPU fidelity/eligibility is verified locally during preparation. A passing CI run is not a claim of universal GPU support.

## 0.4.8 to 0.5.0 review focus

- Compiler/color/panel changes keep pack admission costs and data-only authoring. Generated expression branches only go forward. No contributed native code or shell command path was added.
- Overlay reads only optional anonymous CPU/memory/battery aggregates outside pack expressions and remains before the runtime combined-frame flash guard.
- `.ssv` Videos uses retained descriptors, bounded indexes and confined JPEG decoding. The final composed picture, including surrounds/tone/overlay, still passes the runtime flash guard.
- The external-media import cage now has a 64 MiB tmpfs, 2 GiB address-space limit, 180 CPU-second limit, 64 descriptors, bounded file writes and a fixed pre-exec seccomp policy restricting process creation to threads. The initial execve is required to enter FFmpeg; replacing that same confined process does not expand its authority or resource budget. No home, network access or graphics device is exposed.
- **Prepared playback changes the safety model.** It omits runtime pixel readback/admission and instead requires final-GPU-output approval of the exact sealed media and playback profile, including loop/timing/entry/exit checks. It assumes the fixed decoder/presenter/driver reproduces the approved pixels; a media hash cannot constrain a compromised worker's fabricated output. This requires explicit review, separately from ordinary Videos.
- The Python controller cannot silently outlive its shell/terminal: pidfd lifetime propagation and managed EOF are regression-tested, including a helper stuck during shutdown. The cache-status job independently activates its ten-second service deadline.
- Native build inputs and upstream source are pinned and checksum-verified. CI rebuilds all helper bytes and covers the new Python/JavaScript/C paths; provenance subjects include all eight executables and the native manifest. Use evidence from the exact candidate SHA, not the 0.4.8 approval or earlier local experiments.
