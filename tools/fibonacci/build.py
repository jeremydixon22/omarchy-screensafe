"""Builds the Fibonacci pack: six views of the Fibonacci numbers, one after another, in the theme's colors.

    python3 build.py OUT_DIR

Each view is a scene, told in pictures rather than words: squares tiling a golden rectangle with the spiral through
them, seen from the first square outward; a sunflower opening, its seeds' 21 and 34 spirals lighting up; generations
of Fibonacci's rabbits; Pascal's triangle with its shallow diagonals summing to bars; the ratios closing in on the
golden ratio beside a rectangle of each ratio's shape; and the Fibonacci word fractal. Everything is worked out here,
exactly, and written as data and simple expressions.
"""
import math
import os
import sys

DUR = 30.0
X0, Y0 = -8 / 9, -0.5


def num(v):
    out = f"{v:.5f}".rstrip("0").rstrip(".")
    return "0" if out in ("", "-0") else out


def q(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def fib(n):
    a, b = 0, 1
    out = []
    for _ in range(n):
        out.append(a)
        a, b = b, a + b
    return out


def element(fields, comment=None):
    lines = [f"# {comment}"] if comment else []
    lines.append("[[scene.draw]]")
    for k, v in fields.items():
        if isinstance(v, bool):
            v = "true" if v else "false"
        elif isinstance(v, (int, float)):
            v = num(v)
        elif isinstance(v, list):
            v = "[" + ", ".join(fmt(x) for x in v) + "]"
        else:
            v = q(v)
        lines.append(f"{k} = {v}")
    return "\n".join(lines) + "\n"


def fmt(x):
    if isinstance(x, (list, tuple)):
        return "[" + ", ".join(fmt(y) for y in x) + "]"
    return num(x) if isinstance(x, (int, float)) else q(x)


def scene(name, body, lets=None):
    head = f'[[scene]]\nname = "{name}"\nduration = {num(DUR)}\nfade = 1.2\n'
    if lets:
        head += "let = [" + ", ".join(q(x) for x in lets) + "]\n"
    return head + "\n" + "\n".join(body)


def ellipse(cx, cy, rx, ry, rot=0.0):
    """px and py of an ellipse, centered on (cx, cy), turned by rot."""
    c, s_ = math.cos(rot), math.sin(rot)
    px = f"{num(cx)} + {num(rx * c)} * cos(s * tau) - {num(ry * s_)} * sin(s * tau)"
    py = f"{num(cy)} + {num(rx * s_)} * cos(s * tau) + {num(ry * c)} * sin(s * tau)"
    return px, py


def blob(cx, cy, rx, ry, color, rot=0.0, samples=24):
    px, py = ellipse(cx, cy, rx, ry, rot)
    return {"kind": "lines", "samples": samples, "closed": True, "px": px, "py": py, "width": 0, "fill": color}


def nested(fields, depth, comment=None):
    """An element written as a child `depth` levels down."""
    text = element(fields, comment)
    return text.replace("[[scene.draw]]", "[[scene" + ".draw" * (depth + 1) + "]]")


# ---------------------------------------------------------------------------------------------------------------
# 1. Squares of Fibonacci sides tiling a golden rectangle, and the spiral through them, seen from the first tiny
# square outward: the view draws back as each square is laid.


def squares():
    sizes = fib(12)[1:]                       # 1 1 2 3 5 8 13 21 34 55 89
    rect = [0, 0, 1, 1]                       # x0, y0, x1, y1, y up
    placed = [(0, 0, 1)]
    rects = [tuple(rect)]
    for k, s in enumerate(sizes[1:]):
        x0, y0, x1, y1 = rect
        d = k % 4
        if d == 0:
            sq = (x1, y0, s)
            rect = [x0, y0, x1 + s, y1]
        elif d == 1:
            sq = (x0, y1, s)
            rect = [x0, y0, x1, y1 + s]
        elif d == 2:
            sq = (x0 - s, y0, s)
            rect = [x0 - s, y0, x1, y1]
        else:
            sq = (x0, y0 - s, s)
            rect = [x0, y0 - s, x1, y1]
        placed.append(sq)
        rects.append(tuple(rect))

    def corners(sq):
        x, y, s = sq
        return [(x, y), (x + s, y), (x + s, y + s), (x, y + s)]

    # The spiral: in each square a quarter circle, each starting where the last ended, turning the same way.
    def arcs_from(first_center, first_start):
        out, end = [], None
        for k, sq in enumerate(placed):
            cs = corners(sq)
            s = sq[2]
            options = [first_center] if k == 0 else cs
            for c in options:
                start = first_start if k == 0 else end
                if abs(math.dist(c, start) - s) > 1e-9 or start not in cs:
                    continue
                a0 = math.atan2(start[1] - c[1], start[0] - c[0])
                a1 = a0 + math.pi / 2
                e = (round(c[0] + s * math.cos(a1)), round(c[1] + s * math.sin(a1)))
                if e in cs and e != start:
                    out.append((c, s, a0))
                    end = e
                    break
            else:
                return None
        return out

    arcs = None
    for c in corners(placed[0]):
        for st in corners(placed[0]):
            if abs(math.dist(c, st) - 1) < 1e-9:
                arcs = arcs_from(c, st)
                if arcs:
                    break
        if arcs:
            break
    assert arcs, "the spiral must run through every square"
    x0, y0, x1, y1 = rect
    scale = 0.86 / max(y1 - y0, (x1 - x0) / (16 / 9))
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    to = lambda x, y: ((x - cx) * scale, -(y - cy) * scale)

    # The view: at each square's moment, the rectangle laid so far fills most of the screen. Zoom is kept as its
    # logarithm between moments, so drawing back feels steady.
    at = lambda k: 2.0 + k * 2.2
    zoom_keys, x_keys, y_keys = [], [], []
    for k, (rx0, ry0, rx1, ry1) in enumerate(rects):
        (ax, ay), (bx, by) = to(rx0, ry0), to(rx1, ry1)
        w, h = abs(bx - ax), abs(by - ay)
        z = min(80, 0.62 / h, 0.62 * 16 / 9 / w)    # a group's scale goes up to 100
        zoom_keys += [at(k) + 0.6, math.log(z)]
        x_keys += [at(k) + 0.6, (ax + bx) / 2]
        y_keys += [at(k) + 0.6, (ay + by) / 2]
    lets = [f"zoom = exp(interp(t, {', '.join(num(v) for v in zoom_keys)}))",
            f"fx = interp(t, {', '.join(num(v) for v in x_keys)})",
            f"fy = interp(t, {', '.join(num(v) for v in y_keys)})"]
    body = [element({"kind": "group", "scale": "zoom", "x": "-fx * zoom", "y": "-fy * zoom"},
                    "The view, drawing back from the first square.")]
    tints = ["accent", "cyan", "magenta", "yellow", "green", "blue"]
    for k, (sq, (c, s, a0)) in enumerate(zip(placed, arcs)):
        when = at(k)
        (ax, ay), (bx, by) = to(sq[0], sq[1]), to(sq[0] + s, sq[1] + s)
        body.append(nested({"kind": "rect", "x": (ax + bx) / 2, "y": (ay + by) / 2, "w": abs(bx - ax), "h": abs(by - ay),
                            "color": f"alpha({tints[k % len(tints)]}, 0.1 * smoothstep({num(when + 0.3)}, {num(when + 1.4)}, t))"}, 1))
        pts = [list(to(*p)) for p in corners(sq)]
        body.append(nested({"kind": "lines", "points": pts, "closed": True, "width": "0.0024 / zoom",
                            "color": "alpha(foreground, 0.5)", "draw": f"ease_in_out((t - {num(when)}) / 0.9)"}, 1))
        ccx, ccy = to(*c)
        r = s * scale
        body.append(nested({"kind": "lines", "samples": 48, "width": "0.0045 / zoom", "color": "accent", "glow": 1.5,
                            "px": f"{num(ccx)} + {num(r)} * cos({num(a0)} + s * pi / 2)",
                            "py": f"{num(ccy)} - {num(r)} * sin({num(a0)} + s * pi / 2)",
                            "draw": f"clamp((t - {num(when + 0.7)}) / 1.1, 0, 1)"}, 1))
    return scene("Squares and the spiral", body, lets)


# ---------------------------------------------------------------------------------------------------------------
# 2. A sunflower: its petals open as its seeds fill in, each turned by the golden angle from the last; then its 21
# spirals one way light up, and its 34 the other.


def sunflower():
    golden = math.pi * (3 - math.sqrt(5))
    petals = []
    for layer, (count, length, offset, color) in enumerate(((21, 0.2, 0.0, "alpha(orange, 0.85)"),
                                                            (34, 0.17, 0.5, "yellow"))):
        petals.append(element({
            "kind": "lines", "count": count, "samples": 20, "closed": True, "width": 0,
            "let": [f"ang = (i + {offset}) * tau / n + 0.1 * t / {num(DUR)}",
                    f"open = ease_out((t - {num(0.6 + layer * 0.5)} - hash(i * 3.7) * 1.5) / 3)",
                    f"len = {num(length)} * open * (0.9 + 0.2 * hash(i * 5.1))"],
            "sample_let": ["along = 0.42 + len * 0.5 * (1 - cos(s * tau))", "across = len * 0.16 * sin(s * tau)"],
            "px": "along * cos(ang) - across * sin(ang)", "py": "along * sin(ang) + across * cos(ang)",
            "fill": color}, "Petals, opening." if layer == 0 else None))
    seeds = element({
        "kind": "rect", "count": 1000,
        "x": f"0.43 * sqrt((i + 0.5) / n) * cos(i * {num(golden)})",
        "y": f"0.43 * sqrt((i + 0.5) / n) * sin(i * {num(golden)})",
        "w": "0.007 + 0.006 * sqrt(k)", "h": "0.007 + 0.006 * sqrt(k)", "rotate": f"i * {num(golden)}",
        "visible": "i < (t - 1.5) * 90",
        "color": "t < 12 ? mix(brown, foreground, 0.25 + 0.3 * k) : t < 20.5 ? pick(mod(i, 21), accent, mix(brown, foreground, 0.2)) "
                 ": pick(mod(i, 34), cyan, mix(brown, foreground, 0.2))",
    }, "Each seed turned from the last by the golden angle, 137.5 degrees.")
    disc = element(blob(0, 0, 0.445, 0.445, "mix(background, brown, 0.35)", samples=64), "The flower's head.")
    return scene("The sunflower", petals + [disc, seeds])


# ---------------------------------------------------------------------------------------------------------------
# 3. Fibonacci's rabbits: each grown pair has a young pair every month; the young grow up in a month.


RABBIT = [  # parts of a rabbit about a unit long, sitting and facing right: (cx, cy, rx, ry, turn, shade)
    (0.0, 0.0, 0.5, 0.36, 0.0, 0.0),      # body
    (-0.52, -0.06, 0.12, 0.11, 0.0, 0.6), # tail
    (0.36, -0.68, 0.075, 0.25, -0.25, 0.0),  # far ear
    (0.5, -0.7, 0.075, 0.26, 0.15, 0.15),    # near ear
    (0.46, -0.28, 0.25, 0.21, 0.0, 0.1),  # head
    (0.58, -0.32, 0.04, 0.04, 0.0, -1.0), # eye
]


def rabbit_parts(base, depth):
    out = []
    for cx, cy, rx, ry, turn, shade in RABBIT:
        color = "background" if shade < 0 else f"mix({base}, white, {num(shade)})" if shade else base
        out.append(nested(blob(cx, cy, rx, ry, color, turn, samples=18), depth))
    return out


def rabbits():
    months = 8
    gens = [["Y"]]
    links = []
    for m in range(1, months):
        row = []
        for p, kind in enumerate(gens[-1]):
            kids = ["A", "Y"] if kind == "A" else ["A"]
            for kid in kids:
                links.append((m - 1, p, m, len(row)))
                row.append(kid)
        gens.append(row)
    top, step, width = -0.38, 0.108, 1.62
    xs = lambda m, i: -width / 2 + width * (i + 0.5) / len(gens[m])
    y = lambda m: top + m * step
    start, gap = 2.0, 3.0
    verts = []
    edges = []
    for m0, p, m1, c in links:
        verts += [[xs(m0, p), y(m0), 0], [xs(m1, c), y(m1), 0]]
        edges.append([len(verts) - 2, len(verts) - 1])
    # The links of each month draw in as that month comes.
    order = [m1 for _, _, m1, _ in links]
    keys = []
    for m in range(1, months):
        last = max(k for k, mm in enumerate(order) if mm <= m) + 1
        keys += [start + m * gap - 0.8, (last - sum(1 for mm in order if mm == m)) / len(order),
                 start + m * gap - 0.2, last / len(order)]
    body = [element({
        "kind": "mesh", "vertices": verts, "edges": edges, "size": 1, "distance": 0, "width": 0.0016,
        "color": "alpha(foreground, 0.25)", "draw": "interp(t, " + ", ".join(num(v) for v in keys) + ")"},
        "Each month's pairs, joined to the pairs they came from.")]
    for m, row in enumerate(gens):
        at = start + m * gap
        for kind in ("A", "Y"):
            slots = [xs(m, i) for i, k in enumerate(row) if k == kind]
            if not slots:
                continue
            size, base = (0.05, "accent") if kind == "A" else (0.032, "cyan")
            x = num(slots[0]) if len(slots) == 1 else "pick(i, " + ", ".join(num(v) for v in slots) + ")"
            # Each pair hops in as it's born, and now and then hops where it sits.
            hop = (f"0.05 * sin(pi * clamp((t - {num(at)} - k * 0.25) / 0.45, 0, 1)) + "
                   f"0.012 * max(0, sin(t * 1.7 + i * 2.39 + {m})) ^ 16")
            body.append(element({
                "kind": "group", "count": len(slots), "x": x, "y": f"{num(y(m) + size * 0.3)} - {hop}",
                "scale": size, "visible": f"t >= {num(at)} + k * 0.25",
            }, f"Month {m + 1}: {'grown pairs, each having young' if kind == 'A' else 'young pairs, grown next month'}."))
            # A pair: two rabbits, one a little behind the other.
            body.append(nested({"kind": "group", "count": 2, "x": "-0.45 * i", "y": "-0.12 * i",
                                "opacity": "1 - 0.35 * i"}, 1))
            body += rabbit_parts(base, 2)
    return scene("The rabbits", body)


# ---------------------------------------------------------------------------------------------------------------
# 4. Pascal's triangle, each number a disc as large as it is; its shallow diagonals light up in turn, and each
# one's sum grows as a bar: the Fibonacci numbers.


def pascal():
    rows = 12
    tri = [[1]]
    for _ in range(rows - 1):
        prev = tri[-1]
        tri.append([1] + [prev[k] + prev[k + 1] for k in range(len(prev) - 1)] + [1])
    cx, top, dx, dy = -0.42, -0.36, 0.078, 0.066
    start, gap = 3.0, 2.1
    lit = f"floor((t - {num(start)}) / {num(gap)})"
    body = []
    for r, row in enumerate(tri):
        sizes = [0.006 + 0.0055 * math.log(v + 1) for v in row]
        size = num(sizes[0]) if len(row) == 1 else "pick(i, " + ", ".join(num(v) for v in sizes) + ")"
        body.append(element({
            "kind": "lines", "count": len(row), "samples": 20, "closed": True, "width": 0,
            "let": [f"rad = {size} * (abs({lit} - ({r} + i)) < 0.5 ? 1.35 : 1)",
                    f"ox = {num(cx - r * dx / 2)} + i * {num(dx)}"],
            "px": "ox + rad * cos(s * tau)", "py": f"{num(top + r * dy)} + rad * sin(s * tau)",
            "fill": f"abs({lit} - ({r} + i)) < 0.5 ? accent : alpha(foreground, smoothstep(0.5, 2, t) * 0.4)"},
            "Row by row, each number a disc." if r == 0 else None))
    sums = fib(rows + 2)[1:rows + 1]
    base, full = 0.38, 0.72
    body.append(element({
        "kind": "rect", "count": rows, "x": "0.22 + i * 0.05", "w": 0.032,
        "let": [f"tall = {num(full / sums[-1])} * pick(i, {', '.join(str(v) for v in sums)})",
                f"grow = ease_out((t - {num(start)} - i * {num(gap)}) / 1.2)"],
        "h": "max(0.002, tall * grow)", "y": f"{num(base)} - max(0.002, tall * grow) / 2",
        "visible": f"t >= {num(start)} + i * {num(gap)}",
        "color": f"abs({lit} - i) < 0.5 ? accent : alpha(cyan, 0.7)"},
        "Each diagonal's sum, as a bar."))
    return scene("Pascal's triangle", body)


# ---------------------------------------------------------------------------------------------------------------
# 5. The ratio of each number to the one before it, closing in on the golden ratio, and a rectangle of each
# ratio's shape, settling into the golden rectangle.


def ratios():
    f = fib(18)[1:]
    phi = (1 + math.sqrt(5)) / 2
    pts = []
    rs = []
    for n in range(1, 16):
        r = f[n] / f[n - 1]
        rs.append(r)
        pts.append([-0.8 + (n - 1) * 0.065, 0.06 - (r - phi) * 0.55])
    start, gap = 3.0, 1.6
    shape = "interp(t, " + ", ".join(f"{num(start + n * gap)}, {num(r)}" for n, r in enumerate(rs)) + ")"
    hh, rx = 0.34, 0.5
    body = [element({"kind": "lines", "points": [[-0.84, 0.06], [0.16, 0.06]], "width": 0.0016,
                     "color": "alpha(accent, 0.45)"}, "The golden ratio, as a level the ratios close in on."),
            element({"kind": "lines", "points": pts, "width": 0.003, "color": "foreground", "glow": 1,
                     "draw": f"clamp((t - {num(start)}) / {num(gap * (len(pts) - 1))}, 0, 1)"}),
            element({"kind": "rect", "count": len(pts), "x": "-0.8 + i * 0.065",
                     "y": "pick(i, " + ", ".join(num(p[1]) for p in pts) + ")", "w": 0.012, "h": 0.012,
                     "rotate": 0.785398, "visible": f"t >= {num(start)} + i * {num(gap)}", "color": "cyan"}),
            element({"kind": "group", "let": [f"ratio = {shape}", f"rw = {num(hh)} * ratio"]},
                    "A rectangle in each ratio's shape: the square at its end is cut off, and what's left is the "
                    "same shape again."),
            nested({"kind": "rect", "x": f"{num(rx)}", "w": "rw", "h": num(hh), "color": "alpha(accent, 0.12)"}, 1),
            nested({"kind": "lines", "samples": 5, "closed": True, "width": 0.003, "color": "accent", "glow": 1,
                    "px": f"{num(rx)} + rw * (pick(j, -0.5, 0.5, 0.5, -0.5, -0.5))",
                    "py": f"{num(hh / 2)} * pick(j, -1, -1, 1, 1, -1)"}, 1),
            nested({"kind": "lines", "samples": 2, "width": 0.002, "color": "alpha(cyan, 0.8)",
                    "px": f"{num(rx)} - rw / 2 + {num(hh)}", "py": f"{num(hh / 2)} * (s * 2 - 1)"}, 1)]
    return scene("Toward the golden ratio", body)


# ---------------------------------------------------------------------------------------------------------------
# 6. The Fibonacci word fractal: walk the word, turning at each 0, left on even steps and right on odd.


def word_fractal():
    a, b = "0", "01"
    while len(b) < 1600:
        a, b = b, b + a
    word = b[:1597]
    x, y, dx, dy = 0, 0, 1, 0
    pts = [(0, 0)]
    for k, ch in enumerate(word, start=1):
        x, y = x + dx, y + dy
        pts.append((x, y))
        if ch == "0":
            dx, dy = (-dy, dx) if k % 2 == 0 else (dy, -dx)
    xs, ys = [p[0] for p in pts], [p[1] for p in pts]
    scale = 0.84 / max(max(ys) - min(ys), (max(xs) - min(xs)) / (16 / 9))
    cx, cy = (max(xs) + min(xs)) / 2, (max(ys) + min(ys)) / 2
    pts = [[(px - cx) * scale, -(py - cy) * scale] for px, py in pts]
    return scene("The word fractal", [element({
        "kind": "lines", "points": pts, "width": 0.0024, "color": "mix(accent, cyan, 0.5 + 0.5 * sin(t * 0.3))",
        "glow": 1.2, "draw": "ease_in_out((t - 1.5) / 24)"},
        "1597 steps along 0100101001001…, turning at each 0.")])


def build(out):
    os.makedirs(out, exist_ok=True)
    head = '''# Fibonacci: six views of the Fibonacci numbers, one after another, in the theme's colors. Written by
# tools/fibonacci/build.py.
format = 2
name = "Fibonacci"
author = "ScreenSafe"
license = "MIT"
description = "The sequence in pictures: squares and their spiral, a sunflower, generations of rabbits, Pascal's triangle, the golden ratio, and the Fibonacci word fractal."
preview = 52
fps = 30
fade = 0.5
colorize = "tint_color"

[[setting]]
name = "tint_color"
label = "Color"
kind = "color"
default = 0
'''
    scenes = [squares(), sunflower(), rabbits(), pascal(), ratios(), word_fractal()]
    with open(os.path.join(out, "screensafe.toml"), "w") as f:
        f.write(head + "\n" + "\n".join(scenes))


if __name__ == "__main__":
    build(sys.argv[1])
