---
name: screensafe-packs
description: Make, check and share ScreenSafe screensaver packs for Omarchy. Use when someone asks for a new screensaver, wants to turn art, a video or an idea into one, wants a pack for their theme, or wants to submit a pack for inclusion.
---

# ScreenSafe packs

A ScreenSafe pack is a folder of data: a `screensafe.toml` describing what to draw, written in ScreenSafe's own expression language, plus any images, fonts or text art it uses. A pack can't run code. ScreenSafe's renderer checks it, measures it for flashing, and draws it.

The full format is in the plugin's `docs/PACKS.md` and `docs/EXPRESSIONS.md`. Read both before writing a pack. The built-in packs in `packs/` are worked examples.

## Where things are

```sh
plugin=~/.config/omarchy/plugins/io.github.jeremydixon22.screensafe
screensafe=$plugin/bin/screensafe-$(uname -m)
```

- **Built-in packs:** `$plugin/packs/`. Read them; don't edit them, since updates replace them.
- **The user's own packs:** `~/.config/omarchy/screensafe/packs/<name>/`. Make new packs here. The panel lists them the next time it opens.
- **A theme's pack:** a `screensafe/` folder next to the theme's `colors.toml`. ScreenSafe's **Automatic** choice shows it while that theme is in use.

## Making a pack

1. **Agree what it should look like** before writing anything: the motion, the colors (the theme's palette, or fixed colors for a pack tied to particular art) and how long before it repeats. A pack in the theme's colors should suit light themes as well as dark: `light` and `dark` tell them apart, so it can pick other shades, or keep a night scene dark, where a light theme needs it.
2. **Start small:** one scene, one element. Run `check` after every change.
3. **Work from time alone.** Nothing carries over between frames. Motion is a formula of `time`, `t` or `u`. Use `count` and `samples` to repeat, `hash` for fixed variety and `rand` for variety that changes each time the screensaver opens.
4. **Look at frames, not just the checker:**

   ```sh
   $screensafe check <pack>                                  # rules, cost and flashing
   $screensafe check <pack> --size 2304x1440 --seconds 60   # a large screen
   $screensafe render <pack> --time 12 --size 1920x1080 --out /tmp/frame.png
   $screensafe run --pack <pack> --start 12                  # live; any input closes it
   ```

5. **Keep it calm and cheap.** A screensaver runs for hours on laptops.
   - Aim for `drawMsPerFrame` well under a frame's time (33 ms at 30 fps) at the user's screen size. Lower `fps` for slow motion.
   - Put one `glow` on a group that holds everything that glows, not on each element or on a group with a `count`.
   - Keep image `zoom` and `scale` fixed and move images with `pan_x`/`pan_y`, which costs far less.
6. **Never work around the flash limit.** If `check` reports flashes, slow or soften the change. Don't try to find what the measure misses.
7. **Pick its best moment.** Set `preview` to the time, in seconds, that shows the pack best. The panel's thumbnail draws that frame. Look at that frame with `render` and the same `--time`.
8. **Offer settings only where they matter.** A speed, a density, or a choice of picture can be `[[setting]]`s that people change in the panel (`docs/PACKS.md`, Settings). Keep the defaults as the pack looks best, check each value with `render --settings name=value`, and run `check` at the most demanding ones. A pack that tells a timed story is usually best without any.

### From a video or other existing art

Packs can't play video. Adapt it instead:

- **Stills:** take a few frames as images and move between them slowly. A median of several seconds of frames keeps a scene's background and drops fast-moving detail.
- **Motion:** recreate the movement with lines, meshes and glow.

Only use art the user made or has permission to use, and record where it came from (see below).

## Sharing a pack

- **With a theme:** put the pack folder in the theme repository as `screensafe/`, and add the theme's license and any font licenses.
- **For the community list:** packs stay in their makers' own public repositories. ScreenSafe's catalog, the separate repository `jeremydixon22/omarchy-screensafe-packs`, only lists them, by exact commit, in its `packs.toml` (see `docs/COMMUNITY.md` in the ScreenSafe repository). Its README says whether it's taking listings. To list a pack, put it in a public repository with:
  - the pack folder;
  - a `preview.png`, a frame from `render`;
  - a `LICENSE` (CC0, CC BY 4.0 or MIT).

  Note where every image, font or likeness comes from, including generated material. The listing pull request will ask for it, along with confirmation that the submitter has the right to share the pack.

  The catalog's `CONTRIBUTING.md` has the steps: the entry repeats the pack's `name`, `license` and `description` exactly, and `python3 scripts/catalog.py verify <id>`, run in a clone of the catalog, fetches the pack as people will install it and checks it.

Before opening the pull request:

- Run `check` at the default size and at `--size 2304x1440`, and include the results. Compare `flash.checkedSeconds`
  with the reported loop length: the default covers 600 seconds, or the whole loop if shorter. For longer loops,
  `--seconds` accepts up to 3600; state any portion left unchecked rather than claiming full-loop coverage.
- Make sure the pack holds only allowed files (`.toml`, `.txt`, `.png`, `.jpg`, `.jpeg`, `.webp`, `.ttf`, `.otf`), with plain names, no links, no hidden files and none marked executable.
- Strip images of metadata.

Always show the user the finished pull request text and get their go-ahead before submitting anything on their behalf.
