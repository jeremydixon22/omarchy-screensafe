"""Builds the Distro Wars pack: its sprite sheets and a screensafe.toml scripted from a simulated campaign.

    python3 build.py OUT_DIR

The campaign is worked out here, once, with a fixed seed: who walks where, who settles, who fights and who wins.
The pack only replays it, every position and frame a function of time.
"""
import heapq
import os
import random
import sys

from PIL import Image

import art
import sprites
import table
import terrain
import ui

LENGTH = 1800.0         # seconds the campaign runs before it starts again
SETTLE_UNTIL = 400.0    # rivals settle empty land only this early; after that they only come to fight
SPEED = 7.0             # canvas pixels a second a walker covers
FIGHT = (3.0, 4.5)      # how long a fight lasts
GROW = (40.0, 60.0)     # seconds between a settlement's growing a size
SEED = 1989

H = terrain.heights()
CASTLE_TILES = {(i, j) for i in range(3, 6) for j in range(3, 6)}
# Where the King comes out of his castle, and where the two magnets stand, before its gate.
GATE_IN = (4.5, 6.05)
GATE_OUT = (4.5, 6.75)
MAZE_AT = (3.3, 6.75)
FROWN_AT = (5.7, 6.75)
# The disasters the King casts on rival bands, once the land is all his: when, and how far the mana pointer falls.
SEGFAULT = (720.0, 2.0)
CHEER = 1.2             # seconds a winner cheers
DEPHELL = (1260.0, 2.5)
# Settlement sites: flat tiles, and the largest each can grow to. The tile before the castle's gate is left to a
# few rocks, so that nothing tall hides the King's fights there.
SITES = [((1, 1), 4), ((1, 4), 4), ((4, 1), 4), ((7, 3), 3), ((7, 6), 3), ((1, 7), 3)]
TREES = [((0.4, 0.5), 0), ((1.2, 6.1), 1), ((3.5, 0.4), 0), ((5.4, 0.6), 2), ((2.6, 7.6), 0), ((5.5, 2.4), 2),
         ((7.6, 7.8), 1), ((4.3, 7.8), 2), ((0.5, 2.6), 2), ((2.4, 2.5), 0)]


# ---------------------------------------------------------------------------------------------------------------
# Geometry


def height(i, j):
    """The land's height at map point (i, j), from its corners."""
    i = min(max(i, 0.0), art.TILES - 1e-6)
    j = min(max(j, 0.0), art.TILES - 1e-6)
    i0, j0 = int(i), int(j)
    fi, fj = i - i0, j - j0
    a, b = H[i0][j0], H[i0 + 1][j0]
    c, d = H[i0][j0 + 1], H[i0 + 1][j0 + 1]
    return a * (1 - fi) * (1 - fj) + b * fi * (1 - fj) + c * (1 - fi) * fj + d * fi * fj


def screen(i, j):
    """Canvas pixel of map point (i, j), on the land."""
    x, y = art.corner(i, j, 0)
    return x, y - height(i, j) * art.STEP


def stage(p):
    """Canvas pixel to stage units: the 256 x 224 picture centered, one unit 224 pixels."""
    return ((p[0] - 128) / 224, (p[1] - 112) / 224)


def walkable(t):
    i, j = t
    if not (0 <= i < art.TILES and 0 <= j < art.TILES) or t in CASTLE_TILES:
        return False
    return not all(H[a][b] == 0 for a in (i, i + 1) for b in (j, j + 1))


def path(a, b, blocked=()):
    """Tile centers from tile a to tile b, eight ways, around the castle, water and `blocked` tiles."""
    if a == b:
        return [a]
    frontier, came = [(0, a)], {a: None}
    while frontier:
        _, cur = heapq.heappop(frontier)
        if cur == b:
            break
        for di in (-1, 0, 1):
            for dj in (-1, 0, 1):
                n = (cur[0] + di, cur[1] + dj)
                if n in came or (n != b and (not walkable(n) or n in blocked)):
                    continue
                # No cutting a corner past something blocked.
                if di and dj and not (walkable((cur[0] + di, cur[1])) and walkable((cur[0], cur[1] + dj))):
                    continue
                came[n] = cur
                heapq.heappush(frontier, (abs(n[0] - b[0]) + abs(n[1] - b[1]) + len(came) * 1e-6, n))
    if b not in came:
        return [a, b]
    out = [b]
    while out[-1] != a:
        out.append(came[out[-1]])
    return out[::-1]


def center(t):
    return (t[0] + 0.5, t[1] + 0.5)


def flat(t):
    i, j = t
    return len({H[a][b] for a in (i, i + 1) for b in (j, j + 1)}) == 1


def tile_of(p):
    return (int(p[0]), int(p[1]))


