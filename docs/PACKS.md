# Making packs

A pack is a folder holding a `screensafe.toml` and the files it draws: plain-text art (`.txt`) and images (`.png`, `.jpg`, `.jpeg` or `.webp`). Everything that moves is described with [expressions](EXPRESSIONS.md), small formulas of time.

```text
my-pack/
  screensafe.toml
  logo.txt
  photo.webp
```

Check a pack while you work on it. The renderer reports the first problem with where it is, and for a pack that loads, how heavy each scene is and whether any of it flashes too much:

```sh
screensafe=~/.config/omarchy/plugins/io.github.jeremydixon22.screensafe/bin/screensafe-$(uname -m)
$screensafe check my-pack
$screensafe check my-pack --theme ~/.local/state/omarchy/current/theme --size 2304x1440
$screensafe render my-pack --time 12.5 --size 1920x1080 --out frame.png
$screensafe run --pack my-pack --start 12.5     # show it from 12.5 s in; any input closes it
```

## A first pack

```toml
format = 1
name = "Hello"
author = "you"
license = "CC0-1.0"

[[scene]]
duration = 6
hold = 4

[[scene.draw]]
kind = "text"
text = "HELLO, OMARCHY"
size = 0.08
align = "center"
valign = "middle"
reveal = "t * 8"                    # eight characters a second
color = "mix(accent, foreground, u)"
```

A pack plays its scenes in order and then starts again. Each scene runs for `duration` seconds and then holds for `hold` more, fading in and out at its ends.

## The stage

Everything is placed on the **stage**, which is one unit tall with (0, 0) at its center. x grows to the right and y grows downwards, so the top edge is at y = −0.5 and the bottom at 0.5. With the default `stage = "16:9"` the stage is a 16:9 frame fitted inside the screen, running from x = −0.889 to 0.889. Its width-to-height ratio is the name `aspect`. With `stage = "screen"` the stage is the whole screen, and `aspect` is the screen's own ratio.

Sizes and line widths are in the same units, so `size = 0.05` is a twentieth of the stage's height on any screen.

## The file

### Pack settings

