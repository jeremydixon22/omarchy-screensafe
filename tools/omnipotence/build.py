"""Builds the Omnipotence pack: figures drawn over the very code that draws them, and the Hand of Omnipotence turning
each page to the code of the next.

    python3 build.py OUT_DIR [HAND_SHEET]

HAND_SHEET is the Hand of Omnipotence's poses on chroma green (by default, hand-sheet.png beside this file; see
README.md and hand-provenance.json); hands.py cuts out the reaching and pinching poses as a sprite sheet. The page's curl is
drawn here, in strips, as the turn goes.

Each page is a scene. It opens on its own code, the `screensafe.toml` lines that draw its figure, exactly as they
appear in the pack. The figure then draws itself over them, and each block of code lights up while it's the one
drawing. The hand then turns the page along a diagonal fold, revealing the next page's code beneath.
"""
import math
import os
import sys

from PIL import Image

STAGE_W, STAGE_H = 16 / 9, 1.0
X0, Y0 = -STAGE_W / 2, -0.5

# A page's moments, in seconds.
DUR = 40.0
DIM = (5.0, 7.0)          # the code dims as the figure begins
HAND_IN = (33.0, 34.5)    # the hand reaches in and takes the corner,
TURN = (34.5, 39.5)       # then pulls the page up and away to the top left

# The hand takes the page's bottom-right corner and pulls it across to the left, as a book's page is turned, so the
# fold runs steeply up and to the right and sweeps across the whole page, square to itself. Halfway, with the hand
# at the far side, the page stands up toward us and lifts away to the top left.
_l = math.hypot(1, 0.45)
NX, NY = -1 / _l, -0.45 / _l        # the fold's normal, pointing into the page: left, and a little up
THETA = math.atan2(NY, NX)
CORNER = -X0 * NX - Y0 * NY         # the bottom-right corner's distance along the normal
FAR = -CORNER                       # and the top-left corner's
X1, Y1 = -X0, -Y0
TX, TY = NY, -NX                    # along the fold

# The page curl: the lifted page rolls up over a cylinder lying along the fold, then rises straight on toward us at
# the cylinder's last angle. The front faces us until the paper is edge-on, a quarter of the way round; past that we
# see its back.
CURL_R = (0.05, 0.15)     # the cylinder's radius, as the pull begins and once it's under way
SWEEP_END = FAR + CURL_R[1] + 0.08  # where the fold ends: every part of the page is then past the top-left corner
# Keyframes, by the fold's place: how far round the cylinder the page goes (radians), and how much nearer a raised
# point looks, per unit of height. Lying back over itself as it's pulled across, then standing up toward us.
BEND_KEYS = [(CORNER - 0.02, 0.7), (-0.8, 2.0), (-0.6, 2.8), (-0.4, 2.9), (0.0, 2.9), (0.3, 2.3), (0.6, 1.9),
             (SWEEP_END, 1.7)]
NEAR_KEYS = [(0.0, 0.4), (0.3, 1.2), (0.6, 1.7), (SWEEP_END, 2.0)]
# The fold's place over time: slowly as the hand takes the weight, quicker as it pulls, quickest as the page lifts.
FOLD_KEYS = [(34.5, CORNER - 0.02), (35.2, -0.92), (36.4, -0.5), (37.6, 0.0), (38.5, 0.55), (39.4, SWEEP_END)]
K_MAX = 3.0
FRONT_STRIPS, BACK_STRIPS, FLAT_STRIPS = 8, 6, 4
HAND_FRAME = (809, 643)
HAND_PINCH = (619, 572)
HAND_PIXEL = 0.00062


BG = "#080B16"
CODE = {"header": "#56D4F5", "key": "#F25FD0", "string": "#7CF5A8", "number": "#FFC86B", "comment": "#8A7FB0"}
HIGHLIGHT = "#FFFFFF"
SIZE = 0.021                        # code text
ADV = 0.6 * SIZE                    # its monospace advance
STEP = SIZE * 1.35
CODE_X, CODE_Y = X0 + 0.05, Y0 + 0.055
WRAP = 64
VISIBLE_ROWS = int((STAGE_H - 0.11) / STEP)


