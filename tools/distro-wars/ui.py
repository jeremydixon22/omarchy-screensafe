"""The frame around the close-up map: the atlas, the mana bar, the shield and the command plates.

Drawn here from scratch, with each command drawn as its Distro Wars counterpart (see glyphs.py). A heraldic penguin stands behind the shield, holding it up.
"""
from art import PAL, TOP, fill_poly, noise
from glyphs import stamp, size


def line(img, a, b, color, width=1):
    px = img.load()
    (x0, y0), (x1, y1) = a, b
    steps = max(abs(x1 - x0), abs(y1 - y0), 1)
    for k in range(steps + 1):
        x = round(x0 + (x1 - x0) * k / steps)
        y = round(y0 + (y1 - y0) * k / steps)
        for w in range(width):
            if 0 <= x < img.width and 0 <= y + w < img.height:
                px[x, y + w] = color


def rect(img, x0, y0, x1, y1, color):
    px = img.load()
    for y in range(max(0, y0), min(img.height, y1)):
        for x in range(max(0, x0), min(img.width, x1)):
            px[x, y] = color


# ---------------------------------------------------------------------------------------------------------------
# The atlas


def diamond(cx, cy, hw, hh):
    return [(cx, cy - hh), (cx + hw, cy), (cx, cy + hh), (cx - hw, cy)]


# The open book's front edge: the left page's from the screen's edge to the spine, then the right page's.
FRONT_LEFT = ((0, 97), (56, 85))
FRONT_RIGHT = ((72, 84), (146, 45))
SPINE = (64, 92)
EDGES = 20
MAP_DIAMOND = (62, 38, 62, 31)


def front_edge(x):
    """The y of the book's front edge at x, dipping at the spine where the pages curl."""
    (a, b), (c, d) = FRONT_LEFT, FRONT_RIGHT
    if x <= a[0]:
        return a[1]
    if x <= b[0]:
        return a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0])
    if x < c[0]:
        return b[1] + (c[1] - b[1]) * (x - b[0]) / (c[0] - b[0])
    return c[1] + (d[1] - c[1]) * (x - c[0]) / (d[0] - c[0])


def in_top(x, y):
    """Whether (x, y) is on the book's top surface."""
    return y >= 22 - x / 2 and y >= x / 2 - 28 and y < front_edge(x) and x <= FRONT_RIGHT[1][0]


def draw_book(img, world):
    px = img.load()
    for y in range(0, 130):
        for x in range(0, 160):
            if in_top(x, y):
                # White, shaded with a grey checker in the trough beside the spine on the left page.
                gutter = 52 + x / 2
                below = y - gutter
                c = PAL["page"]
                if x < 64 and 0 <= below < 26 and (x + y) % 2 == 0:
                    c = PAL["page_grey"] if below < 6 * noise(2, x, y) + 4 else PAL["page_shade"] if below < 18 else c
                px[x, y] = c
                continue
            if x > FRONT_RIGHT[1][0] + 1 or y < 22 - x / 2:
                continue
            edge = front_edge(x)
            d = y - edge
            if not 0 <= d < EDGES + 5 or y < x / 2 - 28:
                continue
            # The pages' edges: black lines on white, curling round the spine.
            dx = x - SPINE[0]
            if abs(dx) < 10 and y > SPINE[1] - 8:
                r = ((dx * 1.2) ** 2 + (y - (SPINE[1] + 14)) ** 2) ** 0.5
                if r < 22:
                    px[x, y] = PAL["black"] if int(r) % 3 == 0 else PAL["white"]
                    continue
            if d < EDGES:
                px[x, y] = PAL["black"] if int(d) % 3 == 0 else PAL["white"]
            elif d < EDGES + 1:
                px[x, y] = PAL["black"]
            elif d < EDGES + 4:
                px[x, y] = PAL["cover"] if d < EDGES + 3 else PAL["cover_dark"]
            else:
                px[x, y] = PAL["black"]
    # The world, in an orange frame with a white margin.
    mx, my, mw, mh = MAP_DIAMOND
    fill_poly(img, diamond(mx, my, mw + 5, mh + 3), lambda x, y: PAL["map_frame"] if in_top(x, y) else None)
    fill_poly(img, diamond(mx, my, mw + 3, mh + 2), lambda x, y: PAL["page"] if in_top(x, y) else None)
    fill_poly(img, diamond(mx, my, mw, mh), lambda x, y: world(x - (mx - mw), y - (my - mh)))
    draw_world_marks(img)
    # The book's compass mark, at the top left of the page.
    for a, b in (((12, 23), (23, 18)), ((23, 18), (19, 17)), ((23, 18), (21, 21))):
        line(img, a, b, PAL["black"])
    for a, b in (((28, 13), (35, 10)), ((35, 10), (29, 17)), ((29, 17), (37, 14))):
        line(img, a, b, PAL["black"])


