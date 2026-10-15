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

`no_new_privs` is set first, and core dumps are turned off. The renderer reports how fully the kernel enforced this: `fully confined`, or `partly confined (older kernel)` when the kernel enforces only the kinds of access it knows, which always include reading files. On a kernel without Landlock, or if confining fails, the renderer refuses to read any pack, and the panel says the screensaver couldn't be shown. The settings page's live preview (`preview`) stays at the first layer: it reads its pack again whenever a setting changes, so it keeps read-only access to that pack's folder and the theme, art and badge files it was given, and nothing else. To see the confinement work on your machine, run:

```sh
bin/screensafe-$(uname -m) sandbox-test
```

It applies the first layer to its own folder, checks that it can read there but can't read elsewhere or write, then applies the second, tries to read a file, list a folder, start a program and open a network connection, and reports that each was refused.

## What the plugin does

- **Programs:** it starts only two, each by absolute path, with a cleared environment holding just `HOME`, `XDG_RUNTIME_DIR`, `WAYLAND_DISPLAY` and, if set, `XDG_STATE_HOME` and `TZ`:
  - ScreenSafe's own renderer;
  - Omarchy's `omarchy-shell lock isLocked`, which also gets `OMARCHY_PATH` and a `PATH` of Omarchy's `bin` folder, `/usr/bin` and `/bin`.

  Pack paths are the only arguments that vary. Short calls are canceled after 10 seconds, and give up on their own after 20 even if the shell has stopped answering. The renderer runs until it's dismissed, the session locks, or the plugin unloads, and never outlives the shell that started it. A frame still being drawn after 5 seconds ends it, and the plugin shows the built-in pack instead, so no pack can leave the screen covered and unresponsive.
- **Output:** the renderer bounds its listing to 64 KiB, and a thumbnail is at most 922 KB; its other output is a few lines. The plugin reads at most 64 listed packs, and stops any short call that prints more than it can legitimately print.
- **No text fields:** the panel has none, so nothing reads the clipboard, and everything from a pack is shown as plain text.
- **Files it changes:**
  - Omarchy's `~/.local/state/omarchy/toggles/screensaver-off`: created only while you have **Enable** on, and only if it wasn't already there; removed only if ScreenSafe created it.
  - Its own record of that, in `~/.local/state/screensafe/` (folder mode 0700, files 0600, written without following links).
  - `~/Pictures/Screensaver`, made only when you choose **Your photos** on a photo pack's settings page, if it isn't there (mode 0755, without following links). That folder is then opened in your file manager, the way About opens its links in your browser.
  - Its settings (whether it's on, the pack, its own idle time, the favorite packs, the values chosen for packs' settings, and whether settings pages show a live preview) in its own entry in `~/.config/omarchy/shell.json`, through Omarchy's plugin interface. It reads Omarchy's idle and lock times there but never changes them.
- **No network.** No downloads, installers, package managers or services, and nothing needs elevated privileges.

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

These measures make a pack's reach small and checkable. They don't make bugs impossible. The renderer depends on Rust libraries for TOML, fonts, images and Wayland, listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). A bug in one of them could still crash the renderer or make it draw the wrong thing, though not reach beyond the renderer's confinement. Please report anything you find.
