"""Builds the Tokyo Drip pack: a rainy Tokyo side street at night, in the theme's colors.

    python3 build.py OUT_DIR [FONT_COLLECTION]

Neon signs jut from the buildings on both sides, lanterns glow, an LED screen plays at the end of the street, and
the rain drips from every sign onto the wet street, which reflects it all. Everything is placed in one-point
perspective, worked out here, and written as data and simple expressions. Omarchy's default theme is Tokyo Night,
so by default that's the palette; any other theme recolors the street.

The signs' lettering is a subset of Noto Sans Mono CJK JP Bold (SIL Open Font License 1.1), cut down to the
characters used and renamed Tokyo Drip Sign, as the license asks of modified versions. FONT_COLLECTION is the
NotoSansCJK-Bold.ttc it comes from (default: Debian's and Ubuntu's fonts-noto-cjk path); without it, the existing
subset in OUT_DIR/fonts is kept.
"""
import math
import os
import string
import sys

X0, Y0 = -8 / 9, -0.5
VPY = -0.04            # the horizon, where the street vanishes
F = 0.8                # focal length: a point (X, Y, z) appears at (X F / z, VPY + Y F / z)
GROUND = 0.5           # the street, this far below the eye
WALL = 1.15            # the buildings' faces, either side
END = 9.0              # the building across the end of the street
NEAR = 0.92            # the nearest the walls come

FONT_SOURCE = "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc"
FONT_FACE = "Noto Sans Mono CJK JP Bold"


def num(v):
    out = f"{v:.5f}".rstrip("0").rstrip(".")
    return "0" if out in ("", "-0") else out