def world_heights(seed=3):
    """The world's land: smooth noise over the map, from 0 (deep sea) to 1."""
    def at(u, v):
        n = 0.0
        for scale, weight in ((22, 0.55), (10, 0.3), (4, 0.15)):
            gx, gy = u / scale, v / scale
            x0, y0 = int(gx), int(gy)
            fx, fy = gx - x0, gy - y0
            a = noise(seed + scale, x0, y0)
            b = noise(seed + scale, x0 + 1, y0)
            c = noise(seed + scale, x0, y0 + 1)
            d = noise(seed + scale, x0 + 1, y0 + 1)
            n += weight * (a * (1 - fx) * (1 - fy) + b * fx * (1 - fy) + c * (1 - fx) * fy + d * fx * fy)
        return n
    return at


SEA_LEVEL = 0.55


def world_map(seed=3):
    """The atlas's map: sea, and land shaded by height, its high ground lit on the side facing the upper
    left and dithered dark on the other."""
    h = world_heights(seed)
    def at(u, v):
        n = h(u, v)
        if n <= SEA_LEVEL:
            return PAL["map_sea"]
        slope = h(u + 1, v + 1) - h(u - 1, v - 1)
        if n > 0.66 and slope < -0.004:
            return PAL["grass_hi"] if (u + v) % 2 else PAL["map_land"]
        if n > 0.66 and slope > 0.004:
            return PAL["map_land_dark"] if (u + v) % 2 else PAL["map_land"]
        if n < SEA_LEVEL + 0.02:
            return PAL["grass_light"]
        return PAL["map_land"]
    return at


def draw_world_marks(img, seed=3):
    """On the map: white dots for the Omarchy side's settlements, and the white square showing where the close-up
    map is, on the largest stretch of land."""
    mx, my, mw, mh = MAP_DIAMOND
    h = world_heights(seed)
    px = img.load()
    inside = lambda x, y: abs(x - mx) / mw + abs(y - my) / mh <= 0.85
    land = [(x, y) for y in range(my - mh, my + mh) for x in range(mx - mw, mx + mw)
            if inside(x, y) and h(x - (mx - mw), y - (my - mh)) > SEA_LEVEL + 0.05]
    # The close-up: the land point farthest inland, as a square with a dark middle.
    best = max(land, key=lambda p: h(p[0] - (mx - mw), p[1] - (my - mh)))
    for dx in range(-3, 4):
        for dy in range(-3, 4):
            edge = max(abs(dx), abs(dy)) == 3
            middle = abs(dx) <= 1 and abs(dy) <= 1
            if edge or middle:
                px[best[0] + dx, best[1] + dy] = PAL["white"] if edge or (dx, dy) == (0, 0) else PAL["black"]
    for k in range(9):
        x, y = land[int(noise(90, k, 1) * len(land))]
        if abs(x - best[0]) > 5 or abs(y - best[1]) > 5:
            px[x, y] = PAL["white"]


# ---------------------------------------------------------------------------------------------------------------
# The mana bar


BAR_ICONS = ["pacman", "mark", "segfault", "dephell", "root", "forkbomb", "memleak", "crash"]
CELL0 = (137, 19.5)


def cell_center(k):
    return (CELL0[0] + 16 * k, CELL0[1] + 8 * k)