def crop_tiles():
    """The fields each settlement plants as it grows: up to one fewer than its largest size, on flat land beside it
    that nothing else stands on."""
    used = {s for s, _ in SITES} | CASTLE_TILES | {tile_of(p) for p, _ in TREES} | {tile_of(MAZE_AT), tile_of(FROWN_AT)}
    used |= {(i, j) for i in range(3, 6) for j in range(6, 8)}
    out = []
    for site, cap in SITES:
        fields = []
        for di, dj in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)):
            t = (site[0] + di, site[1] + dj)
            if len(fields) < cap - 1 and t not in used and walkable(t) and flat(t):
                fields.append(t)
                used.add(t)
        out.append(fields)
    return out, used


CROPS, TAKEN = crop_tiles()
# Where Dependency Hell's swamps open: swamps need flat land, and the only flat land left empty is on the map's back
# edge, where rival bands come in.
SWAMPS = [t for t in ((1, 0), (2, 0)) if t not in TAKEN and walkable(t) and flat(t)]
assert len(SWAMPS) == 2
# What the segfault's band keeps off as it wanders: the castle's gate and the magnets before it.
GATE_AREA = {(i, j) for i in range(3, 6) for j in range(6, 8)}


# ---------------------------------------------------------------------------------------------------------------
# The campaign


