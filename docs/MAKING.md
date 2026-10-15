# Making a screensaver

A ScreenSafe screensaver is a **pack**: a folder holding a `screensafe.toml`, which describes what to draw and how it
moves, and any images, fonts or text art it uses. A pack is data, never a program, so the renderer checks it before
showing it, and anyone can share one safely.

Make yours in `~/.config/omarchy/screensafe/packs/<name>/`. It appears in the panel the next time you open it.

## Three ways to start

**With a coding agent.** `skills/screensafe-packs` is an agent skill that walks an agent through making, checking
and sharing a pack. Link it into your agent's skills folder, then describe the screensaver you want:

```sh
ln -s ~/.config/omarchy/plugins/io.github.jeremydixon22.screensafe/skills/screensafe-packs ~/.claude/skills/screensafe-packs
```

**By hand.** Copy a built-in pack from the plugin's `packs/` folder and change it. [Making packs](PACKS.md)
covers the format, and [Expressions](EXPRESSIONS.md) covers the small formulas that move things.

**For a theme.** Put the pack in a `screensafe` folder next to the theme's `colors.toml`. It shows while the theme is
in use, in the theme's colors.

A pack can also offer settings, such as a speed or how many stars, that people change in the panel. See [Settings](PACKS.md#settings).

## Trying it

```sh
screensafe=~/.config/omarchy/plugins/io.github.jeremydixon22.screensafe/bin/screensafe-$(uname -m)
$screensafe check <pack>                                      # every rule, its cost, and flashing
$screensafe render <pack> --time 12 --out /tmp/frame.png      # one frame
$screensafe run --pack <pack>                                 # live; any input closes it
```

The panel shows your changes too. Each time a pack's row opens, its thumbnail is checked against the pack's files,
and drawn again if they've changed. When they have, the panel reads the list of packs again, but at most once every
5 seconds, so a change saved within 5 seconds of the last shows up when the 5 seconds are up.

## From other screensavers

Windows `.scr` files, macOS `.saver` bundles and XScreenSaver's hacks are programs, so there's nothing in them to
convert. A pack can recreate how one looks, though: many classics are a few shapes moving to a formula. Stills or a
short loop of frames can stand in for a video. Recreate only what you have the right to: your own work, or work
whose license allows it.

## Sharing it

See [Community packs](COMMUNITY.md).