| Setting | Default | Meaning |
|---|---|---|
| `format` | required | `1`, or `2` for a pack that uses what ScreenSafe 0.5 added: videos, `color` settings, `colorize`. An older ScreenSafe then refuses the pack with a clear message instead of failing on a field it doesn't know. |
| `name` | required | Up to 60 characters. |
| `author`, `license` | empty | Up to 60 and 40 characters. |
| `description` | empty | Up to 300 characters. |
| `preview` | none | The moment, in seconds, that best shows the pack, for its thumbnail in the panel. Without it, the thumbnail shows the pack 8 seconds in, or halfway through a shorter pack. |
| `stage` | `"16:9"` | `"screen"`, or a ratio between 1:4 and 4:1 such as `"4:3"`. |
| `pixels` | none | For pixel art: draw on a canvas of chunky pixels, this many (64 to 1080) to a stage unit; see [Pixel art](#pixel-art). |
| `fps` | `30` | Frames a second, 1 to 60, for scenes that don't set their own. |
| `fade` | `0.5` | Seconds of fade at each end of a scene, 0 to 5, for scenes that don't set their own. |
| `background` | `"background"` | Color expression for scenes that don't set their own. |

**`[params]`** names numbers for every expression to use, such as `spin = 0.9`.

**`[colors]`** names colors made from the palette and params, such as `glow = "mix(accent, white, 0.3)"`. A pack color can't use another pack color, or the time.

**`[fonts]`** names fonts in the pack's folder for text to use, such as `card = "fonts/card.ttf"`; see [`text`](#text).

Names are up to 40 lowercase letters, digits and `_`, starting with a letter or `_`, and may not reuse a name that already exists.

### `[[scene]]`

| Setting | Default | Meaning |
|---|---|---|
| `name` | empty | Shown in `check` reports. |
| `duration` | `10` | Seconds the scene's action takes, 0.1 to 3600. `u` runs from 0 to 1 over it. |
| `hold` | `0` | Seconds to hold afterwards, 0 to 3600. `t` keeps counting; `u` stays at 1. |

`duration` and `hold` can also be quoted expressions of the pack's params and settings, worked out once as the pack loads: `duration = "each"`, with a setting choosing how long each picture shows.
| `fade` | pack `fade` | Seconds of fade in and out, at most half the scene. |
| `fps` | pack `fps` | Frames a second, 1 to 60. |
| `background` | pack `background` | Color expression. Over a scene's last `fade` seconds it turns into the next scene's. |
| `let` | none | Named values worked out each frame, in order: `let = ["a = t * spin", "b = sin(a)"]`. |
| `[[scene.draw]]` | none | What the scene draws, first to last, so later elements are drawn over earlier ones. |

### Every element

Each `[[scene.draw]]` has a `kind` and these settings. All except `kind` and `count` take expressions.

| Setting | Default | Meaning |
|---|---|---|
| `kind` | required | `group`, `text`, `lines`, `mesh`, `rect`, `image`, `ascii` or `sprite`. |
| `count` | `1` | Draw this many copies, 1 to 1024, each with its own `i`. |
| `let` | none | Named values for this element (and a group's children), worked out for each copy. |
| `visible` | always | A true/false expression; a copy is skipped when false. |
| `opacity` | `1` | 0 to 1, multiplied by any group's. |
| `x`, `y` | `0` | Where the element's origin goes. |
| `rotate` | `0` | Radians, clockwise. Text and ASCII art move with their group but don't turn. |
| `scale` | `1` | Multiplies sizes and distances, from −100 to 100. |
| `color` | `"foreground"` | Color expression. |
| `glow` | none | A soft light around what the element draws, as a lit phosphor or a bright line has: its brightness, from 0 (none) to 4. |
| `glow_radius` | `0.006` | How far the glow spreads, in stage units, from 0.0005 to 0.05 (a number, not an expression). |

A glow on a group lights everything in it together, which costs far less than giving each element its own glow: each glow is a blur worked out every frame it changes. While what's inside a glow stays exactly the same from one frame to the next, the glow and its contents are laid down again as they were, at a fraction of the cost, so a figure that has finished drawing costs little to keep on screen. A glow inside a glowing group adds nothing more.

### `group`

Draws its own `[[scene.draw.draw]]` elements (nested up to 4 deep), moved, turned, scaled and faded together. `clip = ["left", "top", "right", "bottom"]` hides anything outside that rectangle, given in the group's own coordinates.

With `sort = true`, a group draws its children's copies from the least `depth` to the greatest instead of in order, so that, in a scene seen from above, what stands nearer is drawn over what stands behind it. `depth` is a number expression on each child, `0` if it has none; copies with the same depth keep their order.

A child without a `count` of its own shares its group's `i`, `n` and `k`. A child with its own `count` numbers its own copies, so give the group's number a name first (`let = ["row = i"]`) to use it there.

### `text`

| Setting | Default | Meaning |
|---|---|---|
| `text` | empty | Up to 2000 characters; `\n` starts a new line. `{expression}` shows a number (see below). |
| `size` | `0.04` | Font size. |
| `align` | `"left"` | `left`, `center` or `right` of the origin. |
| `valign` | `"top"` | `top`, `middle` or `bottom` of the origin. |
| `line_height` | `1.25` | Line spacing as a multiple of `size`, 0.5 to 4. |
| `reveal` | everything | How many characters show, counting a line break as one, for typing effects. |
| `cursor` | false | A true/false expression: when true, a block cursor shows after the last revealed character. |
| `font` | built in | One of the pack's `[fonts]`, by name. |
| `stretch` | `1` | Draws the characters this much wider or narrower, 0.25 to 4 (a number): `0.7` for a tall, condensed face. |

Numbers in text are rounded to whole numbers unless given a format after `;`: `{x; 3}` pads to 3 characters with spaces, `{x; 03}` with zeros, and `{x; .2}` shows two decimals (up to 6). Write `{{` and `}}` for literal braces.

Five words in braces show the clock's names rather than a number: `{zone}`, the time zone's abbreviation (`CEST`); `{zone_name}`, its name (`Europe/Berlin`); `{offset}`, its offset from UTC (`+02:00`); `{weekday_name}` (`Thursday`) and `{month_name}` (`January`). So a clock is `text = "{hour; 02}:{minute; 02} {zone}"`. See [the clock](#the-clock).

Text uses JetBrains Mono, bundled with ScreenSafe, unless it names one of the pack's own fonts. A pack can bring up to 4, as TrueType or OpenType files of up to 4 MiB each, named in a `[fonts]` table:

```toml
[fonts]
card = "fonts/DejaVuSansMono-Bold.ttf"
```

Text is laid out as a terminal lays it out, every character as wide as an M, so a monospaced font suits it best. Include the font's license with the pack.

### `lines`

Either fixed points, `points = [[x, y], ...]` (2 to 4096), or `samples = m` (2 to 4096) with `px` and `py` expressions. In `px` and `py`, `s` runs from 0 to 1 along the line, `j` counts the samples from 0, and `m` is their number. Defaults: `px = "s"`, `py = "0"`.

Sampled lines can also name values worked out for each sample, before `px` and `py`, with `sample_let`, written as `let` is. They can use `s` and `j`, so a curve can be worked out once and then bent, folded or projected, without writing it out in every expression that uses it:

```toml
sample_let = ["cx = 0.3 * cos(s * tau)", "cy = 0.3 * sin(s * tau)", "lift = max(0, cx - 0.1)"]
px = "cx - lift * 0.5"
py = "cy - lift * 0.8"
```

Each costs its operations once for every sample.

| Setting | Default | Meaning |
|---|---|---|
| `width` | `0.003` | Line width, up to 0.05. 0 draws no line. |
| `closed` | `false` | Join the last point to the first. |
| `draw` | `1` | How much of the line to draw, 0 to 1, for lines that draw themselves in. |
| `fill` | none | A color to fill the shape with, once it's completely drawn. |

### `mesh`

A wireframe in three dimensions: `vertices = [[x, y, z], ...]` (2 to 4096) and `edges = [[a, b], ...]` (1 to 8192), where `a` and `b` number the vertices from 0. As on the stage, y grows downwards; z grows away from you.

| Setting | Default | Meaning |
|---|---|---|
| `ry`, `rx`, `rz` | `0` | Radians to turn about the vertical axis, then tilt, then roll. |
| `offset_x`, `offset_y`, `offset_z` | `0` | Where to move the turned mesh, in mesh units, before it's seen in perspective: a camera moving past it, or one part placed beside another. |
| `size` | `0.25` | Stage units per mesh unit at depth 0. |
| `distance` | `4` | The camera's distance, in mesh units, for perspective; 0 or less draws without perspective. |
| `width` | `0.003` | Line width. |
| `draw` | `1` | How many of the edges to draw, 0 to 1, in the order listed. |
| `edge_color` | `color` | A color for each edge. `e` is its number, and `ex`, `ey` and `ez` the midpoint of its two vertices, before turning. An edge given a fully transparent color isn't drawn. |

### `rect`

A filled rectangle `w` by `h` (default 0.1 by 0.1), centered on its origin, in `color`.

### `image`

`file = "photo.webp"` (PNG, JPEG or WebP, up to 32 MiB and 64 megapixels); `files = ["a.webp", "b.webp", ...]`, up to 64, which `index` chooses among; `source = "theme-backgrounds"` for the current theme's background images; or `source = "photos"` for the viewer's own photos (see [Photos](#photos)). Photos taken with the camera turned are shown upright.

| Setting | Default | Meaning |
|---|---|---|
| `fit` | `"cover"` | `cover` fills the screen, cropping; `contain` shows all of it. |
| `zoom` | `1` | 0.1 to 10. |
| `pan_x`, `pan_y` | `0` | −1 to 1: which part shows when the image is larger than the screen. |
| `index` | `0` | Which of `files`, the theme's backgrounds or the photos, counting from 0 and going round. |
| `w`, `h` | none | A box this many stage units wide and tall, centered on the element's origin, to fit the image to instead of the whole screen (numbers, both or neither). `cover` cuts the image to the box's shape; `contain` shows all of it within the box. |
| `filter` | `"none"` | `none`, `greyscale`, `sepia`, or `theme`: shadows to highlights in the theme's background, accent and foreground colors. Or the name of a setting or param holding 0 to 3, for one of these in that order. Applied once as the image is decoded, so it costs nothing to draw. |

Images are fitted to the whole screen, not the stage. `x` and `y` move them from there, and `rotate` and `scale` turn and scale them about the screen's center.

An image that isn't turned and whose size holds still is drawn fast: ScreenSafe resamples it once to that size and copies it into place, so panning it costs little. Animating `zoom` or `scale`, or turning an image, filters every pixel in every frame, which costs many times more on a large screen. The first frames of a scene may show it without the image, while a large image is decoded.

### Photos

`source = "photos"` shows the viewer's own photos: the PNG, JPEG and WebP files directly in `~/Pictures/Screensaver`, sorted by name, up to 64 MiB each. With more than 200 there, a different selection of 200 is shown each time the screensaver opens. `photos` says how many there are, so a pack can show something else, such as its own examples, when there are none. A pack that shows photos gets a **Your photos** row on its settings page, which opens the folder.

The renderer opens the photos before it confines itself, and reads each only when it's shown.

### `sprite`

A frame of pixel art from a sheet: `file = "sheet.png"` (PNG, JPEG or WebP, at most 4096 pixels a side), cut into frames of `frame_size` pixels, numbered across each row and then down.

| Setting | Default | Meaning |
|---|---|---|
| `frame_size` | the whole sheet | `[width, height]` in pixels, whole numbers. |
| `frame` | `0` | Which frame shows, counting from 0 and going round. |
| `anchor` | `[0, 0]` | The point of the frame, in its pixels from its top left, that goes at the element's `x` and `y`. A mirrored frame keeps its anchor on the same part of the picture. |
| `flip` | false | A true/false expression: when true, the frame is mirrored left to right. |
| `pixel` | `1 / pixels`, or `0.004` | The size of one of the sprite's pixels in stage units. |
| `smooth` | `false` | Draw the frame smoothly, at any size and position, instead of in crisp blocks on whole pixels: for painted art that grows, shrinks or glides, rather than pixel art. |
| `source` | none | `"badge"`: show the viewer's own badge instead of `file`, if they have one (see below). |
| `palette` | none | `"theme"`: redraw the sheet in the theme's colors, as pixel art (see below). |

Sprites are drawn with hard edges, each pixel a block, and their edges fall on whole screen pixels so they stay crisp as they move, unless they're `smooth`. Sprites placed edge to edge, such as strips or tiles of one picture, meet without gaps. They don't turn, and `color` doesn't apply to them. A pack's sprite sheets may hold 16 megapixels altogether.

**A viewer's own badge.** With `source = "badge"`, the sheet is the viewer's own image, if they've put one at `~/.config/omarchy/screensafe/badge.png` (or `.webp`, `.jpg` or `.jpeg`, tried in that order, up to 8 MiB). It's fitted to the size of the sprite's `file`, keeping its shape, centered on transparency, so the pack's `frame_size` and the rest work for any badge. Without one, or if it can't be read, `file` is shown. The Badge packs cut it into tiles this way: `frame_size` a fraction of the sheet, and each copy showing its own `frame`.

**The theme's palette.** With `palette = "theme"`, every pixel of the sheet is redrawn in the nearest of the theme's colors, with an ordered dither where it falls between them, and each pixel is either shown fully or not at all. On a small sheet drawn large, the result is pixel art in the theme's colors, whatever the picture: the shield, someone's badge, or a pack's own art.

### `ascii`

Text art: `art = "logo.txt"` (UTF-8, up to 64 KiB, 200 rows and 400 columns), or `source = "branding"` for your Omarchy screensaver art. Characters sit in terminal-shaped cells; block characters (U+2580 to U+259F) and Braille patterns (U+2800 to U+28FF) fill their cells exactly, as a terminal draws them.

| Setting | Default | Meaning |
|---|---|---|
| `effect` | `"random"` | How the art appears: `print`, `decrypt`, `rain`, `wipe`, `fade`, or `random` for a different one each time round. |
| `progress` | `"u"` | How far the effect has got, 0 to 1. |
| `fit` | `0.8` | The largest share of the stage's width or height the art may take, 0.05 to 1. |
| `colors` | `["foreground"]` | 1 to 8 colors, blended from the top row to the bottom. |

## Settings

A pack can offer up to 8 settings that someone can change in the panel, behind the ⚙ on its row. Each is a `[[setting]]`, and becomes a name the pack's expressions use, as a param does, holding the value chosen:

```toml
[[setting]]
name = "speed"            # the name expressions use
label = "Speed"           # what the panel calls it, up to 40 characters
kind = "choice"
choices = ["Slow", "Normal", "Fast"]   # 2 to 6 labels of up to 16 characters
values = [0.5, 1, 2]                   # a number for each
default = 1

[[setting]]
name = "glowing"
label = "Glow"
kind = "toggle"            # true or false
default = true

[[setting]]
name = "count"
label = "Stars"
kind = "range"             # a number in steps
min = 100
max = 800
step = 100
unit = ""                  # shown after the value, up to 8 characters
default = 400
```

A value someone chose is always one the pack allows: a choice that isn't offered, or a number outside a range, gives the default, and a range's value is snapped to its steps. Settings are numbers and true/false values, worked out as the pack loads, so `count` and the like can't depend on them. Instead, give an element its largest `count` and hide the rest: `visible = "i < count"`. A setting can also choose a scene's `duration` or `hold`. Offer settings only where they change something people will care about. A pack that tells a timed story is usually best without any.

ScreenSafe 0.5 reads pack formats 1 and 2. Existing packs can keep format 1; video sources, `color` settings and `colorize` require `format = 2` at the root. Older renderers reject format 2 clearly instead of ignoring a new feature.

A `color` setting offers the current theme's swatches and a custom HSV picker. For whole-palette recoloring, set `colorize` at the root, before any tables, to that setting's name:

```toml
format = 2
colorize = "tint_color"

[[setting]]
name = "tint_color"
label = "Color"
kind = "color"
default = 0
```

Zero preserves the original palette. Values 1–27 select the theme palette names in their documented renderer order; `16777216 + 0xRRGGBB` selects a 24-bit custom RGB value. Other numbers, fractions and non-finite values give the default. The expression variable holds that numeric selection. Recoloring happens once at load: colored theme inks take the chosen hue with their original linear-light luminance, while canvas backgrounds and absolute black/white stay unchanged. This preserves light/dark relationships even for very dark or bright choices. Theme image filters and theme-quantized sprites use the same palette; Original/Greyscale/Sepia images and fixed-color artwork do not change.

The panel pins the first active color control while other options scroll. A color control can depend on an earlier setting, for example `when = "tone"` and `when_value = 3`. It is shown and applied only when that earlier setting has that value. This is fixed data, not an expression executed by the shell. Color settings count toward the existing limit of eight and require ScreenSafe 0.5.0.

To try a pack's settings without the panel: `screensafe render <pack> --time 12 --settings speed=2,glowing=false --out frame.png`.

## Pixel art

With `pixels = 224`, a pack is drawn the way a game console draws: on a canvas of large pixels, 224 of them to a stage unit, scaled up to the screen by the largest whole number that still shows a whole stage unit, each canvas pixel a solid block. The canvas fills the screen, so on a wide screen it's wider than it is tall; `aspect` is its width in stage units. Sprites drawn on it at their default `pixel` size are one canvas pixel to a sprite pixel.

```toml
pixels = 224
[[scene]]
[[scene.draw]]
kind = "sprite"
file = "walker.png"
frame_size = [10, 16]
anchor = [5, 15]
frame = "floor(t * 6) % 4"
```

## The clock

The clock names give the local time, in the viewer's time zone, daylight saving included: a screensaver running across a change shows the right time on both sides of it. A hand turns with `clock`, as `rotate = "clock / 43200 * tau"` for an hour hand; a clock for another zone at a fixed offset uses `utc_clock`, as `mod(utc_clock / 3600 + 9, 24)` for the hour in Tokyo.

`check`, `render` and the panel's thumbnails show a fixed moment instead, so their results never depend on when they were made: 10:08:30 UTC on Thursday 15 January 2026.

## Names you can use

In every expression:

- **Time:** `time`, seconds since the screensaver opened; `cycle`, how many times the pack has played through.
- **The clock:** `hour` (0 to 23), `minute`, `second`, `day` (1 to 31), `month` (1 to 12), `year` and `weekday` (0 for Monday to 6 for Sunday), in local time; `clock`, the seconds since local midnight, with their fraction, for smooth hands; `utc_offset`, the local offset from UTC in hours (`5.5` for India, `-4` for New York in summer); and `utc_clock`, the seconds since midnight in UTC, for a clock in another zone. See [the clock](#the-clock).
- **Photos:** `photos`, how many of the viewer's photos there are to show (see [Photos](#photos)).
- **Shape:** `aspect`, the stage's width over its height; `screen_aspect`, the screen's.
- **Constants:** `pi` and `tau`.
- **Colors:** the theme's, as named in its `colors.toml`:
  - `background`, `foreground`, `accent`, `muted`, `selection`;
  - `red`, `orange`, `yellow`, `green`, `cyan`, `blue`, `magenta`, `brown`;
  - `bright_foreground`, `bright_red`, `bright_yellow`, `bright_green`, `bright_cyan`, `bright_blue`, `bright_magenta`;
  - `dark_background`, `darker_background`, `lighter_background`, `dark_foreground`, `light_foreground`;
  - and `black` and `white`.

  A color the theme doesn't set takes a related one it does (`bright_red` takes `red`), or else Omarchy's default.
- **Light or dark:** `light` is true when the theme is a light one, and `dark` when it isn't; each is always the other's opposite. They follow the theme's `mode` in its `colors.toml`, or, for a theme without one, whether its background is mostly light. A pack can choose what suits either: `color = "light ? mix(foreground, black, 0.3) : foreground"`, or a whole night scene that stays dark, its colors drawn from a light theme's palette, as Tokyo Drip does.
- **Your own:** the pack's `[params]` and `[colors]`.

In a scene:

- `t`: seconds since the scene began.
- `u`: `t / dur`, held at 1 once the action is over.
- `dur`: the scene's `duration`.
- The scene's `let` names.

In an element:

- `i`: which copy this is, from 0.
- `n`: the `count`.
- `k`: `i / (n - 1)`, from 0 to 1 (0 when there's one copy).
- The element's and any enclosing group's `let` names.

## Limits

ScreenSafe refuses a pack that goes beyond these, rather than drawing it slowly:

- **Files:**
  - at most 96 MiB of files altogether;
  - a `screensafe.toml` of up to 1 MiB;
  - files in the pack's own folder, up to four folders deep, with plain names (letters, digits, `.`, `_` and `-`, not starting with `.`), and no links.
- **Structure:** at most 64 scenes; 256 elements in a scene; 64 `let` names in a list.
- **Cost per frame** in each scene:
  - 1,500,000 expression operations;
  - 250,000 points, edges and characters drawn, after multiplying out `count` and `samples`. `check` reports both figures for each scene.
- **Expressions:** up to 2048 characters, 512 parts, nesting 48 deep.
- **Flashing:** `check` fails a pack with more than three large-area flashes in any second; see [SECURITY.md](../SECURITY.md#flashing). While showing, the renderer holds the previous frame rather than show one that would go over.

## Sharing a pack with a theme

Put the pack in a `screensafe` folder in the theme's repository, next to `colors.toml`. Omarchy copies it with the rest of the theme, and ScreenSafe's **Automatic** choice shows it while the theme is in use. A theme's pack can use `source = "theme-backgrounds"` to show the theme's own images, and its colors follow the theme's palette.

Video packs also have `video_pixel`: one output pixel in stage units, set per display, including compositor scaling. It is geometry only. The global clock/system overlay is controlled by the user outside packs; CPU, battery and memory are not expression inputs.

Video packs also receive `video_hd_mask`, an exact numeric bitset over their at-most-32 prepared clips. Bit `i` is 1 when clip `i` has at least 720 vertical pixels, otherwise 0; `mod(floor(video_hd_mask / pow(2, i)), 2)` reads it. This describes the prepared playback dimensions, including any importer downscale, and exposes no filenames or new I/O.