class Walker:
    def __init__(self, team, t, at):
        self.team, self.born = team, t
        self.keys = [(t, at)]        # (time, map point)
        self.hidden = []             # (from, to): while fighting, the ring shows instead
        self.end = None

    @property
    def now(self):
        return self.keys[-1][0]

    @property
    def at(self):
        return self.keys[-1][1]

    def walk(self, to):
        a, b = screen(*self.at), screen(*to)
        d = ((a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2) ** 0.5
        self.keys.append((self.now + max(d / SPEED, 0.05), to))

    def wait(self, s):
        self.keys.append((self.now + s, self.at))

    def follow(self, tiles):
        for t in tiles[1:]:
            self.walk(center(t))

    def where(self, t):
        """The map point the walker is at at time t."""
        for (t0, a), (t1, b) in zip(self.keys, self.keys[1:]):
            if t0 <= t <= t1:
                f = (t - t0) / (t1 - t0) if t1 > t0 else 1.0
                return (a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f)
        return self.keys[-1][1] if t > self.keys[-1][0] else self.keys[0][1]


class Campaign:
    def __init__(self):
        self.rng = random.Random(SEED)
        self.walkers = []
        self.fights = []                     # (from, to, map point)
        # Each site's history: (time, owner or None, size), and when it's hidden by a fight.
        self.sites = [[(0.0, None, 0)] for _ in SITES]
        self.site_hidden = [[] for _ in SITES]
        # When each site is next free: someone on their way to settle or fight there holds it until they're done,
        # so no one else plans to take it meanwhile.
        self.claims = {}
        self.king = []                       # (time out, fight from, fight to, time back in)
        self.king_free = 0.0
        self.cracks = []                     # (opens, closes, map point): the segfault's
        self.swamps = []                     # (opens, swallows, dries, tile): Dependency Hell's
        self.sinks = []                      # (from, to, map point): rivals going down into a swamp
        self.casts = []                      # (time, mana spent)
        self.cheers = []                     # (from, to, map point): an Omarchy walker cheering a win
        self.quake = None                    # (from, to): the land shakes

    def state(self, k, t):
        """A site's owner and size at time t."""
        owner, size = None, 0
        for (when, o, s) in self.sites[k]:
            if when <= t:
                owner, size = o, s
        return owner, size

    def set_site(self, k, t, owner, size):
        self.sites[k].append((t, owner, size))
        self.sites[k].sort(key=lambda e: e[0])
        # It grows from there, a size at a time, until it's as large as its land allows or it changes hands.
        cap = SITES[k][1]
        when = t
        while size < cap:
            when += self.rng.uniform(*GROW)
            if when >= LENGTH:
                break
            size += 1
            self.sites[k].append((when, owner, size))
        self.sites[k].sort(key=lambda e: e[0])

    def take_over(self, k, t, owner):
        """Site k changes hands at time t, keeping its size; later growth for the old owner is dropped."""
        _, size = self.state(k, t)
        self.sites[k] = [e for e in self.sites[k] if e[0] <= t]
        self.set_site(k, t, owner, max(size, 1))

    def edge_tile(self):
        """A walkable tile on the map's edge, where a walker comes into view."""
        while True:
            side = self.rng.randrange(4)
            n = self.rng.randrange(art.TILES)
            t = [(n, 0), (n, art.TILES - 1), (0, n), (art.TILES - 1, n)][side]
            if walkable(t) and t not in [s for s, _ in SITES]:
                return t

    def wander(self, w, steps, avoid=()):
        """A few hops in the way walkers mill about: a tile or two at a time, with pauses."""
        cur = (int(w.at[0]), int(w.at[1]))
        for _ in range(steps):
            options = [(cur[0] + di, cur[1] + dj) for di in (-1, 0, 1) for dj in (-1, 0, 1) if di or dj]
            options = [o for o in options if walkable(o) and o not in [s for s, _ in SITES] and o not in avoid]
            if not options:
                # Hemmed in: they stand a moment, so that time still passes for them.
                w.wait(1.0)
                break
            cur = self.rng.choice(options)
            w.walk(center(cur))
            if self.rng.random() < 0.4:
                w.wait(self.rng.uniform(0.4, 1.5))

    def fight(self, at, t, *walkers):
        d = self.rng.uniform(*FIGHT)
        self.fights.append((t, t + d, at))
        for w in walkers:
            w.hidden.append((t, t + d))
        return t + d

    def cheer(self, k, t):
        """The winner of a fight at site k cheers before its door; the site is held until they're done."""
        i, j = center(SITES[k][0])
        self.cheers.append((t, t + CHEER, (i + 0.35, j + 0.35)))
        self.claims[k] = t + CHEER

    def free(self, k, t):
        return self.claims.get(k, 0.0) <= t

    def blocked(self, t, except_site=None):
        return {s for k, (s, _) in enumerate(SITES) if k != except_site and self.state(k, t)[1] > 0}

    # A red walker's visit.
    def red_visit(self, t):
        w = Walker("red", t, center(self.edge_tile()))
        self.walkers.append(w)
        self.wander(w, self.rng.randint(1, 4))
        empty = [k for k in range(len(SITES)) if self.state(k, w.now)[1] == 0 and self.free(k, w.now)]
        ours = [k for k in range(len(SITES)) if self.state(k, w.now)[0] == "green" and self.free(k, w.now)]
        r = self.rng.random()
        here = (int(w.at[0]), int(w.at[1]))
        if empty and r < 0.4 and t < SETTLE_UNTIL:
            k = self.rng.choice(empty)
            w.follow(path(here, SITES[k][0], self.blocked(w.now, k)))
            assert self.state(k, w.now)[1] == 0
            w.end = w.now
            self.claims[k] = w.end
            self.set_site(k, w.now, "red", 1)
        elif ours and r < 0.7:
            k = self.rng.choice(ours)
            w.follow(path(here, SITES[k][0], self.blocked(w.now, k)))
            # The site turns to a fight ring; the Omarchy side holds it.
            end = self.fight(center(SITES[k][0]), w.now, w)
            self.site_hidden[k].append((w.now, end))
            w.end = end
            self.cheer(k, end)
        else:
            # A challenge at the King's gate.
            approach = path(here, (4, 7), self.blocked(w.now))
            w.follow(approach)
            w.walk(GATE_OUT)
            arrive = max(w.now, self.king_free)
            if arrive > w.now:
                w.wait(arrive - w.now)
            self.king_duel(w, arrive)

    def king_duel(self, w, t):
        """The King comes out of his keep, fights at his gate, wins, and goes back in."""
        out_at = t - 1.2
        start = t
        end = self.fight(GATE_OUT, start, w)
        back = end + CHEER + 1.2
        self.king.append((out_at, start, end, back))
        self.king_free = back + 2.0
        w.end = end

    # An Omarchy settler from the castle.
    def green_sally(self, t):
        red = [k for k in range(len(SITES)) if self.state(k, t)[0] == "red" and self.free(k, t)]
        empty = [k for k in range(len(SITES)) if self.state(k, t)[1] == 0 and self.free(k, t)]
        if not red and not empty:
            return
        w = Walker("green", t, GATE_IN)
        self.walkers.append(w)
        w.walk(GATE_OUT)
        here = (4, 7)
        if red:
            k = min(red, key=lambda k: abs(SITES[k][0][0] - 4) + abs(SITES[k][0][1] - 6) + self.rng.random() * 3)
            w.follow(path(here, SITES[k][0], self.blocked(w.now, k)))
            end = self.fight(center(SITES[k][0]), w.now, w)
            self.site_hidden[k].append((w.now, end))
            self.take_over(k, end, "green")
            w.end = end
            self.cheer(k, end)
        else:
            k = self.rng.choice(empty)
            w.follow(path(here, SITES[k][0], self.blocked(w.now, k)))
            assert self.state(k, w.now)[1] == 0
            w.end = w.now
            self.set_site(k, w.now, "green", 1)
        self.claims[k] = max(self.claims.get(k, 0.0), w.end)

    def audit(self):
        """Checks the rules the campaign must keep: the Omarchy side never loses a settlement, and a rival's
        settlement only changes hands at the end of a fight there; no two fights share a site at once."""
        for k, history in enumerate(self.sites):
            prev = None
            for when, owner, _ in history:
                if prev is not None and owner != prev:
                    assert prev == "red" and owner == "green", f"site {k} went from {prev} to {owner} at {when:.1f}"
                    assert any(abs(b - when) < 1e-6 for _, b in self.site_hidden[k]), \
                        f"site {k} changed hands without a fight at {when:.1f}"
                prev = owner if owner is not None else prev
            spans = sorted(self.site_hidden[k])
            assert all(o == "green" for _, o, _ in history if _ >= SEGFAULT[0] - 60), f"site {k} isn't Omarchy's by the disasters"
            for (a0, b0), (a1, _) in zip(spans, spans[1:]):
                assert a1 >= b0, f"two fights overlap at site {k} at {a1:.1f}"

    def band(self, t, n, gap):
        """A rival band: n walkers come in from the map's edges, `gap` seconds apart."""
        out = []
        for k in range(n):
            w = Walker("red", t + k * gap, center(self.edge_tile()))
            self.walkers.append(w)
            out.append(w)
        return out

    def segfault(self):
        """A band wanders in; the land shakes and cracks open under each of them, and they're gone."""
        at, cost = SEGFAULT
        self.casts.append((at, cost))
        self.quake = (at, at + 3.0)
        for w in self.band(at - 16.0, 3, 2.5):
            while w.now < at + 2.0:
                self.wander(w, 1, GATE_AREA)
            p = w.where(at + 0.9)
            self.cracks.append((at + 0.5, at + 4.5, p))
            w.end = at + 1.3

    def dephell(self):
        """Swamps open on empty land; a band comes in, and each walks into one and sinks, and that swamp dries up."""
        at, cost = DEPHELL
        self.casts.append((at, cost))
        open_ = {}
        for k, tile in enumerate(SWAMPS):
            open_[tile] = at + 0.4 * k
        for w, tile in zip(self.band(at + 5.0, len(SWAMPS), 9.0), SWAMPS):
            self.wander(w, self.rng.randint(1, 2))
            others = set(SWAMPS) - {tile}
            w.follow(path(tile_of(w.at), tile, self.blocked(w.now) | others))
            sink = (w.now, w.now + 1.6)
            self.sinks.append((sink[0], sink[1], center(tile)))
            w.end = sink[0]
            self.swamps.append((open_[tile], sink[1], sink[1] + 1.2, tile))

    def quiet(self, t):
        """Whether a rival's ordinary visit starting at t would cross a disaster."""
        return any(a - 75 <= t <= a + 45 for a, _ in (SEGFAULT, DEPHELL))

    def mana(self):
        """The mana pointer's position through the campaign, as cell numbers on the bar: it rises with the
        Omarchy side's settlements and falls with each cast."""
        out, m, t = [], 0.4, 0.0
        casts = dict(self.casts)
        while t <= LENGTH:
            size = 6 + sum(self.state(k, t)[1] for k in range(len(SITES)) if self.state(k, t)[0] == "green")
            if t in casts:
                out.append((t, m))
                m -= casts[t]
                out.append((t + 0.6, m))
            elif t % 60.0 == 0 or (m < 7.4 and min(m + size * 0.00225, 7.4) == 7.4):
                # A point a minute is plenty for a pointer that creeps, and one where it tops out.
                out.append((t, m))
            m = min(7.4, m + size * 0.00045 * 5.0)
            t += 5.0
        assert all(when % 5.0 == 0 for when in casts), "casts fall on the mana's five-second steps"
        return out

    def run(self):
        self.set_site(3, 0.0, "red", 2)
        self.set_site(5, 0.0, "red", 1)
        assert SITES[5][0] == (1, 7)
        events = []
        t = 4.0
        while t < LENGTH - 40:
            if not self.quiet(t):
                events.append((t, "red"))
            t += self.rng.uniform(12, 20)
        t = 20.0
        while t < LENGTH - 40:
            events.append((t, "green"))
            t += self.rng.uniform(22, 32)
        for when, kind in sorted(events):
            (self.red_visit if kind == "red" else self.green_sally)(when)
        self.segfault()
        self.dephell()
        self.audit()


# ---------------------------------------------------------------------------------------------------------------
# Writing the pack


def num(v):
    """A number, briefly: to four decimal places, without trailing zeros."""
    out = f"{v:.4f}".rstrip("0").rstrip(".")
    return "0" if out in ("", "-0") else out


def track(points):
    """An expression of t through (time, value) points, held level beyond its ends, in pieces of up to 16."""
    pts = []
    for when, v in points:
        if pts and when <= pts[-1][0]:
            when = pts[-1][0] + 0.001
        pts.append((when, v))
    if len(pts) == 1:
        return num(pts[0][1])
    pieces = [pts[k:k + 8] for k in range(0, len(pts), 7)]
    pieces = [p for p in pieces if len(p) >= 2] or [pts]
    exprs = ["interp(t, " + ", ".join(f"{num(a)}, {num(b)}" for a, b in p) + ")" for p in pieces]
    out = exprs[-1]
    for p, e in zip(reversed(pieces[:-1]), reversed(exprs[:-1])):
        out = f"(t < {num(p[-1][0])} ? {e} : {out})"
    return out


def steps(changes, default=0):
    """An expression of t that steps to each (time, value) in turn: keyframes a thousandth of a second apart."""
    pts, value = [(-1.0, default)], default
    for when, v in sorted(changes):
        if v == value:
            continue
        pts += [(when - 0.001, value), (when, v)]
        value = v
    return track(pts) if len(pts) > 1 else num(default)


def windows(spans):
    """True during any of `spans`."""
    if not spans:
        return "false"
    return " || ".join(f"(t >= {num(a)} && t < {num(b)})" for a, b in sorted(spans))


def sprite(name, file, frame_size, anchor, x, y, frame="0", flip=None, visible=None, depth=None):
    lines = ["  [[scene.draw.draw]]", '  kind = "sprite"', f'  file = "{file}"',
             f"  frame_size = [{frame_size[0]}, {frame_size[1]}]", f"  anchor = [{anchor[0]}, {anchor[1]}]",
             f'  x = "{x}"', f'  y = "{y}"', f'  frame = "{frame}"']
    if flip:
        lines.append(f'  flip = "{flip}"')
    if visible:
        lines.append(f'  visible = "{visible}"')
    lines.append(f'  depth = "{depth if depth is not None else y}"')
    return f"  # {name}\n" + "\n".join(lines) + "\n"


LONGEST = 2000     # characters in an expression; the renderer takes up to 2048


def longest(element):
    """The length of an element's longest expression."""
    return max(len(line.split('"', 1)[1]) for line in element.split("\n") if '= "' in line)


def lanes(items, span, draw, gap=0.2):
    """Shares out `items` among as few lanes as can hold them, no two in a lane at once; `span(item)` is when an
    item is shown, and `draw(lane)` the element that draws a lane, whose expressions must stay short enough. A scene
    can only draw so many elements, and a lane is drawn by one."""
    out = []
    for item in sorted(items, key=lambda it: span(it)[0]):
        a, _ = span(item)
        for lane in out:
            if span(lane[-1])[1] + gap <= a and longest(draw(lane + [item])) <= LONGEST:
                lane.append(item)
                break
        else:
            assert longest(draw([item])) <= LONGEST
            out.append([item])
    return out


def walker_span(w):
    return (w.born, w.end if w.end is not None else w.keys[-1][0])


def walker_element(n, lane):
    """The walkers of one lane, one after another, as one element."""
    keys, moving, facing, shown, hidden = [], [], [], [], []
    for w in lane:
        a, b = walker_span(w)
        mine = [(t, p) for t, p in w.keys if t <= b]
        if mine[-1][0] < b:
            mine.append((b, w.where(b)))
        keys += mine
        for (t0, pa), (t1, pb) in zip(mine, mine[1:]):
            sa, sb = screen(*pa), screen(*pb)
            moving.append((t0, 1 if sa != sb else 0))
            if sa[0] != sb[0]:
                facing.append((t0, 1 if sb[0] < sa[0] else 0))
        moving.append((b, 0))
        shown.append((a, b))
        hidden += w.hidden
    pts = [(t, stage(screen(*p))) for t, p in keys]
    xs = track([(t, p[0]) for t, p in pts])
    ys = track([(t, p[1]) for t, p in pts])
    visible = windows(shown)
    if hidden:
        visible += " && !(" + windows(hidden) + ")"
    frame = f"{steps(moving)} > 0 ? 1 + floor(t * 6) % 3 : 0"
    team = lane[0].team
    file = "walkers-green.png" if team == "green" else "walkers-red.png"
    return sprite(f"{team} walkers, lane {n}", file, (10, 14), (5, 13), xs, ys, frame, steps(facing) + " > 0",
                  visible, f"{ys} + 0.0001")


def spots(name, file, frame_size, anchor, items, frame, depth_bias):
    """Things shown in one place each for a while (fights, rivals going under), shared out among lanes: `items` are
    (from, to, map point), and `frame` an expression of how far through its time each is, `{u}`."""
    def draw(lane, n=0):
        xs = steps([(a, stage(screen(*p))[0]) for a, _, p in lane], stage(screen(*lane[0][2]))[0])
        ys = steps([(a, stage(screen(*p))[1]) for a, _, p in lane], stage(screen(*lane[0][2]))[1])
        starts = steps([(a, a) for a, _, _ in lane], lane[0][0])
        rates = steps([(a, 1 / (b - a)) for a, b, _ in lane], 1 / (lane[0][1] - lane[0][0]))
        u = f"((t - ({starts})) * ({rates}))"
        return sprite(f"{name}, lane {n}", file, frame_size, anchor, xs, ys, frame.format(u=u),
                      visible=windows([(a, b) for a, b, _ in lane]), depth=f"{ys} + {depth_bias}")
    return [draw(lane, n) for n, lane in enumerate(lanes(items, lambda it: (it[0], it[1]), draw))]


def keyed(rgb, key=(255, 0, 255)):
    """An RGB picture drawn on `key` as RGBA, the key clear."""
    out = Image.new("RGBA", rgb.size, (0, 0, 0, 0))
    src, dst = rgb.load(), out.load()
    for y in range(rgb.height):
        for x in range(rgb.width):
            if src[x, y] != key:
                dst[x, y] = src[x, y] + (255,)
    return out


def phases(spans, default):
    """An expression of t: each (from, to, expression) in force over its span, `default` otherwise."""
    out = default
    for a, b, e in reversed(spans):
        out = f"(t >= {num(a)} && t < {num(b)} ? {e} : {out})"
    return out


def build(out):
    os.makedirs(out, exist_ok=True)
    camp = Campaign()
    camp.run()

    # The background: the tabletop and the frame around the map, with the penguin holding up the King's shield.
    bg = Image.new("RGB", (art.W, art.H), art.PAL["black"])
    table.draw_table(bg)
    table.draw_rim(bg)
    ui.draw_plates(bg)
    ui.draw_book(bg, ui.world_map())
    ui.draw_bar(bg)
    ui.draw_penguin(bg, False)
    ui.draw_shield(bg, *ui.KING_SHIELD)
    ui.draw_penguin(bg, True)
    ui.draw_population(bg, 0, 0)
    bg.save(os.path.join(out, "table.png"))
    # The land, on its own layer so that it can shake.
    land = Image.new("RGB", (art.W, art.H), (255, 0, 255))
    terrain.draw_land(land, H)
    table.draw_back_rim(land, H)
    land = keyed(land)
    land_box = land.getbbox()
    land.crop(land_box).save(os.path.join(out, "land.png"))
    # The water's waves, drawn again at each phase over the land.
    water = []
    for k in range(4):
        rgb = Image.new("RGB", (art.W, art.H), (255, 0, 255))
        terrain.draw_land(rgb, H, water_phase=k * 2)
        src = rgb.load()
        waves = (art.PAL["water"], art.PAL["water_deep"], art.PAL["water_light"], art.PAL["water_crest"])
        for y in range(art.H):
            for x in range(art.W):
                if src[x, y] not in waves:
                    src[x, y] = (255, 0, 255)
        water.append(keyed(rgb))
    box = water[0].getbbox() or (0, 0, 1, 1)
    sprites.sheet([w.crop(box) for w in water]).save(os.path.join(out, "water.png"))
    # The mana pointer, under the bar's first cell; it moves a cell at a time along the bar.
    pointer = Image.new("RGB", (art.W, art.H), (255, 0, 255))
    ui.draw_pointer(pointer, 0)
    pointer = keyed(pointer)
    pointer_box = pointer.getbbox()
    pointer.crop(pointer_box).save(os.path.join(out, "pointer.png"))
    sprites.walkers("green").save(os.path.join(out, "walkers-green.png"))
    sprites.walkers("red").save(os.path.join(out, "walkers-red.png"))
    sprites.king().save(os.path.join(out, "king.png"))
    sprites.magnets().save(os.path.join(out, "magnets.png"))
    sprites.fight_frames().save(os.path.join(out, "fight.png"))
    sprites.flags().save(os.path.join(out, "flags.png"))
    sprites.buildings().save(os.path.join(out, "settlements.png"))
    sprites.castle().save(os.path.join(out, "castle.png"))
    sprites.trees().save(os.path.join(out, "trees.png"))
    sprites.crops().save(os.path.join(out, "crops.png"))
    sprites.swamps().save(os.path.join(out, "swamps.png"))
    sprites.cracks().save(os.path.join(out, "cracks.png"))
    sprites.sinking().save(os.path.join(out, "sinking.png"))

    # On the ground, under everything standing: crops, swamps and cracks.
    ground = []
    for k, ((_, _), history, fields) in enumerate(zip(SITES, camp.sites, CROPS)):
        size = steps([(t, s) for t, _, s in history])
        owner = steps([(t, 1 if o == "red" else 0) for t, o, _ in history])
        for f, (i, j) in enumerate(fields):
            x, y = stage(art.corner(i, j, H[i][j]))
            ground.append(sprite(f"site {k}'s field {f}", "crops.png", (32, 16), (16, 0), num(x), num(y), owner,
                                 visible=f"{size} > {f + 1}"))
    for n, (opens, swallows, dries, (i, j)) in enumerate(camp.swamps):
        x, y = stage(art.corner(i, j, H[i][j]))
        frame = phases([(opens, opens + 0.3, "0"), (opens + 0.3, opens + 0.6, "1"), (opens + 0.6, opens + 0.9, "2"),
                        (opens + 0.9, swallows, "3 + floor(t * 2) % 2"), (swallows, swallows + 0.4, "2"),
                        (swallows + 0.4, swallows + 0.8, "1")], "0")
        ground.append(sprite(f"swamp {n}", "swamps.png", (32, 16), (16, 0), num(x), num(y), frame,
                             visible=f"t >= {num(opens)} && t < {num(dries)}"))
    for n, (opens, closes, at) in enumerate(camp.cracks):
        x, y = stage(screen(*at))
        frame = phases([(opens, opens + 0.1, "0"), (opens + 0.1, opens + 0.2, "1"), (opens + 0.2, opens + 0.3, "2"),
                        (opens + 0.3, closes - 0.6, "3"), (closes - 0.6, closes - 0.4, "2"),
                        (closes - 0.4, closes - 0.2, "1")], "0")
        ground.append(sprite(f"crack {n}", "cracks.png", (28, 14), (14, 7), num(x), num(y), frame,
                             visible=f"t >= {num(opens)} && t < {num(closes)}"))

    # The land's things, drawn back to front by where they stand.
    ent = []
    cx, cy = stage(art.corner(6, 6, 2))
    # Sorted by the middle of its front walls: what stands behind it is drawn first, what stands at its gate after.
    ent.append(sprite("the King's castle", "castle.png", (120, 100), (60, 96), num(cx), num(cy),
                      depth=num(stage(screen(4.5, 5.6))[1])))
    for name, at, frame in (("the Omarchy mark", MAZE_AT, 0), ("the frown", FROWN_AT, 1)):
        x, y = stage(screen(*at))
        ent.append(sprite(name, "magnets.png", (15, 17), (7, 16), num(x), num(y), str(frame)))
    for (at, kind) in TREES:
        x, y = stage(screen(*at))
        ent.append(sprite("a tree", "trees.png", (14, 20), (7, 19), num(x), num(y), str(kind)))
    for k, ((tile, _), history) in enumerate(zip(SITES, camp.sites)):
        x, y = stage(screen(*center(tile)))
        size = steps([(t, s) for t, _, s in history])
        owner = steps([(t, 1 if o == "red" else 0) for t, o, _ in history])
        hidden = windows(camp.site_hidden[k])
        vis = f"{size} > 0 && !({hidden})"
        ent.append(sprite(f"site {k}", "settlements.png", (32, 34), (16, 31), num(x), num(y), f"{size} - 1",
                          visible=vis))
        fx, fy = stage((screen(*center(tile))[0] + 8, screen(*center(tile))[1] - 6))
        ent.append(sprite(f"site {k}'s flag", "flags.png", (10, 14), (1, 13), num(fx), num(fy),
                          f"{owner} * 2 + floor(t * 3) % 2", visible=vis, depth=num(y + 0.001)))
    for team in ("green", "red"):
        mine = [w for w in camp.walkers if w.team == team]
        for n, lane in enumerate(lanes(mine, walker_span, lambda lane: walker_element(0, lane))):
            ent.append(walker_element(n, lane))
    ent += spots("fights", "fight.png", (24, 20), (12, 17), camp.fights, "floor(t * 7) % 4", "0.002")
    ent += spots("cheers", "walkers-green.png", (10, 14), (5, 13), camp.cheers, "4 + floor(t * 4) % 2", "0.003")
    ent += spots("rivals going under", "sinking.png", (12, 14), (6, 13), camp.sinks, "min(4, floor({u} * 5))", "0")
    # The King: out of his gate to each fight and back, a lane of sorties an element.
    def king(lane, n=0):
        kx, shown, flips = [], [], []
        cheering = []
        for out_at, start, end, back in lane:
            kx += [(out_at, GATE_IN), (start, GATE_OUT), (end + CHEER, GATE_OUT), (back, GATE_IN)]
            shown += [(out_at, start), (end, back)]
            cheering.append((end, end + CHEER))
            flips += [(out_at, 0), (end + CHEER, 1)]
        pts = [(t, stage(screen(*p))) for t, p in kx]
        xs, ys = track([(t, p[0]) for t, p in pts]), track([(t, p[1]) for t, p in pts])
        flip, vis, cheers = steps(flips) + " > 0", windows(shown), windows(cheering)
        # The small mark he carries, a little rally mark of his own. Its pole's foot, (6, 16) in
        # its frame, sits in his hand, two pixels ahead of his feet and nine up.
        # Cheering, he holds it high: its pole's foot in his raised hand, seventeen pixels above his feet.
        return (sprite(f"the King, lane {n}", "king.png", (12, 18), (6, 17), xs, ys,
                       f"({cheers}) ? 4 + floor(t * 4) % 2 : 1 + floor(t * 6) % 3", flip, vis) +
                sprite(f"the King's mark, lane {n}", "magnets.png", (15, 17), (4, 25), xs, ys, "2", flip,
                       f"({vis}) && !({cheers})", f"{ys} + 0.0002") +
                sprite(f"the King's mark held high, lane {n}", "magnets.png", (15, 17), (6, 33), xs, ys, "2", flip,
                       cheers, f"{ys} + 0.0002"))
    for n, lane in enumerate(lanes(camp.king, lambda s: (s[0], s[3]), king)):
        ent.append(king(lane, n))

    def nest(text):
        return text.replace("[[scene.draw.draw]]", "[[scene.draw.draw.draw]]")
    lx, ly = stage((land_box[0], land_box[1]))
    water_x, water_y = stage((box[0], box[1]))
    a, b = camp.quake
    shake = f"t >= {num(a)} && t < {num(b)} ? floor(sin(t * 23) * 1.5 + 0.5) / 224 : 0"
    parts = [f"""# The land and everything on it, which the segfault shakes.
[[scene.draw]]
kind = "group"
x = "{shake}"

  [[scene.draw.draw]]
  kind = "sprite"
  file = "land.png"
  x = {num(lx)}
  y = {num(ly)}

  [[scene.draw.draw]]
  kind = "sprite"
  file = "water.png"
  frame_size = [{box[2] - box[0]}, {box[3] - box[1]}]
  x = {num(water_x)}
  y = {num(water_y)}
  frame = "floor(t * 2) % 4"

  [[scene.draw.draw]]
  kind = "group"

""" + nest("\n".join(ground)) + """
  [[scene.draw.draw]]
  kind = "group"
  sort = true

""" + nest("\n".join(ent))]

    # The mana pointer, moving along the bar.
    m = camp.mana()
    px0, py0 = pointer_box[0], pointer_box[1]
    parts.append(f"""[[scene.draw]]
kind = "sprite"
file = "pointer.png"
x = "{track([(t, stage((px0 + 16 * v, py0 + 8 * v))[0]) for t, v in m])}"
y = "{track([(t, stage((px0 + 16 * v, py0 + 8 * v))[1]) for t, v in m])}"
""")

    # The population bars beside the shield: each side's share of the settled land, the castle counting for the
    # Omarchy side.
    for x0, side in ((185, "green"), (241, "red")):
        changes = []
        times = sorted({t for hist in camp.sites for t, _, _ in hist})
        for t in times:
            owned = sum(1 for k in range(len(SITES)) if camp.state(k, t)[0] == side)
            settled = sum(1 for k in range(len(SITES)) if camp.state(k, t)[1] > 0) + 1
            share = (owned + (1 if side == "green" else 0)) / settled
            changes.append((t, round(31 * share)))
        fill = steps(changes, 0)
        cx_, top = stage((x0 + 4, 33))
        parts.append(f"""[[scene.draw]]
kind = "rect"
x = {num(cx_)}
y = "{num(top)} - ({fill}) / 448"
w = {num(4 / 224)}
h = "({fill}) / 224"
color = "#009400"
""")
    toml = f'''# Distro Wars: a parody of a popular God-game, with the Omarch King as the leader, the Omarchy mark as the rally
# point, and the Omarchy tribe in green. Every pixel is drawn by tools/distro-wars; nothing is taken from any game.
# The campaign is simulated there with a fixed seed, and replayed here as a function of time.
format = 1
name = "Distro Wars"
author = "ScreenSafe"
license = "MIT"
description = "The Omarch King defends his realm from rivals. One by one, settlements fly Omarchy green. Eventually, disaster strikes in the form of segfault and Dependency Hell."
pixels = 224
preview = 150
fps = 30
fade = 0
background = "#000000"

[[scene]]
name = "The campaign"
duration = {num(LENGTH)}
fade = 1.5

[[scene.draw]]
kind = "sprite"
file = "table.png"
anchor = [128, 112]

''' + "\n".join(parts)
    with open(os.path.join(out, "screensafe.toml"), "w") as f:
        f.write(toml)
    print(f"{len(camp.walkers)} walkers, {len(camp.fights)} fights, {len(camp.king)} duels; "
          f"sites at the end: {[camp.state(k, LENGTH) for k in range(len(SITES))]}")


if __name__ == "__main__":
    build(sys.argv[1])