def q(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def fmt(x):
    if isinstance(x, bool):
        return "true" if x else "false"
    if isinstance(x, (list, tuple)):
        return "[" + ", ".join(fmt(y) for y in x) + "]"
    return num(x) if isinstance(x, (int, float)) else q(x)


def element(fields, comment=None, depth=0):
    lines = [f"# {comment}"] if comment else []
    lines.append("[[scene" + ".draw" * (depth + 1) + "]]")
    lines += [f"{k} = {fmt(v)}" for k, v in fields.items()]
    return "\n".join(lines) + "\n"


def P(X, Y, z):
    return [X * F / z, VPY + Y * F / z]


def quad(points, fill, comment=None, depth=0, extra=None):
    f = {"kind": "lines", "points": points, "closed": True, "width": 0, "fill": fill}
    f.update(extra or {})
    return element(f, comment, depth)


def pick(values):
    return values[0] if len(values) == 1 else "pick(i, " + ", ".join(num(v) for v in values) + ")"


def pick_color(values):
    return values[0] if len(values) == 1 else "pick(i, " + ", ".join(values) + ")"


# ---------------------------------------------------------------------------------------------------------------
# What's on the street.

# Vertical signs jutting from the walls, their faces toward us: (side, depth, height of top, width, text, color).
# Words a side street shows: izakaya, ramen, karaoke, yakitori, coffee house; sushi, open now, games, sake, bar.
SIGNS = [
    (-1, 1.55, -0.75, 0.30, "居酒屋", "red"),
    (-1, 2.35, -1.05, 0.26, "ラーメン", "bright_yellow"),
    (-1, 3.4, -1.45, 0.26, "カラオケ", "bright_magenta"),
    (-1, 4.9, -1.1, 0.24, "焼鳥", "blue"),
    (-1, 6.8, -1.3, 0.24, "喫茶", "bright_cyan"),
    (1, 1.75, -0.85, 0.28, "寿司", "bright_cyan"),
    (1, 2.7, -1.15, 0.26, "営業中", "accent"),
    (1, 3.9, -1.2, 0.26, "ゲーム", "magenta"),
    (1, 5.5, -1.35, 0.24, "酒", "red"),
    (1, 7.4, -1.15, 0.22, "BAR", "green"),
]
# The signs take the theme's neon colors: with Tokyo Night, its blues, purples, cyan and pink, with a little orange
# and green, as that theme's own palette has them; any other theme recolors them in kind.
FLICKER = 2            # the karaoke sign's tube is failing
CELL = 0.62            # a character's size, as a share of its sign's width
LINE = 1.08


def vertical(text):
    # In vertical writing the long-vowel mark stands upright.
    return "\n".join(text.replace("ー", "｜"))


def sign_box(side, z, top, w, text):
    """The sign's face in the world: x of its center, top and bottom, and its character size."""
    size = w * CELL
    h = len(text) * size * LINE + w * 0.5
    x = side * (WALL - 0.06 - w / 2)
    return x, top, top + h, size


# Buildings along each wall: (from depth, to depth, height of roof, shade).
BUILDINGS = {
    -1: [(NEAR, 2.0, -2.6, 0.0), (2.0, 3.0, -1.9, 0.08), (3.0, 4.6, -2.4, 0.03), (4.6, 6.2, -1.7, 0.1),
         (6.2, END, -2.1, 0.05)],
    1: [(NEAR, 2.3, -2.3, 0.05), (2.3, 3.3, -2.8, 0.0), (3.3, 5.0, -1.8, 0.09), (5.0, 7.0, -2.2, 0.02),
        (7.0, END, -1.9, 0.07)],
}


# The LED screen's advertisement shows a few seconds of each of these, drawn by the renderer at build time in
# Omarchy's default colors. Theme backgrounds is left out, as it would show the build machine's own wallpaper, and so
# is Omarchy, which without someone's own screensaver art only shows ScreenSafe's name again.
PREVIEWS = ["Badge", "Orbits", "Starfield", "Echoes", "Cascade", "Harmonograph", "Constellations",
            "Trying To Remember", "Distro Wars", "Omnipotence", "Fibonacci"]
PREVIEW_SIZE = (192, 108)
PREVIEW_SECONDS = 4.0


def make_previews(out, packs, renderer):
    """Renders each pack's preview small and lays them side by side, a sprite sheet for the LED screen."""
    import base64
    import subprocess
    from PIL import Image
    w, h = PREVIEW_SIZE
    sheet = Image.new("RGB", (w * 4, h * math.ceil(len(PREVIEWS) / 4)))
    for n, name in enumerate(PREVIEWS):
        folder = os.path.join(packs, name.lower().replace(" ", "-"))
        result = subprocess.run([renderer, "thumbnail", folder, "--size", f"{w}x{h}"], capture_output=True, text=True,
                                check=True)
        kind, pw, ph, data = result.stdout.splitlines()[1].split(" ")
        assert kind == "pixels" and (int(pw), int(ph)) == (w, h), (name, kind, pw, ph)
        frame = Image.frombytes("RGB", (w, h), base64.b64decode(data))
        sheet.paste(frame, ((n % 4) * w, (n // 4) * h))
    sheet.save(os.path.join(out, "previews.png"), optimize=True)


def font_text():
    chars = set(string.ascii_letters + string.digits + " ｜§")
    for *_, text, _ in SIGNS:
        chars |= set(text.replace("ー", "｜"))
    return "".join(sorted(chars))


def make_font(out, source):
    from fontTools import subset
    from fontTools.ttLib import TTCollection
    face = next(f for f in TTCollection(source).fonts if f["name"].getDebugName(4) == FONT_FACE)
    opts = subset.Options()
    opts.name_IDs = ["*"]
    opts.name_languages = ["*"]
    opts.notdef_outline = True
    opts.layout_features = []
    sub = subset.Subsetter(opts)
    sub.populate(text=font_text())
    sub.subset(face)
    # A modified version takes a name of its own; the copyright and license records stay.
    names = face["name"]
    for rec in list(names.names):
        if rec.nameID in (1, 3, 4, 6, 16, 17, 18, 21, 22):
            names.removeNames(nameID=rec.nameID)
    for nid, value in ((1, "Tokyo Drip Sign"), (2, "Bold"), (3, "Tokyo Drip Sign Bold; subset of Noto Sans Mono CJK JP Bold 2.004"),
                       (4, "Tokyo Drip Sign Bold"), (6, "TokyoDripSign-Bold")):
        names.setName(value, nid, 3, 1, 0x409)
    if "CFF " in face:
        cff = face["CFF "].cff
        cff.fontNames = ["TokyoDripSign-Bold"]
        top = cff.topDictIndex[0]
        top.FullName, top.FamilyName = "Tokyo Drip Sign Bold", "Tokyo Drip Sign"
    # Dated as the source font is, rather than when it was built, so the same source always gives the same file.
    face.recalcTimestamp = False
    face["head"].modified = face["head"].created
    os.makedirs(os.path.join(out, "fonts"), exist_ok=True)
    face.save(os.path.join(out, "fonts", "sign.otf"))
    with open(os.path.join(out, "fonts", "OFL.txt"), "w") as f:
        f.write(OFL_HEADER + ofl_text())


OFL_HEADER = """Tokyo Drip Sign Bold (fonts/sign.otf) is a subset of Noto Sans Mono CJK JP Bold, version 2.004, cut down to the
characters this pack shows and renamed, as the license below asks of modified versions.

Copyright 2014-2021 Adobe (http://www.adobe.com/).

This Font Software is licensed under the SIL Open Font License, Version 1.1.
This license is copied below, and is also available with a FAQ at: https://openfontlicense.org

"""


def ofl_text():
    """The license's own text, from the Debian copyright file that came with the font."""
    lines = open("/usr/share/doc/fonts-noto-cjk/copyright", encoding="utf-8").read().splitlines()
    start = max(n for n, l in enumerate(lines) if l == "License: SIL-1.1")
    body = []
    for l in lines[start + 1:]:
        if l and not l.startswith(" "):
            break
        body.append("" if l.strip() == "." else l[1:])
    return "\n".join(body).strip() + "\n"


# ---------------------------------------------------------------------------------------------------------------
# The picture, back to front.


def sky():
    # The night sky is the scene's night; only the gap between the buildings shows it, lit from below by the
    # city.
    out = [element({"kind": "rect", "count": 2, "y": "-0.1 - i * 0.07", "w": "0.5 - i * 0.15", "h": 0.14,
                    "color": "alpha(mix(mix(magenta, blue, 0.4), night, 0.5), 0.09 - i * 0.03)"},
                   "The night sky, lit from below by the city.")]
    # Towers beyond, with lit windows and slow red lights on top.
    towers = [(-3.6, 1.2, -5.5), (-1.9, 1.0, -7.2), (1.6, 1.3, -6.4), (3.3, 1.1, -4.8)]
    zt = 16.0
    for x, w, top in towers:
        out.append(quad([P(x - w / 2, GROUND, zt), P(x + w / 2, GROUND, zt), P(x + w / 2, top, zt), P(x - w / 2, top, zt)],
                        "mix(sky, night, 0.6)"))
    out.append(element({
        "kind": "rect", "count": 160,
        "let": [f"tw = pick(floor(i / 40), {', '.join(num(t[0]) for t in towers)})",
                f"ww = pick(floor(i / 40), {', '.join(num(t[1]) for t in towers)})",
                f"tt = pick(floor(i / 40), {', '.join(num(t[2]) for t in towers)})",
                "col = mod(i, 5)", "row = floor(mod(i, 40) / 5)"],
        "x": f"(tw - ww * 0.38 + col * ww * 0.19) * {num(F / zt)}",
        "y": f"{num(VPY)} + (tt + 0.4 + row * 0.55) * {num(F / zt)}",
        "w": num(0.1 * F / zt), "h": num(0.16 * F / zt),
        "visible": "hash(i * 3.3) > 0.45",
        "color": "alpha(pick(mod(i, 3), mix(blue, ink, 0.5), mix(magenta, ink, 0.5), mix(yellow, ink, 0.5)), 0.35 + 0.2 * noise(time * 0.05 + i))"},
        "Their windows."))
    out.append(element({
        "kind": "rect", "count": len(towers),
        "x": f"{pick([t[0] for t in towers])} * {num(F / zt)}",
        "y": f"{num(VPY)} + ({pick([t[2] for t in towers])} - 0.1) * {num(F / zt)}",
        "w": 0.006, "h": 0.006,
        "color": "alpha(red, 0.25 + 0.75 * max(0, sin(time * 1.6 + i * 1.3)) ^ 6)"},
        "Aircraft warning lights."))
    return out


def far_end():
    """The building across the end of the street, and the LED screen on it."""
    out = [quad([P(-WALL - 0.4, GROUND, END), P(WALL + 0.4, GROUND, END), P(WALL + 0.4, -3.6, END), P(-WALL - 0.4, -3.6, END)],
                "mix(sky, night, 0.4)", "The building across the end of the street.")]
    cols, rows = 32, 18
    (sx0, sy0), (sx1, sy1) = P(-1.3, -3.05, END), P(1.3, -1.59, END)
    pw, ph = (sx1 - sx0) / cols, (sy1 - sy0) / rows
    out.append(quad([[sx0 - pw, sy0 - ph], [sx1 + pw, sy0 - ph], [sx1 + pw, sy1 + ph], [sx0 - pw, sy1 + ph]], "black",
                    "Its screen: a grid of LEDs, playing its program round and round."))
    # The program: a wave, a level meter and rings rippling out from the middle, twelve seconds each; then, as the
    # "Big screen" setting chooses, nothing more, or an advertisement for ScreenSafe (its name, its shield, and a few
    # seconds of each of its other screensavers), or one for Omarchy (its name, its mark lit in the LEDs themselves,
    # and its name scrolling past).
    loop = 36 + 8 + PREVIEW_SECONDS * len(PREVIEWS)
    omarchy_loop = 56
    out.append(element({
        "kind": "group", "glow": 1.2, "glow_radius": 0.008,
        # Without the advertisement (a setting), the program is the three pictures alone.
        "let": [f"slot = mod(time, screen == 1 ? {num(loop)} : screen == 2 ? {omarchy_loop} : 36)",
                "pic = screen == 2 && slot >= 40 && slot < 48 ? 4 : min(3, floor(slot / 12))",
                "blend = slot < 36 ? smoothstep(0.85, 1, fract(slot / 12)) : 0",
                f"cx = {num((sx0 + sx1) / 2)}", f"cy = {num((sy0 + sy1) / 2)}"]}))
    out.append(element({
        "kind": "rect", "count": cols * rows,
        "let": [f"c = mod(i, {cols})", f"r = floor(i / {cols})",
                f"cu = c / {cols - 1}", f"cv = r / {rows - 1}",
                "wave = 0.5 + 0.5 * sin(cu * 9 - time * 2.2 + sin(cv * 4 + time) * 1.5)",
                "level = step(1 - cv, 0.15 + 0.8 * noise(c * 0.37 + time * 1.8) ^ 1.5)",
                "ring = 0.5 + 0.5 * cos(sqrt(sqr((cu - 0.5) * 1.8) + sqr(cv - 0.5)) * 22 - time * 3)",
                # The Omarchy mark, a light to each square of its 13 x 13 pattern, swept on from its top left corner.
                "mc = c - 9", "mr = r - 2",
                # The mark's rows, each a 13-bit number, its left square the highest bit: Omarchy's own, from its icon.txt.
                "mrow = pick(clamp(mr, 0, 12), 8191, 4161, 6093, 5125, 5125, 5125, 7173, 5125, 5125, 5125, 6141, 4161, 8159)",
                "inside = mc >= 0 && mc < 13 && mr >= 0 && mr < 13",
                "mark = inside ? mod(floor(mrow / 2 ^ (12 - mc)), 2) * step((mc + mr) / 24, (slot - 40) / 2.5) * (0.8 + 0.2 * sin(time * 3)) : 0",
                "a = pick(pic, wave, level, ring, 0, mark)", "b = pick(pic + 1, wave, level, ring, 0, mark)",
                "lit = mix(a, b, blend)"],
        "x": f"{num(sx0 + pw / 2)} + c * {num(pw)}", "y": f"{num(sy0 + ph / 2)} + r * {num(ph)}",
        "w": num(pw * 0.72), "h": num(ph * 0.72),
        "color": "alpha(pick(pic, mix(bright_cyan, bright_magenta, cu), mix(blue, red, 1 - cv), mix(accent, bright_yellow, cv), accent, mix(accent, white, 0.15)), 0.08 + 0.92 * lit)"},
        depth=1))
    out.append(element({
        "kind": "text", "font": "sign", "text": "ScreenSafe", "size": num((sx1 - sx0) / 10 / 0.62 * 0.9),
        "align": "center", "valign": "middle", "x": "cx", "y": "cy",
        "visible": "screen == 1 && slot >= 36 && slot < 40",
        "color": "alpha(mix(accent, white, 0.3), smoothstep(36, 36.6, slot) * (1 - smoothstep(39.4, 40, slot)))"},
        "ScreenSafe's name,", depth=1))
    # Omarchy's: its name, its mark (in the LEDs, above), and its name again, scrolling past.
    out.append(element({
        "kind": "text", "font": "sign", "text": "Omarchy", "size": num((sx1 - sx0) / 7 / 0.62 * 0.9),
        "align": "center", "valign": "middle", "x": "cx", "y": "cy",
        "visible": "screen == 2 && slot >= 36 && slot < 40",
        "color": "alpha(mix(accent, white, 0.3), smoothstep(36, 36.6, slot) * (1 - smoothstep(39.4, 40, slot)))"},
        "Omarchy's name,", depth=1))
    marquee = (sy1 - sy0) * 0.55
    out.append(element({
        "kind": "group", "visible": "screen == 2 && slot >= 48 && slot < 56",
        "clip": [num(sx0), num(sy0), num(sx1), num(sy1)]}, "and its name, scrolling past.", depth=1))
    out.append(element({
        "kind": "text", "font": "sign", "text": "OMARCHY  OMARCHY", "size": num(marquee), "valign": "middle",
        "x": f"{num(sx1)} - (slot - 48) / 8 * {num((sx1 - sx0) + marquee * 0.62 * 16)}", "y": "cy",
        "color": "mix(accent, white, 0.2)"}, depth=2))
    out += shield(sx0, sy0, sx1, sy1)
    w, h = PREVIEW_SIZE
    out.append(element({
        "kind": "sprite", "file": "previews.png", "frame_size": [w, h], "anchor": [w // 2, h // 2], "smooth": True,
        "x": "cx", "y": "cy", "pixel": num((sx1 - sx0) / w),
        "let": [f"shown = (slot - 44) / {num(PREVIEW_SECONDS)}"],
        "frame": "floor(shown)", "visible": "screen == 1 && slot >= 44",
        "opacity": "smoothstep(0, 0.12, fract(shown)) * (1 - smoothstep(0.88, 1, fract(shown)))"},
        "and a few seconds of each of its screensavers: " + ", ".join(PREVIEWS) + ".", depth=1))
    out.append(element({
        "kind": "lines", "count": cols + rows, "samples": 2, "width": num(min(pw, ph) * 0.3),
        "visible": "screen == 1 && slot >= 44 || screen == 2 && slot >= 48",
        "px": f"i < {cols} ? {num(sx0)} + i * {num(pw)} : {num(sx0)} + s * {num(sx1 - sx0)}",
        "py": f"i < {cols} ? {num(sy0)} + s * {num(sy1 - sy0)} : {num(sy0)} + (i - {cols}) * {num(ph)}",
        "color": "alpha(black, 0.35)"}, "The LEDs' grid, over the pictures.", depth=1))
    return out


def shield(sx0, sy0, sx1, sy1):
    """ScreenSafe's mark, as the panel draws it: a shield with three points along its top, coming to a point below,
    in the accent, its left half a shade lighter, with a section sign cut through it (here, the screen's black)."""
    hgt = (sy1 - sy0) * 0.78
    wid = hgt * 14 / 16
    cx, cy = (sx0 + sx1) / 2, (sy0 + sy1) / 2
    at = lambda u, v: [cx + (u - 0.5) * wid, cy + (v - 0.5) * hgt]

    def curve(p0, c, p1, steps=12):
        return [at(*[(1 - t) ** 2 * a + 2 * (1 - t) * t * b + t ** 2 * d for a, b, d in zip(p0, c, p1)])
                for t in (k / steps for k in range(1, steps + 1))]

    right_curve = curve((1, 0.46), (1, 0.8), (0.5, 1))
    left_curve = curve((0.5, 1), (0, 0.8), (0, 0.46))
    whole = [at(0, 0.02), at(0.25, 0.15), at(0.5, 0), at(0.75, 0.15), at(1, 0.02), at(1, 0.46)] + right_curve + left_curve
    left = [at(0, 0.02), at(0.25, 0.15), at(0.5, 0)] + [at(0.5, 1)] + left_curve
    fade = "smoothstep(40, 40.6, slot) * (1 - smoothstep(43.4, 44, slot))"
    return [
        element({"kind": "group", "visible": "screen == 1 && slot >= 40 && slot < 44", "opacity": fade}, "its shield,", depth=1),
        element({"kind": "lines", "points": whole, "closed": True, "width": 0, "fill": "accent"}, depth=2),
        element({"kind": "lines", "points": left, "closed": True, "width": 0, "fill": "mix(accent, white, 0.2)"}, depth=2),
        # Sized and placed by its ink, as the panel's mark places it: centered across, a little below the middle, about
        # 0.44 of the shield's height. (This font's § sits right of and below its cell's center, so it's moved back.)
        element({"kind": "text", "font": "sign", "text": "§", "size": num(hgt * 0.514), "align": "center", "valign": "middle",
                 "x": num(cx - hgt * 0.1249), "y": num(cy - hgt * 0.116), "color": "black"}, depth=2),
    ]


def buildings():
    out = []
    for side in (-1, 1):
        for n, (z0, z1, top, shade) in enumerate(BUILDINGS[side]):
            x = side * WALL
            out.append(quad([P(x, GROUND, z0), P(x, GROUND, z1), P(x, top, z1), P(x, top, z0)],
                            f"mix(night, black, {num(0.25 + shade)})",
                            ("The buildings on the left," if side < 0 else "and on the right,") if n == 0 else None))
            # The end of a taller building where a lower one meets it.
            if n + 1 < len(BUILDINGS[side]) and BUILDINGS[side][n + 1][2] > top:
                nxt = BUILDINGS[side][n + 1][2]
                out.append(quad([P(x, nxt, z1), P(x + side * 0.6, nxt, z1), P(x + side * 0.6, top, z1), P(x, top, z1)],
                                f"mix(night, black, {num(0.45 + shade)})"))
        # Windows: a grid along each building, some lit, warm or cool, now and then going dark or coming on.
        spans = BUILDINGS[side]
        out.append(element({
            "kind": "rect", "count": 22 * 6,
            "let": [f"col = floor(i / 6)", "row = mod(i, 6)",
                    f"zz = mix(2.1, {num(END - 0.4)}, (col + 0.5) / 22)",
                    "roof = " + " : ".join(f"zz < {num(b[1])} ? {num(b[2])}" for b in spans[:-1]) + f" : {num(spans[-1][2])}",
                    "yy = -0.75 - row * 0.3"],
            "visible": "yy > roof + 0.15 && hash(i * 7.31 + " + str(side) + ") > 0.42",
            "x": f"{num(side * WALL)} * {num(F)} / zz", "y": f"{num(VPY)} + yy * {num(F)} / zz",
            # On the wall's face, a window's width shrinks with the square of its distance.
            "w": f"{num(WALL * F * 0.1)} / sqr(zz)", "h": f"{num(0.15 * F)} / zz",
            "color": f"alpha(pick(floor(hash(i * 1.9 + {side}) * 4), mix(blue, ink, 0.4), mix(magenta, ink, 0.4), mix(bright_cyan, ink, 0.5), mix(yellow, ink, 0.4)), "
                     f"0.22 * step(0.12, noise(time * 0.02 + i * 0.7)))"},
            "Their windows."))
        # Shopfronts at street level, one after another with pillars between: glass lit from inside, and a short
        # curtain over each door.
        for part, (lo, hi, color) in enumerate(((GROUND, 0.14, "alpha(pick(floor(hash(i * 2.3 + {side}) * 5), mix(blue, ink, 0.3), mix(magenta, ink, 0.3), mix(bright_cyan, ink, 0.4), mix(bright_yellow, ink, 0.35), mix(red, ink, 0.35)), (0.17 + 0.06 * noise(time * 0.3 + i)) * min(1, za / 2.6))"),
                                                 (0.14, 0.02, "mix(pick(floor(hash(i * 4.7 + {side}) * 4), red, blue, magenta, accent), black, 0.5)"))):
            corners = [(lo, "za"), (lo, "zb"), (hi, "zb"), (hi, "za")]
            out.append(element({
                "kind": "lines", "count": 16, "samples": 4, "closed": True, "width": 0,
                "let": ["za = 1.05 + i * 0.5", "zb = za + 0.38"],
                "visible": f"zb < {num(END)}",
                "px": f"{num(side * WALL * F)} / pick(j, za, zb, zb, za)",
                "py": f"{num(VPY)} + pick(j, {num(lo)}, {num(lo)}, {num(hi)}, {num(hi)}) * {num(F)} / pick(j, za, zb, zb, za)",
                "fill": color.replace("{side}", str(side))},
                ("Shopfronts at street level, lit from inside," if part == 0 else "a short curtain over each door.") if side < 0 else None))
    return out


def street():
    # Starting a little nearer than the walls, so it reaches the bottom of the screen.
    out = [quad([P(-WALL, GROUND, 0.7), P(WALL, GROUND, 0.7), P(WALL, GROUND, END), P(-WALL, GROUND, END)],
                "mix(night, black, 0.45)", "The wet street,"),
           # Kerbs, and the line down the middle.
           element({"kind": "lines", "count": 2, "samples": 2, "width": 0.002,
                    "px": f"(i * 2 - 1) * {num(WALL - 0.25)} * {num(F)} / mix({num(NEAR)}, {num(END)}, s)",
                    "py": f"{num(VPY)} + {num(GROUND * F)} / mix({num(NEAR)}, {num(END)}, s)",
                    "color": "alpha(ink, 0.12)"}, "its kerbs"),
           element({"kind": "rect", "count": 9,
                    "let": [f"za = 1.1 + i * 0.85", "zb = za + 0.4"],
                    "x": 0, "y": f"{num(VPY)} + {num(GROUND * F)} * (1 / za + 1 / zb) / 2",
                    "w": f"{num(0.025 * F)} / za", "h": f"{num(GROUND * F)} * (1 / za - 1 / zb)",
                    "color": "alpha(ink, 0.1)"}, "and the line down the middle.")]
    # Reflections: every sign's light, streaked down the wet street below it, shimmering.
    xs, tops, bots, zs, ws, colors = [], [], [], [], [], []
    for side, z, top, w, text, color in SIGNS:
        x, t, b, _ = sign_box(side, z, top, w, text)
        xs.append(x * F / z)
        tops.append(VPY + (2 * GROUND - b) * F / z)
        bots.append(VPY + (2 * GROUND - t) * F / z)
        zs.append(z)
        ws.append(w * F / z)
        colors.append(color)
    # Broken by the wet asphalt into ripples: short dashes down the street below each sign, wavering.
    dashes = 12
    out.append(element({
        "kind": "rect", "count": len(SIGNS) * dashes,
        "let": [f"s0 = mod(i, {len(SIGNS)})", f"d = floor(i / {len(SIGNS)}) / {dashes - 1}",
                f"ya = pick(s0, {', '.join(num(v) for v in tops)})", f"yb = pick(s0, {', '.join(num(v) for v in bots)})",
                f"wd = pick(s0, {', '.join(num(v) for v in ws)})",
                f"on = s0 == {FLICKER} ? 1 - 0.85 * step(0.8, noise(time * 2.3)) : 1"],
        "x": f"pick(s0, {', '.join(num(v) for v in xs)}) + wd * 0.35 * (noise(time * 0.8 + i * 1.7) - 0.5)",
        "y": "mix(ya, yb, d)", "w": "wd * (0.35 + 0.6 * noise(time * 1.1 + i * 0.9))",
        "h": f"(yb - ya) / {dashes} * 0.45",
        "color": f"alpha(pick(s0, {', '.join(colors)}), on * 0.32 * (1 - 0.7 * d))"},
        "The signs' light, broken into ripples on the wet street."))
    return out


def signs():
    out = []
    boxes = [sign_box(side, z, top, w, text) for side, z, top, w, text, _ in SIGNS]
    steady = [k for k in range(len(SIGNS)) if k != FLICKER]

    def sign(k, depth):
        side, z, top, w, text, color = SIGNS[k]
        x, t, b, size = boxes[k]
        (ax, ay), (bx, by) = P(x - w / 2, t, z), P(x + w / 2, b, z)
        bracket = [P(side * WALL, t + 0.08, z), P(x + side * w / 2, t + 0.08, z)]
        inset = 0.025 * F / z
        parts = [
            element({"kind": "lines", "points": bracket, "width": num(0.012 * F / z), "color": "mix(night, black, 0.2)"},
                    depth=depth),
            quad([[ax, ay], [bx, ay], [bx, by], [ax, by]], f"mix({color}, black, 0.82)", depth=depth),
            element({"kind": "lines", "points": [[ax + inset, ay + inset], [bx - inset, ay + inset], [bx - inset, by - inset],
                                                 [ax + inset, by - inset]],
                     "closed": True, "width": num(0.008 * F / z), "color": color}, depth=depth),
            element({"kind": "text", "font": "sign", "text": vertical(text), "size": num(size * F / z),
                     "line_height": LINE, "align": "center",
                     "x": num((ax + bx) / 2 - 0.25 * size * F / z), "y": num(ay + 0.25 * w * F / z),
                     "color": f"mix({color}, white, 0.35)"}, depth=depth),
        ]
        return parts

    out.append(element({"kind": "group", "glow": 1.6, "glow_radius": 0.01},
                       "Signs jutting from the walls, their neon lighting the rain. Being still, their glow is "
                       "worked out once and kept."))
    for k in steady:
        out += sign(k, 1)
    # Lanterns: a pair of red paper lanterns at the izakaya's door, swaying, so in a glow of their own.
    out.append(element({"kind": "group", "glow": 1.4, "glow_radius": 0.01}))
    out.append(element({
        "kind": "lines", "count": 2, "samples": 24, "closed": True, "width": 0,
        "let": ["lz = 1.9 + i * 0.32", f"lx = {num(-WALL + 0.12)} * {num(F)} / lz",
                f"ly = {num(VPY)} + (-0.1 + 0.02 * sin(time * 0.8 + i)) * {num(F)} / lz", f"lr = {num(0.075 * F)} / lz"],
        "px": "lx + lr * 0.8 * cos(s * tau)", "py": "ly + lr * 1.15 * sin(s * tau)",
        "fill": "mix(red, orange, 0.25)"}, "Red paper lanterns at the izakaya's door.", depth=1))
    out.append(element({
        "kind": "rect", "count": 6,
        "let": ["lz = 1.9 + floor(i / 3) * 0.32", f"lr = {num(0.075 * F)} / lz", "band = mod(i, 3) - 1"],
        "x": f"{num(-WALL + 0.12)} * {num(F)} / lz",
        "y": f"{num(VPY)} + (-0.1 + 0.02 * sin(time * 0.8 + floor(i / 3))) * {num(F)} / lz + band * lr * 0.5",
        "w": "lr * 1.45 * (1 - abs(band) * 0.25)", "h": "lr * 0.06", "color": "alpha(mix(red, black, 0.5), 0.8)"},
        depth=1))
    out.append(element({"kind": "group", "glow": 1.6, "glow_radius": 0.01,
                        "opacity": "1 - 0.85 * step(0.8, noise(time * 2.3)) - 0.3 * step(0.93, noise(time * 9.1 + 4))"},
                       "The karaoke sign, its tube failing: its own glow, so the others' stays still."))
    out += sign(FLICKER, 1)
    return out


def vending():
    z0, z1 = 1.5, 1.95
    x = WALL - 0.02
    face = [P(x - 0.3, GROUND, z0), P(x - 0.3, GROUND, z1), P(x - 0.3, -0.25, z1), P(x - 0.3, -0.25, z0)]
    inner = [P(x - 0.3, -0.05, z0 + 0.04), P(x - 0.3, -0.05, z1 - 0.04), P(x - 0.3, -0.22, z1 - 0.04), P(x - 0.3, -0.22, z0 + 0.04)]
    out = [quad(face, "mix(night, ink, 0.25)", "A vending machine, glowing at the kerb."),
           quad(inner, "alpha(mix(ink, bright_cyan, 0.4), 0.8)"),
           element({"kind": "rect", "count": 12,
                    "let": ["zz = mix(1.56, 1.89, (mod(i, 4) + 0.5) / 4)", "yy = 0.02 + floor(i / 4) * 0.1"],
                    "x": f"{num(x - 0.3)} * {num(F)} / zz", "y": f"{num(VPY)} + yy * {num(F)} / zz",
                    "w": f"{num((x - 0.3) * F * 0.05)} / sqr(zz)", "h": f"{num(0.06 * F)} / zz",
                    "color": "pick(i, red, blue, bright_magenta, bright_yellow, magenta, bright_cyan, green, red, accent, bright_cyan, bright_yellow, blue)"})]
    return out


def wires():
    out = []
    for n, (za, zb, ya, yb) in enumerate(((2.2, 3.0, -1.9, -1.75), (3.6, 4.4, -1.7, -1.8), (5.8, 6.6, -1.75, -1.6))):
        out.append(element({
            "kind": "lines", "samples": 24, "width": 0.0015,
            "sample_let": [f"zz = mix({num(za)}, {num(zb)}, s)", f"yy = mix({num(ya)}, {num(yb)}, s) + 0.15 * sin(s * pi)"],
            "px": f"mix({num(-WALL)}, {num(WALL)}, s) * {num(F)} / zz", "py": f"{num(VPY)} + yy * {num(F)} / zz",
            "color": "alpha(black, 0.7)"}, "Wires across the street." if n == 0 else None))
    return out


def walkers():
    """Two people with clear umbrellas, crossing the street far off, each at their own pace."""
    out = [element({
        "kind": "group", "count": 2,
        "let": ["wz = pick(i, 4.3, 6.6)", "pace = pick(i, 0.045, 0.032)", "dir = pick(i, 1, -1)",
                f"wx = dir * (fract(time * pace / 2.6 + i * 0.4) * 2.6 - 1.3)"],
        "x": f"wx * {num(F)} / wz", "y": f"{num(VPY)} + {num(GROUND * F)} / wz", "scale": f"{num(0.27 * F)} / wz"},
        "Two people with clear umbrellas, crossing the street far off.")]
    step = "sin(time * 5 + i * 2)"
    parts = [
        {"kind": "lines", "count": 2, "samples": 2, "width": 0.07, "color": "mix(night, black, 0.6)",
         "px": f"(i - 0.5) * 0.12 + s * {step} * 0.1 * (i * 2 - 1)", "py": "-0.85 + s * 0.85"},
        {"kind": "lines", "samples": 20, "closed": True, "width": 0, "fill": "mix(night, black, 0.55)",
         "px": "0.2 * cos(s * tau)", "py": "-1.15 + 0.36 * sin(s * tau)"},
        {"kind": "lines", "samples": 16, "closed": True, "width": 0, "fill": "mix(night, black, 0.55)",
         "px": "0.11 * cos(s * tau)", "py": "-1.62 + 0.12 * sin(s * tau)"},
        {"kind": "lines", "samples": 2, "width": 0.02, "color": "alpha(white, 0.35)", "px": "0.15", "py": "-1.3 - s * 0.75"},
        {"kind": "lines", "samples": 24, "closed": True, "width": 0.012, "color": "alpha(white, 0.5)",
         "fill": "alpha(white, 0.1)", "px": "0.15 + 0.62 * cos(s * pi + pi)", "py": "-2.02 + 0.42 * min(0, sin(s * pi + pi)) + 0.0"},
    ]
    out += [element(p, depth=1) for p in parts]
    return out


def rain():
    out = [element({
        "kind": "lines", "count": 260, "samples": 2, "width": 0.0012,
        "let": ["rz = 1.0 + 8 * hash(i * 1.37)", "rx = (hash(i * 4.11) * 2 - 1) * 1.5",
                "fall = fract(time * (0.9 + 0.3 * hash(i * 2.2)) + hash(i * 9.7))", "ry = -2.6 + fall * 3.1"],
        "px": f"(rx - s * 0.04) * {num(F)} / rz", "py": f"{num(VPY)} + (ry - s * 0.22) * {num(F)} / rz",
        # The rain setting thins it: a share of the streaks, as many as it says.
        "visible": f"ry < {num(GROUND)} && hash(i * 5.9) < rain",
        "color": "alpha(mix(ink, mix(blue, bright_cyan, 0.5), 0.35), 0.18 + 0.25 / rz)"},
        "Rain, falling at many depths.")]
    # Rings where rain lands on the wet street.
    out.append(element({
        "kind": "lines", "count": 40, "samples": 16, "closed": True, "width": 0.001,
        "visible": "hash(i * 4.2) < rain",
        "let": ["gz = 1.2 + 7 * hash(i * 3.9)", f"gx = (hash(i * 6.2) * 2 - 1) * {num(WALL - 0.1)}",
                "age = fract(time * 0.8 + hash(i * 2.7))", "gr = 0.09 * age"],
        "sample_let": ["zz = gz + gr * sin(s * tau)"],
        "px": f"(gx + gr * cos(s * tau)) * {num(F)} / zz", "py": f"{num(VPY)} + {num(GROUND * F)} / zz",
        "color": "alpha(mix(ink, mix(blue, bright_cyan, 0.5), 0.4), 0.35 * (1 - age))"},
        "Rings where it lands."))
    return out


def drips():
    """Drops gathering under every sign and falling to the street, each landing in a ring."""
    xs, ys, zs, colors = [], [], [], []
    for side, z, top, w, text, color in SIGNS:
        x, t, b, _ = sign_box(side, z, top, w, text)
        xs.append(x)
        ys.append(b)
        zs.append(z)
        colors.append(color)
    lets = [f"dx = pick(i, {', '.join(num(v) for v in xs)})", f"dy = pick(i, {', '.join(num(v) for v in ys)})",
            f"dz = pick(i, {', '.join(num(v) for v in zs)})",
            "period = 1.6 + 1.4 * hash(i * 5.3)", "age = mod(time + hash(i * 8.1) * 9, period)",
            f"fall = sqrt(2 * ({num(GROUND)} - dy) / 9)"]
    out = [element({
        "kind": "lines", "count": len(SIGNS), "samples": 2, "width": "0.009 / dz", "visible": "rain > 0",
        "let": lets + ["swell = clamp(age / (period - fall), 0, 1)",
                       "fy = age < period - fall ? dy + 0.02 * swell : dy + 0.02 + 4.5 * sqr(age - period + fall)",
                       "len = age < period - fall ? 0.015 : 0.06"],
        "px": f"dx * {num(F)} / dz", "py": f"{num(VPY)} + (fy - len * s) * {num(F)} / dz",
        "color": f"alpha(mix({pick_color(colors)}, white, 0.5), 0.85)"},
        "Drops swelling under every sign, then falling,")]
    out.append(element({
        "kind": "lines", "count": len(SIGNS) * 2, "samples": 20, "closed": True, "width": 0.0012,
        "let": [l.replace("pick(i,", "pick(mod(i, 10),") for l in lets]
               + ["since = age", "ring = floor(i / 10)", "gr = (0.04 + 0.1 * since) * (1 - ring * 0.4)"],
        "sample_let": ["zz = dz + gr * sin(s * tau)"],
        "px": f"(dx + gr * cos(s * tau)) * {num(F)} / zz", "py": f"{num(VPY)} + {num(GROUND * F)} / zz",
        "visible": "rain > 0 && since < 0.9",
        "color": f"alpha(mix(pick(mod(i, 10), {', '.join(colors)}), white, 0.4), 0.6 * (1 - since / 0.9))"},
        "and landing in rings."))
    return out


def build(out, font_source=None):
    os.makedirs(out, exist_ok=True)
    if font_source:
        make_font(out, font_source)
    here = os.path.dirname(os.path.abspath(__file__))
    make_previews(out, os.path.join(here, "..", "..", "packs"), os.path.join(here, "..", "..", "renderer", "target", "release", "screensafe"))
    head = '''# Tokyo Drip: a rainy Tokyo side street at night, in the theme's colors. Written by tools/tokyo-drip/build.py.
format = 2
colorize = "tint_color"
name = "Tokyo Drip"
author = "ScreenSafe"
license = "MIT"
description = "A rainy Tokyo side street at night: neon signs, lanterns and an LED screen, as rain drips onto the wet street, in your theme's colors."
preview = 30
fps = 30
fade = 0

[fonts]
sign = "fonts/sign.otf"

# What can be changed in the panel: how hard it rains (with none, the signs stop dripping, though the street stays
# wet), and what the big screen advertises between its pictures: nothing, ScreenSafe, or Omarchy.
[[setting]]
name = "rain"
label = "Rain"
kind = "choice"
choices = ["None", "Light", "Heavy"]
values = [0, 0.4, 1]
default = 1

[[setting]]
name = "screen"
label = "Big screen"
kind = "choice"
choices = ["Patterns", "ScreenSafe", "Omarchy"]
values = [0, 1, 2]
default = 1

[[setting]]
name = "tint_color"
label = "Color"
kind = "color"
default = 0

[[scene]]
name = "The street"
duration = 600
# It's always night here. A dark theme's own colors make it; a light theme's are darkened to dusk, keeping their tint,
# and its light color takes the place of a dark theme's for the rain and the like, so the neon still glows.
let = ["night = light ? mix(background, black, 0.8) : background",
       "sky = light ? mix(darker_background, black, 0.86) : darker_background",
       "ink = light ? background : foreground"]
background = "light ? mix(darker_background, black, 0.86) : darker_background"

'''
    body = sky() + far_end() + buildings() + street() + wires() + vending() + walkers() + signs() + drips() + rain()
    with open(os.path.join(out, "screensafe.toml"), "w") as f:
        f.write(head + "\n".join(body))


if __name__ == "__main__":
    build(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else (FONT_SOURCE if os.path.exists(FONT_SOURCE) else None))
