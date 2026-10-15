"""The figures, buildings and props of Distro Wars, as sprite sheets.

Small figures are drawn as text, one character a pixel; buildings are built from isometric boxes and roofs. Each
sheet is a row of equal frames, read by the pack with `frame_size` and `frame`.
"""
from PIL import Image

from art import PAL, noise

# Colors of figures, beyond the map's.
FIG = {
    "k": (0x00, 0x00, 0x00),   # outline
    "h": (0x63, 0x39, 0x00),   # hair
    "s": (0xEF, 0x84, 0x29),   # skin
    "d": (0x94, 0x52, 0x00),   # skin in shade
    "w": (0xFF, 0xFF, 0xFF),
}
TEAMS = {
    # tunic dark, tunic, tunic light: Omarchy's green, and the rivals' red.
    "green": ((0x4F, 0x7A, 0x2A), (0x9E, 0xCE, 0x6A), (0xC3, 0xE8, 0x8D)),
    "red": ((0x6B, 0x18, 0x00), (0xAD, 0x18, 0x00), (0xD6, 0x18, 0x00)),
}
# The Omarchy mark's greens: its face, its shaded depth and its highlight.
MARK_GREEN = {"e": (0x9E, 0xCE, 0x6A), "E": (0x4F, 0x7A, 0x2A), "f": (0xC3, 0xE8, 0x8D)}
KING = {
    "g": (0xFF, 0xDE, 0x63),   # crown and trim, gold
    "G": (0xBD, 0x9C, 0x00),   # gold in shade
    "a": (0x29, 0x29, 0x39),   # armor, near black
    "A": (0x4A, 0x4A, 0x63),   # armor's highlight
    "p": (0x6B, 0x31, 0xA5),   # cape, purple
    "P": (0x42, 0x18, 0x6B),   # cape in shade
    "q": (0x94, 0x5A, 0xD6),   # cape's lit edge
}
STONE = {
    "o": (0xC6, 0xC6, 0xC6),
    "O": (0x9C, 0x9C, 0x9C),
    "m": (0x63, 0x63, 0x63),
    "M": (0x4A, 0x4A, 0x4A),
}

# A walker, facing right: standing, then three steps of the walk. Stocky, with a big head of hair, as the
# original's are.
_WALKER_TOP = """
...khhh...
..khhhhh..
.khhhhhhs.
.khhhhsks.
.khhhssss.
..khhsdd..
"""
WALKER = [_WALKER_TOP + legs for legs in (
    """
..kcbBBs..
.kcbbBBbs.
.kcbbBbbs.
..kcbbBb..
..kcbbbb..
...ks.ks..
...ks.ks..
..kkd.kd..
""",
    """
..kcbBBs..
.kcbbBBb.s
.kcbbBbb..
..kcbbBb..
..kcbbbb..
..ks...ks.
.ks.....s.
kkd....kd.
""",
    """
..kcbBBs..
.kcbbBBbs.
.kcbbBbbs.
..kcbbBb..
...kbbbb..
...kss....
...ks.s...
..kkd.d...
""",
    """
..kcbBBs..
skcbbBBb..
.kcbbBbb..
..kcbbBb..
..kcbbbb..
...ks.ks..
..ks...ks.
.kkd...kd.
""",
)]

# Cheering, after a win: arms up, then up and out.
WALKER_CHEER = [
    """
.s......s.
.s.khhh.s.
.skhhhhhs.
.khhhhhhs.
.khhhhsks.
.khhhssss.
..khhsdd..
..kcbBBb..
..kcbbBb..
..kcbbBb..
..kcbbbb..
...ks.ks..
...ks.ks..
..kkd.kd..
""",
    """
s........s
.s.khhh.s.
..khhhhhs.
.khhhhhhs.
.khhhhsks.
.khhhssss.
..khhsdd..
..kcbBBb..
..kcbbBb..
..kcbbBb..
..kcbbbb..
..ks...ks.
..ks...ks.
.kkd...kd.
""",
]