def num(v):
    out = f"{v:.4f}".rstrip("0").rstrip(".")
    return "0" if out in ("", "-0") else out


def q(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def keys(pairs):
    return ", ".join(f"{num(a)}, {num(b)}" for a, b in pairs)


def warp_lets(xe, ye):
    """Named values carrying the point (xe, ye) of the page to where the turn puts it: (WX, WY)."""
    return [f"wx0 = {xe}", f"wy0 = {ye}",
            f"wu = {num(NX)} * wx0 + {num(NY)} * wy0", f"wv = {num(TX)} * wx0 + {num(TY)} * wy0",
            "wq = max(0, fold - wu)", "wa = min(wq / rr, bend)", "wr = max(0, wq - bend * rr)",
            "wm = wu + wq - rr * sin(wa) - wr * cos(bend)",
            "wz = rr * (1 - cos(wa)) + wr * sin(bend)",
            f"wk = min({num(K_MAX)}, 1 + near * wz)"]


WX = f"({num(NX)} * wm + {num(TX)} * wv) * wk"
WY = f"({num(NY)} * wm + {num(TY)} * wv) * wk"


def warped(fields):
    """A copy of a figure element whose every point is carried along by the turn."""
    f = dict(fields)
    if f["kind"] == "lines":
        f["sample_let"] = warp_lets(f.pop("px"), f.pop("py"))
        f["px"], f["py"] = WX, WY
        # A leaflet whose root has gone round past edge-on is under the page's back, and isn't drawn.
        lets = " ".join(f.get("let", []))
        root = next(((x, y) for x, y in (("lx", "ly"), ("sx", "sy")) if f"{x} = " in lets and f"{y} = " in lets), None)
        if root:
            hidden = f"fold - ({num(NX)} * {root[0]} + {num(NY)} * {root[1]}) < rr * 1.5708 + 0.06"
            f["visible"] = f"({f['visible']}) && {hidden}" if "visible" in f else hidden
    elif f["kind"] == "rect":
        f["let"] = list(f.get("let", [])) + warp_lets(f.pop("x", "0"), f.pop("y", "0"))
        f["x"], f["y"] = WX, WY
        f["w"] = f"({f.get('w', '0.1')}) * wk"
        f["h"] = f"({f.get('h', '0.1')}) * wk"
    else:
        raise ValueError(f"can't turn a {f['kind']}")
    # Lines and rects keep their order: "draw", "color" and the rest follow the moved keys harmlessly.
    return f


# ---------------------------------------------------------------------------------------------------------------
# The figures. Each is a list of elements: (comment, fields, (from, to)), the window being when it draws, for
# lighting its code.

CX, CY = 0.30, 0.02


def prog(a, b, ease=True):
    inner = f"(t - {num(a)}) / {num(b - a)}"
    return f"ease_in_out({inner})" if ease else f"clamp({inner}, 0, 1)"


def fern(start=7.0, end=27.0, cx=CX, by=0.46):
    stem = ("bx + ln * v * sin(a + c * v * 0.5)", "by - ln * v * cos(a + c * v * 0.5)")
    fronds = [
        ("Three fronds rise from one root, each leaning its own way.", {
            "kind": "lines", "count": 3,
            "let": [f"bx = {num(cx)}", f"by = {num(by)}", "a = pick(i, -0.55, 0.05, 0.6)",
                    "ln = pick(i, 0.62, 0.86, 0.66)", "c = pick(i, -0.4, 0.15, 0.45)"],
            "samples": 120, "px": stem[0].replace("v", "s"), "py": stem[1].replace("v", "s"),
            "width": 0.0034, "color": "#5EF2E0", "draw": prog(start, start + 6)}, (start, start + 6)),
        ("Leaflets open along each frond, from its root to its tip.", {
            "kind": "lines", "count": 132,
            "let": ["f = floor(i / 44)", "v = 0.06 + 0.92 * mod(i, 44) / 44", "side = mod(i, 2) * 2 - 1",
                    f"bx = {num(cx)}", f"by = {num(by)}", "a = pick(f, -0.55, 0.05, 0.6)",
                    "ln = pick(f, 0.62, 0.86, 0.66)", "c = pick(f, -0.4, 0.15, 0.45)",
                    f"sx = {stem[0]}", f"sy = {stem[1]}", "th = a + c * v",
                    "len = 0.15 * ln * sin(pi * (0.08 + 0.88 * v)) * (1 - 0.45 * v)"],
            "samples": 14,
            "px": "sx + len * s * sin(th + side * (1.15 - 0.7 * s))",
            "py": "sy - len * s * cos(th + side * (1.15 - 0.7 * s))",
            "width": 0.0021, "color": "mix(#2FB7B0, #A8F7FF, v)",
            "draw": f"clamp((t - {num(start + 2)} - v * {num(end - start - 8)}) / 1.4, 0, 1)"}, (start + 2, end)),
        ("Each leaflet feathers into seven smaller ones as it grows.", {
            "kind": "lines", "count": 924,
            "let": ["leaf = floor(i / 7)", "w = (mod(i, 7) + 1) / 8", "f = floor(leaf / 44)",
                    "v = 0.06 + 0.92 * mod(leaf, 44) / 44", "side = mod(leaf, 2) * 2 - 1", "sub = mod(i, 2) * 2 - 1",
                    f"bx = {num(cx)}", f"by = {num(by)}", "a = pick(f, -0.55, 0.05, 0.6)",
                    "ln = pick(f, 0.62, 0.86, 0.66)", "c = pick(f, -0.4, 0.15, 0.45)",
                    f"sx = {stem[0]}", f"sy = {stem[1]}", "th = a + c * v",
                    "len = 0.15 * ln * sin(pi * (0.08 + 0.88 * v)) * (1 - 0.45 * v)",
                    "phi = th + side * (1.15 - 0.7 * w)", "lx = sx + len * w * sin(phi)", "ly = sy - len * w * cos(phi)",
                    f"grow = {num(start + 2)} + v * {num(end - start - 8)} + 1.4 * w"],
            "samples": 5,
            "px": "lx + len * 0.3 * (1 - w) * s * sin(phi + sub * 0.95)",
            "py": "ly - len * 0.3 * (1 - w) * s * cos(phi + sub * 0.95)",
            "width": 0.0012, "color": "alpha(mix(#2FB7B0, #A8F7FF, v), 0.85)",
            "draw": "clamp((t - grow) / 0.9, 0, 1)"}, (start + 3, end + 1)),
    ]
    return fronds


def harmonograph():
    return [
        ("Two damped pendulums, swinging at nearly the same rate.", {
            "kind": "lines", "samples": 3200,
            "px": f"{num(CX)} + 0.36 * exp(-s * 2.2) * sin(s * 92 + 0.4) + 0.05 * exp(-s * 1.6) * sin(s * 61)",
            "py": f"{num(CY)} + 0.36 * exp(-s * 2.0) * sin(s * 88.4 + 1.3)",
            "width": 0.0018, "color": "#C9A7FF", "draw": prog(7, 21)}, (7, 21)),
        ("A third, faster and fainter, crossing the first.", {
            "kind": "lines", "samples": 2400,
            "px": f"{num(CX)} + 0.24 * exp(-s * 1.8) * sin(s * 70 + 2.1)",
            "py": f"{num(CY)} + 0.24 * exp(-s * 1.7) * sin(s * 71.3)",
            "width": 0.0016, "color": "#5EF2E0", "draw": prog(16, 28)}, (16, 28)),
    ]


def spirograph():
    return [
        ("A wheel of 3 rolling inside a ring of 5, its pen at 5.", {
            "kind": "lines", "samples": 2400,
            "px": f"{num(CX)} + 0.062 * (2 * cos(s * tau * 3) + 5 * cos(s * tau * 2))",
            "py": f"{num(CY)} + 0.062 * (2 * sin(s * tau * 3) - 5 * sin(s * tau * 2))",
            "width": 0.0022, "color": "#FFC86B", "draw": prog(7, 20)}, (7, 20)),
        ("A rose of seven petals inside it.", {
            "kind": "lines", "samples": 1800,
            "px": f"{num(CX)} + 0.17 * cos(7 / 3 * s * tau * 3) * cos(s * tau * 3)",
            "py": f"{num(CY)} + 0.17 * cos(7 / 3 * s * tau * 3) * sin(s * tau * 3)",
            "width": 0.002, "color": "#F25FD0", "draw": prog(17, 28)}, (17, 28)),
    ]


def knot():
    return [
        ("A torus knot, three times around and seven through, turning as it's drawn.", {
            "kind": "lines", "samples": 2000,
            "let": ["spin = t * 0.22"],
            "px": f"{num(CX)} + 0.12 * ((2 + cos(7 * s * tau)) * cos(3 * s * tau) * cos(spin) + sin(7 * s * tau) * sin(spin))",
            "py": f"{num(CY)} + 0.12 * (2 + cos(7 * s * tau)) * sin(3 * s * tau)",
            "width": 0.0026, "color": "mix(#F25FD0, #56D4F5, 0.5 + 0.5 * sin(t * 0.4))",
            "draw": prog(7, 26)}, (7, 26)),
    ]


def sunflower():
    return [
        ("Nine hundred seeds, each turned by the golden angle from the last.", {
            "kind": "rect", "count": 900,
            "x": f"{num(CX)} + 0.4 * sqrt(k) * cos(i * 2.39996)",
            "y": f"{num(CY)} + 0.4 * sqrt(k) * sin(i * 2.39996)",
            "w": "0.0034 + 0.0055 * k", "h": "0.0034 + 0.0055 * k", "rotate": "i * 2.39996",
            "color": "hsv(0.42 - 0.3 * k, 0.6, 1)",
            "visible": "i < (t - 7) * 45"}, (7, 27)),
    ]


def revelation():
    shards = ("Panes of folded light, unfolding one after another.", {
        "kind": "lines", "count": 9, "samples": 4, "closed": True,
        "let": ["r = 0.09 + 0.1 * hash(i * 3.1)", "ang = hash(i * 7.7) * tau + t * 0.04 * (hash(i) - 0.5)",
                "ox = 0.3 + 0.36 * hash(i * 5.3)", "oy = -0.3 + 0.36 * hash(i * 2.9)"],
        "px": "ox + r * cos(ang + j * tau / 4 + 0.3 * sin(j * 2.1 + i))",
        "py": "oy + r * 0.8 * sin(ang + j * tau / 4 + 0.3 * sin(j * 2.1 + i))",
        "width": 0.0014, "color": "alpha(#C9A7FF, 0.8)", "fill": "alpha(#7B4BFF, 0.09)",
        "draw": "clamp((t - 7 - i * 1.2) / 1.4, 0, 1)"}, (7, 19))
    return [shards] + fern(start=15, end=29, cx=0.18, by=0.47)


PAGES = [
    ("I", "The Fern", fern()),
    ("II", "The Harmonograph", harmonograph()),
    ("III", "The Spirograph", spirograph()),
    ("IV", "The Knot", knot()),
    ("V", "The Sunflower", sunflower()),
    ("VI", "The Revelation", revelation()),
]


# ---------------------------------------------------------------------------------------------------------------
# TOML, written once and shown as written.


def value(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return num(v)
    if isinstance(v, list):
        return "[" + ", ".join(q(x) for x in v) + "]"
    return q(v)


def element_lines(comment, fields, header):
    lines = [f"# {comment}", header]
    for key, v in fields.items():
        lines.append(f"{key} = {value(v)}")
    return lines


def figure_code(number, title, elements, header):
    lines = [f"# Page {number}: {title}"]
    blocks = []
    for comment, fields, window in elements:
        block = element_lines(comment, fields, header)
        blocks.append((len(lines), len(lines) + len(block), window))
        lines += block
        lines.append("")
    return lines[:-1], blocks


# ---------------------------------------------------------------------------------------------------------------
# The code as shown: each line wrapped and split into colored pieces, each piece a text element.


def pieces(line):
    """(text, kind) pieces of a TOML line."""
    if line.startswith("#"):
        return [(line, "comment")]
    if line.startswith("["):
        return [(line, "header")]
    if " = " in line:
        key, val = line.split(" = ", 1)
        kind = "string" if val.startswith('"') or val.startswith("[") else "number"
        return [(key + " = ", "key"), (val, kind)]
    return [(line, "string")]


def wrapped(line):
    """Display rows of a line: (column, text, kind) pieces each, long values broken across rows."""
    rows = []
    col = 0
    row = []
    for text, kind in pieces(line):
        while text:
            room = WRAP - col
            if len(text) <= room:
                row.append((col, text, kind))
                col += len(text)
                text = ""
            else:
                cut = text.rfind(" ", 0, room)
                cut = room if cut <= 0 else cut + 1
                row.append((col, text[:cut], kind))
                rows.append(row)
                row, col, text = [], 4, text[cut:]
    rows.append(row)
    return rows


def code_elements(lines, blocks, live, indent):
    """Text elements for the code, as rows; `live` lights each block while it draws and dims the rest."""
    out = []
    row = 0
    for n, line in enumerate(lines):
        window = next((w for a, b, w in blocks if a <= n < b), None)
        for pieces_ in wrapped(line):
            for col, text, kind in pieces_:
                if not text.strip():
                    continue
                base = CODE[kind]
                if live and window:
                    color = f"t >= {num(window[0])} && t < {num(window[1])} ? {HIGHLIGHT} : alpha({base}, codeop)"
                elif live:
                    color = f"alpha({base}, codeop)"
                else:
                    color = base
                out.append(text_element(text, CODE_X + col * ADV, CODE_Y + row * STEP, color, indent))
            row += 1
    return out, row


def text_element(text, x, y, color, indent):
    pad = "  " * indent
    hdr = "[[scene" + ".draw" * (indent + 1) + "]]"
    return "\n".join([f"{pad}{hdr}", f'{pad}kind = "text"', f"{pad}text = {q(text)}", f"{pad}size = {num(SIZE)}",
                      f"{pad}x = {num(x)}", f"{pad}y = {num(y)}", f"{pad}color = {q(color)}"]) + "\n"


def block(fields, indent, header_override=None):
    pad = "  " * indent
    hdr = header_override or ("[[scene" + ".draw" * (indent + 1) + "]]")
    return "\n".join([f"{pad}{hdr}"] + [f"{pad}{k} = {value(v)}" for k, v in fields.items()]) + "\n"


# ---------------------------------------------------------------------------------------------------------------


def page_scene(k):
    number, title, elements = PAGES[k]
    nxt = PAGES[(k + 1) % len(PAGES)]
    fig_header = "[[scene.draw.draw.draw.draw]]"
    lines, blocks = figure_code(number, title, elements, fig_header)
    next_lines, next_blocks = figure_code(nxt[0], nxt[1], nxt[2], fig_header)

    # The code's scroll: none until the figure starts, then keeping the lit block in view.
    shown, rows = code_elements(lines, blocks, True, 2)
    _, total_rows = shown, rows
    scroll = "0"
    if total_rows > VISIBLE_ROWS:
        over = (total_rows - VISIBLE_ROWS) * STEP
        scroll = f"-{num(over)} * smoothstep({num(DIM[1])}, {num(HAND_IN[0] - 6)}, t)"
    under, _ = code_elements(next_lines, next_blocks, False, 2)

    lets = [
        # The code dims as the figure draws, and fades from the page as the hand takes it: text isn't carried round
        # the curl, so it's gone before the fold comes near it.
        f"codeop = t < {num(DIM[0])} ? 1 : mix(1, 0.3, smoothstep({num(DIM[0])}, {num(DIM[1])}, t)) * "
        f"(1 - smoothstep({num(TURN[0])}, {num(TURN[0] + 0.6)}, t))",
        f"sky = smoothstep({num(DIM[0])}, {num(DIM[1] + 2)}, t)",
        # The fold: well off the screen before and after the turn, and between, sweeping from the corner the hand
        # holds to past the far one.
        f"fold = t < {num(TURN[0])} ? -4 : t >= {num(TURN[1])} ? 4 : interp(t, {keys(FOLD_KEYS)})",
        f"dm = max(0, fold - {num(CORNER)})",
        f"rr = interp(fold, {num(CORNER - 0.02)}, {num(CURL_R[0])}, -0.7, {num(CURL_R[1])})",
        f"bend = interp(fold, {keys(BEND_KEYS)})",
        f"near = interp(fold, {keys(NEAR_KEYS)})",
        # The page's corner, where the hand holds it, carried round the curl like every other point.
        "cq = dm", "ca = min(cq / rr, bend)", "cr = max(0, cq - bend * rr)",
        f"cm = {num(CORNER)} + cq - rr * sin(ca) - cr * cos(bend)",
        "cz = rr * (1 - cos(ca)) + cr * sin(bend)",
        f"ck = min({num(K_MAX)}, 1 + near * cz)",
        f"ct = {num(-X0 * TX - Y0 * TY)}",
        # The figure's light fades as the hand takes the page (a glow on a moving page would be blurred afresh every
        # frame, which costs more than the rest of the turn together).
        f"gl = 1 - smoothstep({num(TURN[0] - 0.4)}, {num(TURN[0] + 0.6)}, t)",
        f"reach = t < {num(HAND_IN[0])} ? 1 : 1 - ease_out((t - {num(HAND_IN[0])}) / {num(HAND_IN[1] - HAND_IN[0])})",
    ]
    out = [f"""[[scene]]
name = "Page {number}: {title}"
duration = {num(DUR)}
fade = 0
let = {value(lets)}

# Beneath the page: the next page's code, shown where the page has been pulled away. (Clipping this, rather than
# covering it with the page, saves drawing a whole screen of page through a mask.)
[[scene.draw]]
kind = "group"
visible = "t >= {num(TURN[0])}"
rotate = {num(THETA)}
clip = ["-9", "-9", "fold", "9"]

  [[scene.draw.draw]]
  kind = "group"
  rotate = {num(-THETA)}

""" + "\n".join(under)]

    figure = []
    turning = []
    for comment, fields, _ in elements:
        figure.append(f"      # {comment}\n" + block(fields, 3, "[[scene.draw.draw.draw]]"))
        turning.append(block(warped(fields), 2, "[[scene.draw.draw]]"))

    # The page's sky: stars, and a line of horizon, the same before the turn and carried along by it. Both stay within
# the page, so nothing of them shows past its edges as it's lifted.
    stars = {"kind": "rect", "count": 160, "x": "(hash(i * 1.37) * 2 - 1) * 0.88", "y": "(hash(i * 4.11) * 2 - 1) * 0.5",
             "w": "0.002 + 0.002 * hash(i * 9.1)", "h": "0.002 + 0.002 * hash(i * 9.1)",
             "color": "alpha(#FFFFFF, sky * (0.25 + 0.55 * noise(t * 0.25 + i * 3.3)))"}
    horizon = {"kind": "lines", "samples": 48, "px": f"{num(X0)} + {num(2 * X1)} * s", "py": "0.13", "width": 0.0016,
               "color": "alpha(#F2A65A, sky * 0.55)"}

    out.append(f"""# The page as written, until the hand takes it: its sky,
[[scene.draw]]
kind = "group"
visible = "t < {num(TURN[0])}"

""" + block(stars, 1) + "\n" + block(horizon, 1) + f"""glow = "1.5 * gl"
glow_radius = 0.012

# its code, which fades as the hand takes the page (text isn't carried round the curl, so it's gone before the fold
# comes near it),
[[scene.draw]]
kind = "group"
visible = "t < {num(TURN[0] + 0.6)}"
y = "{scroll}"

""" + "\n".join(shown).replace("[[scene.draw.draw.draw]]", "[[scene.draw.draw]]") + f"""
# and the figure, drawn over its code.
[[scene.draw]]
kind = "group"
visible = "t < {num(TURN[0])}"
glow = "1.4 * gl"
glow_radius = 0.006

""" + "\n".join(figure).replace("      #", "  #").replace("[[scene.draw.draw.draw]]", "[[scene.draw.draw]]"))

    # The curl's surface, in strips across the page, each lit by the way it faces. q runs from the fold to the
    # corner: round the cylinder (front, then back), then straight on.
    def strips(name, count, q0, q1, fill, comment):
        lets_ = [f"qa = {q0}", f"qb = {q1}",
                 "q0 = mix(qa, qb, i / n)", "q1 = min(qb, mix(qa, qb, (i + 1) / n) + 0.003)"]
        for e in (0, 1):
            lets_ += [f"c{e} = fold - q{e}",
                      f"ax{e} = ({num(X0)} - {num(NX)} * c{e}) / {num(TX)}", f"bx{e} = ({num(X1)} - {num(NX)} * c{e}) / {num(TX)}",
                      f"ay{e} = ({num(Y0)} - {num(NY)} * c{e}) / {num(TY)}", f"by{e} = ({num(Y1)} - {num(NY)} * c{e}) / {num(TY)}",
                      f"lo{e} = max(min(ax{e}, bx{e}), min(ay{e}, by{e}))",
                      f"hi{e} = max(lo{e}, min(max(ax{e}, bx{e}), max(ay{e}, by{e})))",
                      f"a{e} = min(q{e} / rr, bend)", f"r{e} = max(0, q{e} - bend * rr)",
                      f"m{e} = fold - rr * sin(a{e}) - r{e} * cos(bend)",
                      f"k{e} = min({num(K_MAX)}, 1 + near * (rr * (1 - cos(a{e})) + r{e} * sin(bend)))"]
        lets_ += ["am = (a0 + a1) / 2"]
        corner = lambda m, tau, k, nn, tt: f"({num(nn)} * {m} + {num(tt)} * {tau}) * {k}"
        cs = (("m0", "lo0", "k0"), ("m0", "hi0", "k0"), ("m1", "hi1", "k1"), ("m1", "lo1", "k1"))
        px = "pick(j, " + ", ".join(corner(m, tau, k, NX, TX) for m, tau, k in cs) + ")"
        py = "pick(j, " + ", ".join(corner(m, tau, k, NY, TY) for m, tau, k in cs) + ")"
        return f"""# {comment}
[[scene.draw]]
kind = "lines"
count = {count}
visible = "t >= {num(TURN[0])} && t < {num(TURN[1])} && {q1} > {q0} + 0.0005"
let = {value(lets_)}
samples = 4
closed = true
px = "{px}"
py = "{py}"
width = 0
fill = "{fill}"
"""

    edge_on = "min(dm, rr * 1.5708)"
    round_ = "min(dm, rr * bend)"
    out.append(strips("front", FRONT_STRIPS, "0", edge_on, f"mix({BG}, #222A4A, 0.7 * sin(am))",
                      "The curl's front, rising toward us: the page itself, which its figure is drawn on next."))
    out.append(f"""# The shadow the curl casts on what it uncovers.
[[scene.draw]]
kind = "lines"
visible = "t >= {num(TURN[0])} && t < {num(TURN[1])}"
samples = 2
px = "{num(NX)} * (fold - rr - 0.03) + {num(TX)} * (s * 6 - 3)"
py = "{num(NY)} * (fold - rr - 0.03) + {num(TY)} * (s * 6 - 3)"
width = 0.06
color = "alpha(#000000, 0.35 * min(1, dm * 8))"

# The page, carried by the turn: flat beyond the fold, round the curl and on toward us before it.
[[scene.draw]]
kind = "group"
visible = "t >= {num(TURN[0])} && t < {num(TURN[1])}"

""" + block(warped(stars), 1) + "\n" + block(warped(horizon), 1) + f"""glow = "1.5 * gl"
glow_radius = 0.012

  # The figure, carried along with its page.
  [[scene.draw.draw]]
  kind = "group"
  glow = "1.4 * gl"
  glow_radius = 0.006

""" + "\n".join(t.replace("[[scene.draw.draw]]", "    [[scene.draw.draw.draw]]") for t in turning) + f"""
# The lifted page's shadow, falling on the page beyond the fold.
[[scene.draw]]
kind = "lines"
count = 3
visible = "t >= {num(TURN[0])} && t < {num(TURN[1])}"
samples = 2
px = "{num(NX)} * (fold + 0.012 + i * 0.022) + {num(TX)} * (s * 6 - 3)"
py = "{num(NY)} * (fold + 0.012 + i * 0.022) + {num(TY)} * (s * 6 - 3)"
width = "0.022 + i * 0.012"
color = "alpha(#000000, (0.34 - i * 0.1) * min(1, dm * 8))"
""")
    back = "mix(#4E4668, #F1EBFA, 0.42 + 0.55 * cos(am - 0.75))"
    out.append(strips("back", BACK_STRIPS, edge_on, round_, back,
                      "The curl past edge-on, where the page shows its back."))
    out.append(strips("flat", FLAT_STRIPS, round_, "dm", back,
                      "The rest of the lifted page, its back to us, rising straight on toward us."))
    out.append(f"""# The Hand of Omnipotence: it reaches in, takes the page's corner, and pulls the page up and away.
[[scene.draw]]
kind = "sprite"
file = "hands.png"
frame_size = [{HAND_FRAME[0]}, {HAND_FRAME[1]}]
anchor = [{HAND_PINCH[0]}, {HAND_PINCH[1]}]
smooth = true
visible = "t >= {num(HAND_IN[0])} && t < {num(TURN[1])}"
frame = "t < {num(TURN[0] - 0.25)} ? 0 : 1"
x = "({num(NX)} * cm + {num(TX)} * ct) * ck + 0.45 * reach"
y = "({num(NY)} * cm + {num(TY)} * ct) * ck + 0.4 * reach"
pixel = "{num(HAND_PIXEL)} * min(1.8, ck)"
""")
    return "\n".join(out)


def build(out, hand_src):
    os.makedirs(out, exist_ok=True)
    import hands
    stale = os.path.join(out, "hand.png")
    if os.path.exists(stale):
        os.remove(stale)
    size, pinch = hands.sheet(hand_src, os.path.join(out, "hands.png"))
    assert (size, pinch) == (HAND_FRAME, HAND_PINCH), (size, pinch)
    head = f'''# Omnipotence: each figure is drawn over the very lines of this file that draw it, and the Hand of Omnipotence
# turns the page to the next. Written by tools/omnipotence/build.py; the hand is released with the pack under its
# MIT license.
format = 1
name = "Omnipotence"
author = "ScreenSafe"
license = "MIT"
description = "Figures drawn over the very code that draws them, each line lighting up as it works, until the Hand of Omnipotence turns the page."
preview = {num(TURN[0] + 2.2)}
fps = 30
fade = 0
background = "{BG}"
'''
    scenes = [page_scene(k) for k in range(len(PAGES))]
    with open(os.path.join(out, "screensafe.toml"), "w") as f:
        f.write(head + "\n" + "\n".join(scenes))


if __name__ == "__main__":
    build(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "hand-sheet.png"))
