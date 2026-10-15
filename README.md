<p align="center">
  <img src="docs/gallery/shield.png" width="96" alt="ScreenSafe's shield">
</p>

<h1 align="center">ScreenSafe</h1>

<p align="center">
  <b>Screensavers for <a href="https://omarchy.org">Omarchy</a>, made of data, not code.</b><br>
  ASCII art, line art, glowing curves, neon streets and pixel kingdoms: drawn sharp on every screen, in your theme's colors.
</p>

<p align="center">
  <a href="https://github.com/jeremydixon22/omarchy-screensafe/actions/workflows/ci.yml"><img src="https://github.com/jeremydixon22/omarchy-screensafe/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/version-0.4.8-9ece6a" alt="Version 0.4.8">
  <img src="https://img.shields.io/badge/Omarchy-4-9ece6a" alt="Omarchy 4">
  <img src="https://img.shields.io/badge/license-MIT-9ece6a" alt="MIT license">
</p>

![ScreenSafe's Harmonograph pack, and the ScreenSafe panel with Harmonograph chosen and open](preview.png)

A screensaver in ScreenSafe is a **pack**: a folder with a `screensafe.toml` and its art. A pack describes what to draw and how it moves, with small formulas like `sin(t * 2)` in a language that can't loop, read files or reach the network. ScreenSafe draws the pack itself, and nothing in a pack ever runs as a program, **so a pack from someone else can only describe pictures.**

- **Fifteen packs built in**, from a harmonograph's glowing figures to a rainy Tokyo side street, and your own photos.
- **Your theme, everywhere.** Packs draw in the current theme's palette, and a theme can bring a pack of its own.
- **Sharp at any size.** Drawn on Wayland's overlay layer at your screen's full resolution, on every screen.
- **Settings without code.** Packs can offer a speed, a density or a picture; change them in the panel and watch the pack update live.
- **Made to be safe.** A sandboxed renderer, a checked format, and a flash limit no pack can get past.

## The packs

| | | |
|:---:|:---:|:---:|
| ![Harmonograph](docs/gallery/harmonograph.webp)<br>**Harmonograph**<br><sub>Two pendulums draw a glowing figure that winds inwards, a new one each time round.</sub> | ![Tokyo Drip](docs/gallery/tokyo-drip.webp)<br>**Tokyo Drip**<br><sub>A rainy side street at night: neon signs, lanterns and an LED screen.</sub> | ![Distro Wars](docs/gallery/distro-wars.webp)<br>**Distro Wars**<br><sub>A parody of a popular God-game. The Omarch King's settlements fly Omarchy green, until segfault and Dependency Hell strike.</sub> |
| ![Omnipotence](docs/gallery/omnipotence.webp)<br>**Omnipotence**<br><sub>Figures drawn over the lines of code that draw them, until the Hand of Omnipotence turns the page.</sub> | ![Fibonacci](docs/gallery/fibonacci.webp)<br>**Fibonacci**<br><sub>The sequence in pictures: squares and spirals, a sunflower, rabbits, Pascal's triangle.</sub> | ![Trying To Remember](docs/gallery/trying-to-remember.webp)<br>**Trying To Remember**<br><sub>Thoughts wander through a fog of half-remembered ones, each reaching for an idea just out of reach.</sub> |
| ![Starfield](docs/gallery/starfield.webp)<br>**Starfield**<br><sub>Flying through stars that streak past as they come close.</sub> | ![Cascade](docs/gallery/cascade.webp)<br>**Cascade**<br><sub>Columns of ever-changing digits falling down the screen.</sub> | ![Constellations](docs/gallery/constellations.webp)<br>**Constellations**<br><sub>Drifting stars, joined by faint lines whenever two pass close.</sub> |
| ![Echoes](docs/gallery/echoes.webp)<br>**Echoes**<br><sub>Two shapes bouncing around the screen, trailing echoes of where they've been.</sub> | ![Orbits](docs/gallery/orbits.webp)<br>**Orbits**<br><sub>The Omarchy box turning in its orbits, then a slowly changing Lissajous figure.</sub> | ![Badge](docs/gallery/badge.webp)<br>**Badge**<br><sub>ScreenSafe's shield, the Omarchy mark or your own badge: bouncing, shattering, waving and in orbit, smoothly or as pixel art.</sub> |
| ![Omarchy](docs/gallery/omarchy.webp)<br>**Omarchy**<br><sub>Your screensaver art from <i>Style › Screensaver</i>, revealed a different way each time.</sub> | ![Theme backgrounds](docs/gallery/backgrounds.webp)<br>**Theme backgrounds**<br><sub>Your theme's own backgrounds, one after another, drifting slowly across the screen.</sub> | ![Photos](docs/gallery/photos.webp)<br>**Photos**<br><sub>Your photos from <code>~/Pictures/Screensaver</code>, or sets of examples (nature, technology, aerospace, and astronomy from NASA): full screen, whole, or as prints or Polaroids, in their own tones or the theme's.</sub> |

<sub>Frames from the renderer at each pack's own preview moment (Photos as prints, with its examples), in Tokyo Night, Omarchy's default theme, with its first background and Omarchy's own screensaver art. To show your own badge in Badge, put an image at <code>~/.config/omarchy/screensafe/badge.png</code> (or <code>.webp</code>, <code>.jpg</code> or <code>.jpeg</code>).</sub>

## Install

```sh
omarchy plugin add https://github.com/jeremydixon22/omarchy-screensafe
omarchy plugin enable io.github.jeremydixon22.screensafe
```

ScreenSafe's shield appears in the bar, in your theme's accent. Nothing changes until you switch it on: Omarchy's own screensaver stays in use until then.

ScreenSafe needs Omarchy 4 on an x86-64 or 64-bit ARM computer, and nothing else. It's tested on Omarchy 4.0.4 with Hyprland 0.56 and Quickshell 0.3.1 on x86-64; the ARM build is tested under emulation, drawing identical frames.

## Use

Click the shield in the bar.

| | |
|---|---|
| **Enable** | Show ScreenSafe instead of Omarchy's screensaver when you're idle. Right-clicking the shield does the same. |
| **After … idle** | How long you're idle before it shows. It follows Omarchy's screensaver time until you choose your own with **−** and **+** (or the arrow keys), from 1 minute to an hour; step back below 1 minute to follow Omarchy's again. Point at it to see whose time it is and when Omarchy locks. |
| **Pack** | Which screensaver to show. **Automatic** shows the current theme's pack if it has one, and the Omarchy pack otherwise. Rest on a pack and its row opens with a thumbnail in your theme's colors. Press `f` or click ☆ to make it a favorite, listed first. |
| **⚙ Settings** | For a pack that has them: click ⚙, or press → or `s` on its row. The page shows the pack running live as you change them; turn **Live preview** off for a still thumbnail and the usual Preview. |
| **Preview** | Show the chosen pack now. Any key, click, touch or mouse movement closes it. |
| **About** | Links to this page, to the [catalog of packs others have made](https://github.com/jeremydixon22/omarchy-screensafe-packs), and to guides for making and sharing your own. |

If anything stops ScreenSafe showing, such as Omarchy locking first, a note at the bottom of the panel says so.

<details>
<summary><b>From a terminal or a key binding</b></summary>

```sh
omarchy-shell screensafe preview
omarchy-shell screensafe enable        # or: disable
omarchy-shell screensafe pack auto     # or the path of a pack the panel lists
omarchy-shell screensafe delay 300     # seconds idle before it shows; omarchy to follow Omarchy's
omarchy-shell screensafe favorite PATH # star or unstar a pack the panel lists
omarchy-shell screensafe status
```

</details>

<details>
<summary><b>How it works with Omarchy's own screensaver</b></summary>

While **Enable** is on, ScreenSafe switches Omarchy's screensaver off and opens its own when Omarchy's would have opened. Turning it off, disabling the plugin or removing it switches Omarchy's back on. If you had already switched Omarchy's screensaver off yourself, ScreenSafe leaves it as you set it.

Omarchy stays in charge of everything else:

- **Timing:** unless you choose ScreenSafe's own time, it opens one second after Omarchy's screensaver would have (`idle.screensaver` in `~/.config/omarchy/shell.json`).
- **Locking:** Omarchy still locks on its own schedule (`idle.lock`), and locking closes the screensaver. If ScreenSafe's time is longer than that, Omarchy locks first, and the panel says so.
- **Stay awake** pauses ScreenSafe just as it pauses Omarchy's screensaver.
- **Video and games:** whatever keeps Omarchy's screensaver away keeps ScreenSafe away too. It watches for idle the same way, honoring apps that ask Wayland to keep the screen awake.

If you switch Omarchy's screensaver back on yourself (for example with `omarchy toggle screensaver`), ScreenSafe steps aside and turns itself off the next time it would have opened.

The **Screensaver** entry in Omarchy's menu always opens Omarchy's own screensaver. To open ScreenSafe's, use **Preview** or `omarchy-shell screensafe preview`.

If the shell stops while ScreenSafe holds Omarchy's screensaver off, Omarchy's stays off until ScreenSafe next loads, or until you remove `~/.local/state/omarchy/toggles/screensaver-off`.

</details>

## More packs

- **Find one:** see [the ScreenSafe pack catalog](https://github.com/jeremydixon22/omarchy-screensafe-packs) for community listings and the current submission status.
- **From a theme:** a theme can ship a pack in a `screensafe` folder next to its `colors.toml`. Omarchy keeps that folder when it installs the theme, since it's only data, and **Automatic** shows it while that theme is in use.
- **Your own:** put a pack folder in `~/.config/omarchy/screensafe/packs/`, and it appears in the panel the next time you open it.

## Make your own

Start with [Making a screensaver](docs/MAKING.md); the format is in [Making packs](docs/PACKS.md) and [Expressions](docs/EXPRESSIONS.md). The built-in packs in [`packs/`](packs) are worked examples.

**With a coding agent:** [`skills/screensafe-packs`](skills/screensafe-packs/SKILL.md) is an agent skill that walks your agent through making, checking and sharing a pack. Link it into your agent's skills folder, such as `~/.claude/skills/`, and describe the screensaver you want.

The renderer checks a pack line by line, measures its cost and its flashing, and renders any frame:

```sh
screensafe=~/.config/omarchy/plugins/io.github.jeremydixon22.screensafe/bin/screensafe-$(uname -m)
$screensafe check path/to/pack
$screensafe render path/to/pack --time 12 --out /tmp/frame.png
```

To share it, see [Community packs](docs/COMMUNITY.md).

## Safety

- **Packs are data.** The format has no way to run a program, open a file outside its own folder, reach the network or loop without end. Everything a pack asks for is checked and compiled before it's shown, and every per-frame cost is bounded.
- **The renderer confines itself.** Before reading a pack it can only read the packs, theme, art and photos folder it was given; after loading it, it gives up new file access. Photo files opened while loading remain readable. It can never reach the network or start programs. This uses Linux's Landlock, and the renderer refuses to read a pack if the kernel can't confine it.
- **Flashing is limited.** The renderer measures every frame and won't show more than three large-area flashes a second, the WCAG 2.2 limit for photosensitivity, whatever a pack asks for.
- **Stalled frames are stopped.** If drawing a frame stalls for about five seconds, the renderer exits and ScreenSafe falls back to the built-in pack.
- **The binaries are checkable.** Both renderers are built reproducibly from [`renderer/`](renderer), and CI attests each one; see [how to rebuild and compare them](docs/BINARY-PROVENANCE.md).

[SECURITY.md](SECURITY.md) has the details and how to report a problem; [Notes for reviewers](docs/REVIEW-NOTES.md) lists everything ScreenSafe runs and writes.

## Remove

```sh
omarchy plugin remove io.github.jeremydixon22.screensafe
```

Removing the plugin switches Omarchy's screensaver back on if ScreenSafe had switched it off. ScreenSafe keeps a small record in `~/.local/state/screensafe/` while it holds Omarchy's screensaver off; you can delete that folder afterwards. Your own packs in `~/.config/omarchy/screensafe/` are left in place.

## What's inside

- `Service.qml`, `Panel.qml` and `ShieldMark.qml`: the plugin. They start only ScreenSafe's own renderer and Omarchy's `omarchy-shell` (to ask whether the session is locked).
- `bin/screensafe-x86_64` and `bin/screensafe-aarch64`: the renderer, static Linux executables built reproducibly from [`renderer/`](renderer). The plugin runs the one for your machine.
- `packs/`: the built-in packs, and `tools/`: the generators for the larger ones.
- `skills/screensafe-packs/`: instructions for coding agents that make packs. The plugin doesn't install or read them.

The renderer embeds [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) (SIL Open Font License 1.1) and uses the Rust libraries listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). It needs nothing else at run time.

The expression language borrows ideas and a few function names from MilkDrop's preset equations, but no code; [Expressions](docs/EXPRESSIONS.md#compared-with-milkdrop) describes how they differ.

## License

MIT; see [LICENSE](LICENSE).