# The Omarch King, facing right: crowned, armored in black and gold, caped in purple.
KING_FRAMES = [
    """
..g.g.g..
..ggggg..
.kGgggGk.
.khhsss..
.khssss..
..kssd...
.pPaAas..
pPPaaAas.
pPaagaas.
pPaaAaas.
pPPagaa..
.pPaaaa..
..PaaAa..
..ka.ka..
..ka.ka..
..ka.ka..
.kkk.kkk.
""",
    """
..g.g.g..
..ggggg..
.kGgggGk.
.khhsss..
.khssss..
..kssd...
.pPaAas..
pPPaaAa.s
pPaagaa..
pPaaAaa..
pPPagaa..
.pPaaaa..
.pka..ka.
.ka....a.
ka.....ka
ka......a
kk......kk
""",
    """
..g.g.g..
..ggggg..
.kGgggGk.
.khhsss..
.khssss..
..kssd...
.pPaAas..
pPPaaAas.
pPaagaas.
pPaaAaas.
pPPagaa..
.pPaaaa..
..PaaAa..
..kaa....
..ka.a...
..ka.a...
.kkk.kk..
""",
    """
..g.g.g..
..ggggg..
.kGgggGk.
.khhsss..
.khssss..
..kssd...
.pPaAas..
pPPaaAask
pPaagaa..
pPaaAaa..
pPPagaa..
.pPaaaa..
.pPa.ka..
.pka..a..
.ka...ka.
.ka...ka.
kkk...kkk
""",
]

# The rally marks: the Omarchy mark, standing in green on a stone footing, and the frown of a crashed screen as
# a tombstone. The mark is Omarchy's own, from its icon.txt, a pixel to each of its strokes.
MARK = """
#############
#.....#.....#
#.#####..##.#
#.#.......#.#
#.#.......#.#
#.#.......#.#
###.......#.#
#.#.......#.#
#.#.......#.#
#.#.......#.#
#.#########.#
#.....#.....#
#######.#####
"""
# The mark at nine pixels, for icons and for the one the King carries.
MARK_SMALL = """
#########
#...#...#
#.###.#.#
#.#...#.#
###...#.#
#.#...#.#
#.#####.#
#...#...#
#####.###
"""
FROWN_STONE = """
...kkkkkk...
..kooooooOk.
.koooooooOOk
koooooooooOk
kooMooooMoOk
kooMooooMoOk
koooooooooOk
koooooooooOk
kooooMMoooOk
koooMooMooOk
kooMooooMoOk
koooooooooOk
.kOOOOOOOOOk
..kmmmmmmmmk
...kkkkkkkk.
"""


