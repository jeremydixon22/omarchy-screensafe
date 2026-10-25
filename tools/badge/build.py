"""Builds the Badge pack: ScreenSafe's shield or the Omarchy mark, or someone's own badge, put through its paces.

    python3 build.py PACKS_DIR

Writes PACKS_DIR/badge. Its five scenes are all made from the one picture cut into an 8 x 8 grid of tiles: a slow
bounce around the screen, a burst apart and a snap back together, a wave through it as through a flag, tiles flying in
and raining away, and copies in orbit. Two settings choose the picture (the shield or the Omarchy mark) and the look:
drawn smoothly from a 512-pixel sheet, or as pixel art in the theme's colors, from a 64-pixel sheet the renderer
redraws in the theme's palette. Anyone can show their own badge instead by putting an image at
~/.config/omarchy/screensafe/badge.png (or .webp, .jpg, .jpeg): the renderer fits it to the sheet, keeping its shape.

The shield is drawn here to the same geometry as the panel's mark (ShieldMark.qml): three points along its top,
coming to a point below, in Omarchy's default accent, its left half a shade lighter, with a section sign cut through
it, set in the renderer's own JetBrains Mono. The Omarchy mark is Omarchy's own, from its icon.txt. Needs Python 3 and
Pillow.
"""
import math
import os
import sys
from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
FONT = os.path.join(HERE, "..", "..", "renderer", "fonts", "JetBrainsMono-Regular.ttf")
ACCENT = (0x7A, 0xA2, 0xF7)
LIGHTER = (0x95, 0xB5, 0xF9)
GRID = 8                       # tiles a side
SIZE = 0.56                    # the badge's height on the stage
DUR = 32.0


def num(v):
    out = f"{v:.5f}".rstrip("0").rstrip(".")
    return "0" if out in ("", "-0") else out