def draw_bar(img):
    # Two gold rails under the bar, and the bracket joining it to the book.
    for off in (27, 33):
        line(img, (120, 120 // 2 - off + 3), (256, 256 // 2 - off), PAL["gold"], 2)
    for a, b in (((98, 21), (122, 9)), ((98, 21), (122, 33)), ((122, 9), (130, 13))):
        line(img, a, b, PAL["gold"], 2)
    # The bar: a strip of rhombic cells down to the right, its front face below.
    for k in range(0, 9):
        x, y = cell_center(k)
        top = (x - 1, y - 7.5)
        cell = [top, (top[0] + 16, top[1] + 8), (top[0] + 2, top[1] + 15), (top[0] - 14, top[1] + 7)]
        fill_poly(img, cell, lambda px, py: PAL["bar"])
        # The front face: dark below the cell's lower edge.
        face = [cell[3], cell[2], (cell[2][0], cell[2][1] + 3), (cell[3][0], cell[3][1] + 3)]
        fill_poly(img, face, lambda px, py: PAL["bar_dark"])
        line(img, cell[3], cell[2], PAL["orange"])
        line(img, cell[0], cell[3], PAL["bar_cell"])
        line(img, cell[0], cell[1], PAL["bar_cell"])
        if 0 <= k < len(BAR_ICONS):
            name = BAR_ICONS[k]
            w, h = size(name)
            stamp(img, name, round(x - w / 2), round(y - h / 2), PAL["gold_hi"], PAL["gold"])
    # The bar's left end, and the post from its underside down to the tabletop.
    rect(img, 145, 34, 151, 80, PAL["gold"])
    rect(img, 145, 34, 146, 80, PAL["gold_hi"])
    rect(img, 149, 34, 151, 80, PAL["gold_mid"])
    rect(img, 144, 79, 152, 82, PAL["gold_dark"])


def draw_pointer(img, k):
    """The mana pointer: a gold arrow under cell `k` of the bar, pointing up to it."""
    x, y = cell_center(k)
    tip = (round(x - 4), round(y + 14))
    for d in range(10):
        line(img, (tip[0] - d, tip[1] + d), (tip[0] - d, tip[1] + d), PAL["gold_hi"], 2)
    line(img, tip, (tip[0] + 5, tip[1]), PAL["gold_hi"], 2)
    line(img, tip, (tip[0], tip[1] + 5), PAL["gold_hi"], 2)


# ---------------------------------------------------------------------------------------------------------------
# The shield and the population bars


SHIELD = (197, 12, 234, 52)

# The penguin, a pixel a character: k black, K its sheen, w white, y its beak and feet, Y their shade.
PENGUIN_HEAD = """
......kkkkkk......
....kkKkkkkkkk....
...kKkkkkkkkkkk...
..kkkwwwkkwwwkkk..
..kkwwkwwwwkwwkk..
.kkkwwkwwwwkwwkkk.
.kkkwwwwyywwwwkkk.
.kkkkwwyyyywwkkkk.
.kkkkkwYyyYwkkkkk.
..kkkkwwYYwwkkkk..
..kkkwwwwwwwwkkk..
...kkwwwwwwwwkk...
"""
PENGUIN_FLIPPER = """
..kk
.kKk
kKkk
kkkk
kkk.
kkk.
kk..
k...
"""
PENGUIN_FOOT = """
.yyy..
yyyyyy
YyYyYY
"""
PENGUIN_COLORS = {
    "k": (0x18, 0x18, 0x29), "K": (0x4A, 0x4A, 0x63), "w": PAL["white"],
    "y": PAL["gold"], "Y": PAL["orange"],
}


def art_at(img, text, x0, y0, mirror=False):
    rows = text.strip("\n").split("\n")
    px = img.load()
    w = max(len(r) for r in rows)
    for dy, row in enumerate(rows):
        for dx, ch in enumerate(row):
            if ch in PENGUIN_COLORS:
                x = x0 + (w - 1 - dx if mirror else dx)
                if 0 <= x < img.width and 0 <= y0 + dy < img.height:
                    px[x, y0 + dy] = PENGUIN_COLORS[ch]


def draw_penguin(img, front):
    """The penguin behind the shield: its head above it and its feet on the mana bar below it, then, in `front`,
    its flippers gripping the shield's sides."""
    x0, y0, x1, y1 = SHIELD
    cx = (x0 + x1) // 2
    if not front:
        art_at(img, PENGUIN_HEAD, cx - 8, 0)
        art_at(img, PENGUIN_FOOT, cx - 9, y1 - 4)
        art_at(img, PENGUIN_FOOT, cx + 4, y1 - 4, mirror=True)
    else:
        art_at(img, PENGUIN_FLIPPER, x0 - 3, 16)
        art_at(img, PENGUIN_FLIPPER, x1 + 0, 16, mirror=True)


def shield_shape(x, y):
    """Whether (x, y) is inside the shield: straight sides, then curving to a point."""
    x0, y0, x1, y1 = SHIELD
    cx = (x0 + x1) / 2
    if not (x0 <= x <= x1 and y0 <= y <= y1):
        return False
    straight = y0 + (y1 - y0) * 0.5
    if y <= straight:
        return True
    t = (y - straight) / (y1 - straight)
    half = (x1 - x0) / 2 * (1 - t ** 1.6)
    return abs(x - cx) <= half


def draw_shield(img, quarters, emblems=()):
    """`quarters`: colors for top left, top right, bottom left, bottom right; `emblems`: (glyph, quarter, color)
    drawn in the middle of a quarter."""
    from glyphs import stamp, size
    px = img.load()
    x0, y0, x1, y1 = SHIELD
    cx, cy = (x0 + x1) // 2, y0 + 19
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            if not shield_shape(x, y):
                continue
            inner = all(shield_shape(x + dx, y + dy) for dx, dy in ((-3, 0), (3, 0), (0, -3), (0, 3)))
            mid = all(shield_shape(x + dx, y + dy) for dx, dy in ((-2, 0), (2, 0), (0, -2), (0, 2)))
            if not mid:
                px[x, y] = PAL["gold_hi"] if x < cx + 6 else PAL["orange"]
            elif not inner:
                px[x, y] = PAL["orange"] if x < cx + 6 else PAL["red_dark"]
            else:
                q = (1 if x > cx else 0) + (2 if y > cy else 0)
                px[x, y] = quarters[q]
                if x in (cx, cx + 1) or y in (cy, cy + 1):
                    px[x, y] = PAL["black"] if (x == cx or y == cy) else PAL["white"]
    for name, q, color in emblems:
        w, h = size(name)
        qx = (x0 + 3 + cx) / 2 if q in (0, 2) else (cx + 2 + x1 - 3) / 2
        qy = (y0 + 3 + cy) / 2 if q in (0, 1) else cy + 2 + 6
        stamp(img, name, round(qx - w / 2), round(qy - h / 2), color, PAL["black"])


# The info shield shows its bearer, the Omarch King: his side's green and his mark, his crown, his castle, and his
# strength.
KING_SHIELD = (
    [PAL["omarchy_green"], PAL["shield_grey"], PAL["shield_grey"], PAL["omarchy_green"]],
    [("mark", 0, PAL["omarchy_green_dark"]), ("crown", 1, PAL["gold"]), ("castle", 2, PAL["icon_dark"]),
     ("strength", 3, PAL["omarchy_green_dark"])],
)


def draw_population(img, good, evil):
    """The two population bars beside the shield, `good` and `evil` from 0 to 1."""
    for x0, amount, tip in ((185, good, PAL["omarchy_green"]), (241, evil, PAL["red_bright"])):
        rect(img, x0, 0, x0 + 8, 37, PAL["orange"])
        rect(img, x0 + 1, 0, x0 + 7, 36, PAL["gold_hi"])
        rect(img, x0 + 2, 0, x0 + 6, 35, PAL["black"])
        fill = round(33 * max(0.0, min(1.0, amount)))
        rect(img, x0 + 2, 35 - fill, x0 + 6, 35, PAL["bar_green"])
        rect(img, x0 + 2, 33, x0 + 6, 35, tip)
    for y in (11, 27):
        rect(img, 193, y, 197, y + 2, PAL["orange"])
        rect(img, 234, y, 241, y + 2, PAL["orange"])


# ---------------------------------------------------------------------------------------------------------------
# The command plates, set into the tabletop in the two bottom corners


def cell(i, j):
    """The tabletop cell whose top corner is grid corner (i, j) at sea level."""
    x, y = TOP[0] + (i - j) * 16, TOP[1] + (i + j) * 8
    return [(x, y), (x + 16, y + 8), (x, y + 16), (x - 16, y + 8)]


LEFT_PLATES = [
    # (i, j, glyph, raised plate): tabletop cells beyond the map's front left edge.
    (2, 9, "package", False), (3, 9, "flame", False),
    (2, 10, "shield", False), (3, 10, "merge", False), (4, 9, "merge", False),
    (3, 11, "mark", False), (4, 10, "flame", False), (5, 9, "flag", True),
    (4, 11, "mark", False), (5, 10, "prompt", False), (6, 9, "pacman", True),
    (5, 11, "mark", False), (6, 10, "manual", False),
]
RIGHT_PLATES = [
    # Beyond the map's front right edge.
    (9, 2, "cog", False), (9, 3, "bell", True), (10, 3, "globe", False),
    (9, 4, "note", True), (10, 4, "moon", False), (10, 5, "keyboard", True),
    (8, 4, "tiles", False), (8, 5, "merge", False), (9, 5, "mark", False),
    (7, 5, "flag", False), (8, 6, "segfault", False), (9, 6, "crash", False),
]


def draw_plates(img):
    for plates in (LEFT_PLATES, RIGHT_PLATES):
        for i, j, name, raised in plates:
            c = cell(i, j)
            if raised:
                fill_poly(img, c, lambda x, y: PAL["icon_plate"])
                for a, b in ((c[0], c[1]), (c[0], c[3])):
                    line(img, a, b, PAL["rim_light"])
            cx, cy = c[0][0], c[0][1] + 8
            w, h = size(name)
            color = PAL["white"] if raised else PAL["icon"]
            gx, gy = round(cx - w / 2), round(cy - h / 2)
            # Cut into the stone: a dark edge below and to the right of each stroke.
            stamp(img, name, gx + 1, gy + 1, PAL["grout"], PAL["grout"])
            stamp(img, name, gx, gy, color, PAL["icon_dark"])