def mark_image(text, footing=0, pole=0):
    """The mark in green, lit along its top and given a pixel of shaded depth below and to the right; on a stone
    `footing` rows deep, or held up on a `pole`."""
    rows = text.strip("\n").split("\n")
    n = len(rows)
    on = lambda x, y: 0 <= y < n and 0 <= x < len(rows[y]) and rows[y][x] == "#"
    w, h = n + 2, n + 1 + footing + pole
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    px = img.load()
    for y in range(n):
        for x in range(n):
            if on(x, y) and not on(x + 1, y + 1):
                px[x + 1, y + 1] = MARK_GREEN["E"] + (255,)
    for y in range(n):
        for x in range(n):
            if on(x, y):
                px[x, y] = MARK_GREEN["f" if y == 0 else "e"] + (255,)
    for y in range(n + 1, n + 1 + footing):
        for x in range(w):
            if x == 0 or y == h - 1:
                c = FIG["k"]
            else:
                c = STONE["o"] if x < w // 2 and y == n + 1 else STONE["O"] if x < w - 2 else STONE["m"]
            px[x, y] = c + (255,)
    for y in range(n, n + pole + 1):
        px[n // 2, y] = (0x63, 0x39, 0x00, 255)
        px[n // 2 + 1, y] = (0x42, 0x29, 0x00, 255)
    return img


def grid(text, colors, team=None):
    """A sprite from text: each character a pixel, '.' clear."""
    rows = [r for r in text.strip("\n").split("\n")]
    w, h = max(len(r) for r in rows), len(rows)
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    px = img.load()
    keys = dict(FIG)
    keys.update(KING)
    keys.update(STONE)
    keys.update(colors)
    if team:
        dark, mid, light = TEAMS[team]
        keys.update({"b": mid, "B": light, "c": dark})
    for y, row in enumerate(rows):
        for x, ch in enumerate(row):
            if ch != "." and ch in keys:
                px[x, y] = keys[ch] + (255,)
    return img


def sheet(frames, size=None):
    """Frames side by side, each in a cell of `size` (the largest frame's, by default), aligned at the bottom
    middle."""
    if size is None:
        size = (max(f.width for f in frames), max(f.height for f in frames))
    out = Image.new("RGBA", (size[0] * len(frames), size[1]), (0, 0, 0, 0))
    for k, f in enumerate(frames):
        out.alpha_composite(f, (k * size[0] + (size[0] - f.width) // 2, size[1] - f.height))
    return out


def walkers(team):
    """Standing, three steps of the walk, and two of a cheer."""
    return sheet([grid(f, {}, team) for f in WALKER + WALKER_CHEER], (10, 14))


# The King cheering, his free arm raised; the other holds his mark aloft.
KING_CHEER = [
    """
.....s...
..g.gsg..
..gggsg..
.kGgggGk.
.khhsss..
.khssss..
..kssd...
.pPaAa...
pPPaaAa..
pPaagaa..
pPaaAaa..
pPPagaa..
.pPaaaa..
..PaaAa..
..ka.ka..
..ka.ka..
..ka.ka..
.kkk.kkk.
""",
    """
......s..
..g.g.sg.
..gggsg..
.kGgggGk.
.khhsss..
.khssss..
..kssd...
.pPaAa...
pPPaaAa..
pPaagaa..
pPaaAaa..
pPPagaa..
.pPaaaa..
..PaaAa..
.ka...ka.
.ka...ka.
.ka...ka.
kkk...kkk
""",
]


def king():
    """Standing, three steps of the walk, and two of a cheer."""
    return sheet([grid(f, {}) for f in KING_FRAMES + KING_CHEER], (12, 18))


def magnets():
    """The mark on its footing, the frown, and the small mark on a pole the King carries."""
    return sheet([mark_image(MARK, footing=3), grid(FROWN_STONE, {}), mark_image(MARK_SMALL, pole=4)], (15, 17))


# ---------------------------------------------------------------------------------------------------------------
# The fight: a ring of rubble, the two fighters grappling inside it, changing places.


def fight_frames():
    frames = []
    for k in range(4):
        img = Image.new("RGBA", (24, 20), (0, 0, 0, 0))
        px = img.load()
        # The ring: grey stones on an ellipse, its back half drawn first.
        def ring(front):
            for a in range(0, 360, 6):
                import math
                t = math.radians(a)
                x = 12 + 10.5 * math.cos(t)
                y = 13 + 5.5 * math.sin(t)
                if (math.sin(t) > 0) != front:
                    continue
                for dx in (-1, 0, 1):
                    for dy in (-1, 0):
                        xx, yy = round(x + dx), round(y + dy)
                        if 0 <= xx < 24 and 0 <= yy < 20:
                            n = noise(k * 7 + a, xx, yy)
                            px[xx, yy] = (STONE["o"] if n < 0.3 else STONE["O"] if n < 0.7 else STONE["m"]) + (255,)
        ring(False)
        # Two fighters, facing each other; they swap sides and strike in turn.
        a_team, b_team = ("green", "red") if k % 2 == 0 else ("red", "green")
        left = grid(WALKER[1 + (k // 2) % 2], {}, a_team)
        right = grid(WALKER[3 - (k // 2) % 2], {}, b_team).transpose(Image.FLIP_LEFT_RIGHT)
        img.alpha_composite(left, (3, 3 + k % 2))
        img.alpha_composite(right, (11, 4 - k % 2))
        ring(True)
        # Dust kicked up.
        for d in range(6):
            x, y = 3 + int(noise(k, d, 1) * 18), 2 + int(noise(k, d, 2) * 6)
            px[x, y] = (STONE["o"] if d % 2 else (0xFF, 0xFF, 0xFF)) + (255,)
        frames.append(img)
    return sheet(frames, (24, 20))


# ---------------------------------------------------------------------------------------------------------------
# Flags: a pennant on a pole, in two flutters.


def flags():
    frames = []
    for team in ("green", "red"):
        dark, mid, light = TEAMS[team]
        for k in range(2):
            img = Image.new("RGBA", (10, 14), (0, 0, 0, 0))
            px = img.load()
            for y in range(1, 14):
                px[1, y] = (0x42, 0x29, 0x00, 255)
                px[2, y] = (0x8C, 0x6B, 0x00, 255)
            for y in range(2, 7):
                length = 7 - abs(y - 4) - (1 if k and y % 2 else 0)
                for x in range(3, 3 + length):
                    wave = (x + k) % 3 == 0
                    px[x, y + (1 if wave and k else 0)] = (light if y < 4 else mid if not wave else dark) + (255,)
            if team == "green":
                # Omarchy's green would vanish against the grass without an edge.
                cloth = [(x, y) for y in range(14) for x in range(3, 10) if px[x, y][3]]
                for x, y in cloth:
                    for nx, ny in ((x + 1, y), (x, y - 1), (x, y + 1)):
                        if 3 <= nx < 10 and 0 <= ny < 14 and not px[nx, ny][3]:
                            px[nx, ny] = (0x18, 0x30, 0x08, 255)
            frames.append(img)
    return sheet(frames, (10, 14))


# ---------------------------------------------------------------------------------------------------------------
# Buildings, from isometric boxes. Each sprite's anchor is the bottom corner of its footprint.

def _poly(img, pts, color_at):
    from art import fill_poly
    rgb = Image.new("RGB", img.size)
    mask = Image.new("L", img.size, 0)
    from PIL import ImageDraw
    ImageDraw.Draw(mask).polygon(pts, fill=255)
    px = img.load()
    m = mask.load()
    for y in range(img.height):
        for x in range(img.width):
            if m[x, y]:
                c = color_at(x, y)
                if c is not None:
                    px[x, y] = c + (255,)


def _tex(seed, a, b, density):
    return lambda x, y: b if noise(seed, x, y) < density else a


def box(img, cx, by, hw, hh, height, left, right, top=None):
    """A box standing on the diamond whose bottom corner is (cx, by), `hw` and `hh` its half-width and half-height,
    `height` tall; `left`, `right` and `top` texture its faces."""
    l, b, r, t = (cx - hw, by - hh), (cx, by), (cx + hw, by - hh), (cx, by - 2 * hh)
    up = lambda p: (p[0], p[1] - height)
    _poly(img, [l, b, up(b), up(l)], left)
    _poly(img, [b, r, up(r), up(b)], right)
    if top:
        _poly(img, [up(l), up(b), up(r), up(t)], top)
    return [up(l), up(b), up(r), up(t)]


def roof(img, corners, apex_height, left, right, back=None):
    """A pyramid roof over `corners` (left, bottom, right, top of the walls' top)."""
    l, b, r, t = corners
    apex = ((l[0] + r[0]) // 2, (b[1] + t[1]) // 2 - apex_height)
    if back:
        _poly(img, [l, t, apex], back)
        _poly(img, [t, r, apex], back)
    _poly(img, [l, b, apex], left)
    _poly(img, [b, r, apex], right)


THATCH_L = _tex(31, (0xA5, 0x84, 0x00), (0xBD, 0x9C, 0x00), 0.35)
THATCH_R = _tex(32, (0x7B, 0x52, 0x00), (0x8C, 0x6B, 0x00), 0.35)
MUD_L = _tex(33, (0x7B, 0x52, 0x00), (0x5A, 0x42, 0x00), 0.3)
MUD_R = _tex(34, (0x5A, 0x42, 0x00), (0x42, 0x29, 0x00), 0.3)


def brick(seed, light):
    """Stone courses: light or shaded grey blocks with dark joints."""
    a, b, j = ((0xBD, 0xBD, 0xB5), (0xA5, 0xA5, 0x9C), (0x7B, 0x7B, 0x7B)) if light else \
              ((0x8C, 0x8C, 0x84), (0x7B, 0x7B, 0x7B), (0x52, 0x52, 0x4A))
    def at(x, y):
        if y % 3 == 0 or (x + (y // 3) * 2) % 5 == 0:
            return j
        return b if noise(seed, x, y) < 0.3 else a
    return at


def door(img, x, y, w=2, h=3):
    px = img.load()
    for dy in range(h):
        for dx in range(w):
            px[x + dx, y - dy] = (0, 0, 0, 255)


def building(tier):
    """A settlement, smallest to largest: a hut, a bigger hut, a stone house, a round stone tower."""
    img = Image.new("RGBA", (32, 34), (0, 0, 0, 0))
    cx, by = 16, 31
    if tier == 1:
        top = box(img, cx, by, 7, 3.5, 3, MUD_L, MUD_R)
        roof(img, top, 8, THATCH_L, THATCH_R)
        door(img, cx - 4, by - 2, 2, 3)
    elif tier == 2:
        top = box(img, cx, by, 10, 5, 6, MUD_L, MUD_R)
        roof(img, top, 9, THATCH_L, THATCH_R)
        door(img, cx - 6, by - 3, 2, 4)
    elif tier == 3:
        top = box(img, cx, by, 12, 6, 9, brick(1, True), brick(2, False))
        roof(img, top, 10, THATCH_L, THATCH_R)
        door(img, cx - 7, by - 4, 3, 5)
    else:
        # A round tower: a stone drum with battlements.
        import math
        px = img.load()
        r, hgt = 11, 13
        for y in range(img.height):
            for x in range(img.width):
                dx = (x - cx) / r
                if abs(dx) > 1:
                    continue
                base = by - 5 + 5.5 * math.sqrt(max(0, 1 - dx * dx))
                if base - hgt <= y <= base:
                    px[x, y] = brick(5, dx < 0.2)(x, y) + (255,)
        for y in range(img.height):
            for x in range(img.width):
                dx, dy = (x - cx) / r, (y - (by - 5 - hgt)) / 5.5
                if dx * dx + dy * dy <= 1:
                    inner = (x - cx) ** 2 / (r - 3) ** 2 + (y - (by - 5 - hgt)) ** 2 / 3.5 ** 2 <= 1
                    px[x, y] = (0x39, 0x31, 0x00, 255) if inner else (0xBD, 0xBD, 0xB5, 255)
        for k in range(-r, r + 1, 4):
            for dy in range(3):
                x = cx + k
                yb = by - 5 + 5.5 * math.sqrt(max(0, 1 - (k / r) ** 2)) - hgt
                if 0 <= x < img.width:
                    px[x, round(yb) - dy] = (0xBD, 0xBD, 0xB5, 255)
                    if x + 1 < img.width:
                        px[x + 1, round(yb) - dy] = (0x8C, 0x8C, 0x84, 255)
        door(img, cx - 2, by - 1, 3, 6)
    return img


def buildings():
    return sheet([building(t) for t in (1, 2, 3, 4)], (32, 34))


class _Safe:
    """Pixel access that ignores writes outside the image."""
    def __init__(self, img):
        self.img, self.px = img, img.load()

    def __setitem__(self, xy, c):
        x, y = round(xy[0]), round(xy[1])
        if 0 <= x < self.img.width and 0 <= y < self.img.height:
            self.px[x, y] = c


def castle():
    """The King's castle, three tiles a side: battlemented walls, a tower at each corner and, in the middle, the
    keep the King comes and goes from."""
    img = Image.new("RGBA", (120, 100), (0, 0, 0, 0))
    cx, by = 60, 96
    hw, hh = 48, 24
    px = _Safe(img)
    # The courtyard.
    grass = _tex(40, (0x39, 0xA5, 0x10), (0x29, 0x84, 0x00), 0.3)
    _poly(img, [(cx - hw + 6, by - hh), (cx, by - 3), (cx + hw - 6, by - hh), (cx, by - 2 * hh + 3)], grass)
    def wall(a, b, light):
        # A wall from a to b along the footprint's edge: its face, then battlements along its top.
        (ax, ay), (bx, by_) = a, b
        _poly(img, [a, b, (bx, by_ - 9), (ax, ay - 9)], brick(7 if light else 8, light))
        steps = max(abs(bx - ax), 1)
        for k in range(0, steps + 1):
            x = round(ax + (bx - ax) * k / steps)
            y = round(ay + (by_ - ay) * k / steps) - 9
            if (k // 2) % 2 == 0:
                for dy in range(1, 3):
                    px[x, y - dy] = (0xBD, 0xBD, 0xB5, 255) if light else (0x8C, 0x8C, 0x84, 255)
            px[x, y] = (0x7B, 0x7B, 0x7B, 255)
    l, b, r, t = (cx - hw, by - hh), (cx, by), (cx + hw, by - hh), (cx, by - 2 * hh)
    # Back walls first, then the keep, then the front walls.
    wall(l, t, False)
    wall(t, r, True)
    def tower(x, y, height):
        top = box(img, x, y, 7, 3.5, height, brick(9, True), brick(10, False),
                  lambda a, b: (0x7B, 0x7B, 0x7B))
        for k, p in enumerate(top):
            for dy in range(1, 3):
                if 0 <= p[1] - dy < img.height:
                    px[round(p[0]), round(p[1]) - dy] = (0xBD, 0xBD, 0xB5, 255)
        # Arrow slits: small crosses.
        for sx in (x - 4, x + 3):
            sy = y - height // 2 - 2
            for d in (-1, 0, 1):
                px[sx, sy + d] = (0, 0, 0, 255)
            px[sx - 1, sy - 0] = (0, 0, 0, 255)
            px[sx + 1, sy - 0] = (0, 0, 0, 255)
    tower(*t, 14)
    tower(*l, 14)
    tower(*r, 14)
    # The keep: a tall square tower in the middle.
    keep_top = box(img, cx, by - hh + 8, 9, 4.5, 30, brick(11, True), brick(12, False),
                   lambda a, b: (0x7B, 0x7B, 0x7B))
    for p in keep_top:
        for dy in range(1, 4):
            px[round(p[0]), round(p[1]) - dy] = (0xBD, 0xBD, 0xB5, 255)
    door(img, cx - 5, by - hh + 3, 3, 7)
    for sx in (cx - 6, cx + 5):
        for d in range(-2, 3):
            px[sx, by - hh - 14 + d] = (0, 0, 0, 255)
        px[sx - 1, by - hh - 14] = (0, 0, 0, 255)
        px[sx + 1, by - hh - 14] = (0, 0, 0, 255)
    wall(l, b, True)
    wall(b, r, False)
    tower(*b, 14)
    # The gate, in the front left wall.
    gx, gy = (l[0] + b[0]) // 2, (l[1] + b[1]) // 2
    door(img, gx - 1, gy, 4, 7)
    return img


def trees():
    """A pine, a bare tree and a cluster of rocks."""
    out = []
    pine = Image.new("RGBA", (14, 20), (0, 0, 0, 0))
    px = pine.load()
    for y in range(16):
        half = 1 + y * 5 // 15
        for x in range(7 - half, 7 + half + 1):
            n = noise(50, x, y)
            px[x, y] = ((0x18, 0x4A, 0x18) if x > 7 else (0x21, 0x6B, 0x10) if n < 0.6 else (0x29, 0x84, 0x00)) + (255,)
    for y in range(16, 20):
        px[6, y] = (0x63, 0x39, 0x00, 255)
        px[7, y] = (0x42, 0x29, 0x00, 255)
    out.append(pine)
    bare = Image.new("RGBA", (14, 20), (0, 0, 0, 0))
    px = bare.load()
    for y in range(6, 20):
        px[7, y] = (0x8C, 0x6B, 0x00, 255)
        px[6, y] = (0xA5, 0x84, 0x00, 255)
    for k, (x0, y0, dx) in enumerate(((7, 10, -1), (7, 8, 1), (7, 12, 1), (6, 7, -1))):
        for s in range(5):
            x, y = x0 + dx * s, y0 - s // 2 - 1
            if 0 <= x < 14 and 0 <= y < 20:
                px[x, y] = (0xA5, 0x84, 0x00, 255)
    for k in range(10):
        x, y = 2 + int(noise(60, k, 1) * 10), 1 + int(noise(60, k, 2) * 9)
        px[x, y] = ((0x73, 0xAD, 0x00) if k % 2 else (0x29, 0x84, 0x00)) + (255,)
    out.append(bare)
    rocks = Image.new("RGBA", (14, 20), (0, 0, 0, 0))
    px = rocks.load()
    for (rx, ry, rr) in ((4, 15, 4), (9, 16, 4), (7, 12, 4)):
        for y in range(20):
            for x in range(14):
                d = ((x - rx) / rr) ** 2 + ((y - ry) / (rr * 0.8)) ** 2
                if d <= 1:
                    shade = (x - rx) - (y - ry)
                    px[x, y] = (STONE["o"] if shade < -1 else STONE["O"] if shade < 2 else STONE["m"]) + (255,)
    out.append(rocks)
    return sheet(out, (14, 20))


# ---------------------------------------------------------------------------------------------------------------
# Crops, swamps and cracks: on the ground, a tile's diamond (32 x 16) at a time.


def _diamond(x, y, inset=0.0):
    """Whether pixel (x, y) is in a tile's diamond, its top corner at (16, 0), shrunk by `inset` (0 to 1)."""
    u = abs(x + 0.5 - 16) / 16 + abs(y + 0.5 - 8) / 8
    return u <= 1.0 - inset


def crops():
    """Planted land: light fields with rows of yellow dots for the Omarchy side, dark fields with dark dots for
    its rivals, the rows running along the tile."""
    frames = []
    for good in (True, False):
        img = Image.new("RGBA", (32, 16), (0, 0, 0, 0))
        px = img.load()
        base, alt, dot = ((PAL["grass_light"], PAL["grass_hi"], PAL["crop"]) if good else
                          (PAL["grass_shade"], PAL["grass_dark"], PAL["dirt_dark"]))
        for y in range(16):
            for x in range(32):
                if not _diamond(x, y, 0.08):
                    continue
                row = (x + 2 * y) % 6
                c = dot if row == 0 and x % 2 == 0 else alt if row == 3 else base
                px[x, y] = c + (255,)
        frames.append(img)
    return sheet(frames, (32, 16))


def swamps():
    """A swamp spreading over its tile (three frames), then bubbling (two)."""
    mud, mire, bubble, reed = (0x31, 0x39, 0x10), (0x4A, 0x52, 0x18), (0x8C, 0x94, 0x39), (0x6B, 0x84, 0x10)
    frames = []
    for k, inset in enumerate((0.6, 0.35, 0.1, 0.1, 0.1)):
        img = Image.new("RGBA", (32, 16), (0, 0, 0, 0))
        px = img.load()
        for y in range(16):
            for x in range(32):
                if _diamond(x, y, inset):
                    edge = not _diamond(x, y, inset + 0.12)
                    px[x, y] = (mire if edge or noise(70, x, y) < 0.25 else mud) + (255,)
        if k >= 2:
            for b in range(4):
                x, y = 9 + int(noise(71 + k, b, 1) * 14), 5 + int(noise(71 + k, b, 2) * 6)
                px[x, y] = bubble + (255,)
            for x, y in ((6, 8), (25, 7), (16, 3)):
                for d in range(3):
                    px[x + (d % 2), y - d] = reed + (255,)
        frames.append(img)
    return sheet(frames, (32, 16))


def cracks():
    """The ground splitting: a jagged black crack with lit edges, opening wider each frame."""
    frames = []
    for k in range(4):
        img = Image.new("RGBA", (28, 14), (0, 0, 0, 0))
        px = img.load()
        length = 6 + k * 6
        for s in range(-length, length + 1):
            x = 14 + s
            y = 7 + s // 2 + (1 if noise(80, s, 0) < 0.5 else 0) - (s // 6) % 2
            width = max(1, (k + 1) - abs(s) * (k + 1) // (length + 1))
            for d in range(-1, width + 1):
                if 0 <= x < 28 and 0 <= y + d < 14:
                    edge = d in (-1, width)
                    px[x, y + d] = (PAL["dirt_light"] if d == -1 else PAL["dirt_dark"]) + (255,) if edge else (0, 0, 0, 255)
        frames.append(img)
    return sheet(frames, (28, 14))


def sinking(team="red"):
    """A walker going down into a swamp, a few rows at a time, mud rippling round him."""
    stand = grid(WALKER[0], {}, team)
    frames = []
    for k in range(5):
        img = Image.new("RGBA", (12, 14), (0, 0, 0, 0))
        img.alpha_composite(stand.crop((0, 0, 10, 14 - 3 * k)), (1, 3 * k))
        px = img.load()
        for x in range(12):
            if abs(x - 5.5) > 2 + k % 2:
                px[x, 13] = (0x8C, 0x94, 0x39, 255) if (x + k) % 3 else (0x4A, 0x52, 0x18, 255)
        frames.append(img)
    return sheet(frames, (12, 14))