def q(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def fmt(x):
    if isinstance(x, bool):
        return "true" if x else "false"
    if isinstance(x, (list, tuple)):
        return "[" + ", ".join(fmt(y) for y in x) + "]"
    return num(x) if isinstance(x, (int, float)) else q(x)


def element(fields, comment=None):
    lines = [f"# {comment}"] if comment else []
    lines.append("[[scene.draw]]")
    lines += [f"{k} = {fmt(v)}" for k, v in fields.items()]
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------------------------------------------
# The shield.


def shield(side):
    """The shield on a transparent square `side` pixels across, drawn four times larger and shrunk, for smooth edges."""
    big = side * 4
    hgt = big * 0.94
    wid = hgt * 14 / 16
    ox, oy = (big - wid) / 2, (big - hgt) / 2
    at = lambda u, v: (ox + u * wid, oy + v * hgt)

    def curve(p0, c, p1, steps=48):
        return [at(*[(1 - t) ** 2 * a + 2 * (1 - t) * t * b + t ** 2 * d for a, b, d in zip(p0, c, p1)])
                for t in (k / steps for k in range(1, steps + 1))]

    left_curve = curve((0.5, 1), (0, 0.8), (0, 0.46))
    whole = [at(0, 0.02), at(0.25, 0.15), at(0.5, 0), at(0.75, 0.15), at(1, 0.02), at(1, 0.46)]
    whole += curve((1, 0.46), (1, 0.8), (0.5, 1)) + left_curve
    left = [at(0, 0.02), at(0.25, 0.15), at(0.5, 0), at(0.5, 1)] + left_curve

    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.polygon(whole, fill=ACCENT + (255,))
    d.polygon(left, fill=LIGHTER + (255,))
    # The section sign, cut out: centered across by its ink, a little below the middle, about 0.44 of the height.
    font = ImageFont.truetype(FONT, round(hgt * 0.6))
    hole = Image.new("L", (big, big), 0)
    hd = ImageDraw.Draw(hole)
    stroke = max(1, round(hgt * 0.012))
    x0, y0, x1, y1 = hd.textbbox((0, 0), "§", font=font, stroke_width=stroke)
    scale = hgt * 0.44 / (y1 - y0)
    font = ImageFont.truetype(FONT, max(1, round(hgt * 0.6 * scale)))
    stroke = max(1, round(stroke * scale))
    x0, y0, x1, y1 = hd.textbbox((0, 0), "§", font=font, stroke_width=stroke)
    cx, cy = at(0.5, 0.54)
    hd.text((cx - (x0 + x1) / 2, cy - (y0 + y1) / 2), "§", font=font, fill=255, stroke_width=stroke, stroke_fill=255)
    # Then moved so its ink, as drawn, is centered exactly: a font's reported box can sit off its ink.
    ix0, iy0, ix1, iy1 = hole.getbbox()
    dx, dy = round(cx - (ix0 + ix1) / 2), round(cy - (iy0 + iy1) / 2)
    hole = Image.new("L", (big, big), 0)
    ImageDraw.Draw(hole).text((cx - (x0 + x1) / 2 + dx, cy - (y0 + y1) / 2 + dy), "§", font=font, fill=255,
                              stroke_width=stroke, stroke_fill=255)
    alpha = img.getchannel("A")
    alpha.paste(0, mask=hole)
    img.putalpha(alpha)
    return img.resize((side, side), Image.LANCZOS)


# ---------------------------------------------------------------------------------------------------------------
# The Omarchy mark: Omarchy's own, from its icon.txt, a block to each of its strokes.

OMARCHY = [
    "#############",
    "#.....#.....#",
    "#.#####..##.#",
    "#.#.......#.#",
    "#.#.......#.#",
    "#.#.......#.#",
    "###.......#.#",
    "#.#.......#.#",
    "#.#.......#.#",
    "#.#.......#.#",
    "#.#########.#",
    "#.....#.....#",
    "#######.#####",
]
MARK_COLOR = (0xC0, 0xCA, 0xF5)


def omarchy(side):
    """The Omarchy mark, centered on a transparent square `side` pixels across, each of its blocks a whole number of
    pixels so it stays crisp."""
    cell = int(side * 0.9) // len(OMARCHY)
    size = cell * len(OMARCHY)
    img = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    o = (side - size) // 2
    for y, row in enumerate(OMARCHY):
        for x, ch in enumerate(row):
            if ch == "#":
                d.rectangle((o + x * cell, o + y * cell, o + (x + 1) * cell - 1, o + (y + 1) * cell - 1), fill=MARK_COLOR + (255,))
    return img


# ---------------------------------------------------------------------------------------------------------------
# The scenes. Each is drawn four ways, for the two pictures and the two looks, each shown only when the settings
# choose it: `sheet` is the sheet's side in pixels, and `crisp` draws it as pixel art in the theme's colors.

PICTURES = ["shield", "omarchy"]


def scene_bodies(sheet, crisp, picture):
    tile = sheet // GRID
    common = {"kind": "sprite", "file": f"{picture}{'-pixel' if crisp else ''}.png", "source": "badge", "smooth": not crisp}
    if crisp:
        common["palette"] = "theme"
    whole = dict(common, frame_size=[sheet, sheet], anchor=[sheet / 2, sheet / 2])
    tiles = dict(common, frame_size=[tile, tile], anchor=[tile / 2, tile / 2])
    step = SIZE / GRID
    home = [f"col = mod(i, {GRID})", f"row = floor(i / {GRID})",
            f"hx = (col - {num((GRID - 1) / 2)}) * {num(step)}", f"hy = (row - {num((GRID - 1) / 2)}) * {num(step)}"]
    out = {}

    # 1. A slow bounce from edge to edge, never quite repeating, as old screensavers did.
    d = SIZE * 0.62
    out["Bounce"] = ("The badge, drifting from edge to edge and back.", [dict(
        whole, x=f"(tri(time / 23 + 0.2) - 0.5) * (aspect - {num(d * 0.9)})", y=f"(tri(time / 17 + 0.6) - 0.5) * {num(1 - d)}",
        pixel=num(d / sheet))])

    # 2. Whole, then a burst: the tiles fly out and drift about the middle as a turning cloud, then snap back into
    # place, overshooting a little, and the badge is whole again, with a small pulse as it settles. Every offset is
    # scaled by how far apart the tiles are, so the badge is exactly whole at the start and the end.
    out["Shatter"] = ("Whole, bursting apart, drifting, and snapping back together.", [dict(
        tiles, count=GRID * GRID, frame="i",
        let=home + ["burst = ease_out(clamp((t - 3) / 4, 0, 1))",
                    "snap = clamp((t - 19) / 5, 0, 1)",
                    "back = 1 + 2.70158 * (snap - 1) ^ 3 + 1.70158 * sqr(snap - 1)",
                    "out = t < 19 ? burst : 1 - back",
                    "spin = 0.8 * sin(pi * clamp((t - 3) / 21, 0, 1))",
                    "ang = atan2(hy, hx) + out * (hash(i * 3.1) - 0.5) * 1.2 + spin",
                    "dist = sqrt(sqr(hx) + sqr(hy)) * (1 + out * (0.6 + hash(i * 7.3) * 0.9))",
                    "settle = 1 + 0.05 * max(0, 1 - abs(t - 24.2) * 2)"],
        x="dist * cos(ang) * settle", y="dist * sin(ang) * settle",
        pixel=f"{num(step / tile * 1.03)} * (1 - 0.3 * clamp(out, 0, 1) * hash(i * 5.7)) * settle")])

    # 3. A wave through the badge, as through a flag: cut into narrow upright strips, each rising and falling a
    # little after the one before, so the waves run smoothly across it and no gaps open.
    strips = 32
    out["Wave"] = ("A wave through the badge, as through a flag.", [dict(
        common, frame_size=[sheet // strips, sheet], anchor=[sheet // strips / 2, sheet / 2], count=strips, frame="i",
        let=["calm = smoothstep(1, 4, t) * (1 - smoothstep(28, 31, t))"],
        x=f"(i - {num((strips - 1) / 2)}) * {num(SIZE / strips)}", y=f"0.04 * sin(i * {num(6 / strips)} - t * 2.1) * calm",
        pixel=f"{num(SIZE / sheet * 1.03)}")])

    # 4. The tiles fly in from scattered places, one after another, then rain away.
    out["Assemble"] = ("Tiles flying in, one after another, then raining away.", [dict(
        tiles, count=GRID * GRID, frame="i",
        let=home + ["arrive = ease_out(clamp((t - 1 - hash(i * 2.9) * 8) / 2.5, 0, 1))",
                    "fall = ease_in(clamp((t - 22 - hash(i * 4.4) * 6) / 2.2, 0, 1))",
                    "sx = (hash(i * 6.1) * 2 - 1) * aspect * 0.6", "sy = (hash(i * 8.3) * 2 - 1) * 0.6"],
        x="mix(sx, hx, arrive)", y="mix(sy, hy, arrive) + fall * 1.2",
        visible="arrive > 0 && fall < 1",
        pixel=f"{num(step / tile * 1.03)}")])

    # 5. Six small copies in orbit about a large one, passing behind it on the far side (drawn first) and in front
    # of it on the near side (drawn last).
    small = SIZE * 0.22
    copies = lambda side: dict(
        whole, count=6, let=["a = i / n * tau + t * 0.35"], visible=f"sin(a) {side} 0",
        x="0.62 * cos(a)", y="0.3 * sin(a)", pixel=f"{num(small / sheet)} * (0.85 + 0.15 * sin(a))")
    out["Orbit"] = ("Six small copies in orbit about a large one.",
                    [copies("<"), dict(whole, pixel=num(SIZE * 0.75 / sheet), y="0.02 * sin(t * 0.7)"), copies(">=")])
    return out


SETTINGS = (
    '[[setting]]\nname = "logo"\nlabel = "Picture"\nkind = "choice"\nchoices = ["ScreenSafe", "Omarchy"]\n'
    'values = [0, 1]\ndefault = 0\n\n'
    '[[setting]]\nname = "pixelart"\nlabel = "Pixel art in theme colors"\nkind = "toggle"\ndefault = false\n\n'
    '[[setting]]\nname = "tint_color"\nlabel = "Theme color"\nkind = "color"\ndefault = 0\nwhen = "pixelart"\nwhen_value = 1\n'
)


def build(packs):
    folder = os.path.join(packs, "badge")
    os.makedirs(folder, exist_ok=True)
    shield(512).save(os.path.join(folder, "shield.png"), optimize=True)
    shield(64).save(os.path.join(folder, "shield-pixel.png"), optimize=True)
    omarchy(512).save(os.path.join(folder, "omarchy.png"), optimize=True)
    omarchy(64).save(os.path.join(folder, "omarchy-pixel.png"), optimize=True)
    variants = [(sheet, crisp, k, picture) for k, picture in enumerate(PICTURES) for sheet, crisp in ((512, False), (64, True))]
    bodies = [(f"logo == {k} && {'pixelart' if crisp else '!pixelart'}", scene_bodies(sheet, crisp, picture))
              for sheet, crisp, k, picture in variants]
    scenes = []
    for name in bodies[0][1]:
        comment = bodies[0][1][name][0]
        elements = []
        for cond, body in bodies:
            for fields in body[name][1]:
                f = dict(fields)
                f["visible"] = f"({cond}) && ({f['visible']})" if "visible" in f else cond
                elements.append(element(f))
        scenes.append(f'[[scene]]\nname = "{name}"\nduration = {num(DUR)}\nfade = 1.5\n\n# {comment}\n' + "\n".join(elements))
    description = ("ScreenSafe's shield or the Omarchy mark, or your own badge, bouncing, shattering, waving and in orbit. To show "
                   "yours, put an image at ~/.config/omarchy/screensafe/badge.png.")
    head = (f"# Badge: {description} Written by tools/badge/build.py.\n"
            f'format = 2\nname = "Badge"\nauthor = "ScreenSafe"\nlicense = "MIT"\ndescription = {q(description)}\n'
            f'colorize = "tint_color"\n'
            f"preview = {num(DUR * 4 + 10)}\nfps = 30\nfade = 0\n\n" + SETTINGS)
    with open(os.path.join(folder, "screensafe.toml"), "w") as f:
        f.write(head + "\n" + "\n".join(scenes))


if __name__ == "__main__":
    build(sys.argv[1])
