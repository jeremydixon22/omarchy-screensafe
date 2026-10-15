"""The close-up map: a heightmap on the corners of 8 x 8 tiles, drawn as a 16-bit console's isometric land."""
from PIL import Image

from art import PAL, TILES, STEP, corner, fill_poly, noise


def heights():
    """Corner heights, h[i][j]: 0 is sea level, where tiles all of whose corners are 0 are water."""
    h = [[1] * (TILES + 1) for _ in range(TILES + 1)]
    for i in range(TILES + 1):
        for j in range(TILES + 1):
            # The mound the King's castle stands on, three tiles a side, in the middle.
            if 3 <= i <= 6 and 3 <= j <= 6:
                h[i][j] = 2
            # The sea, in the far corner, with a beach running down to it.
            if i >= 6 and j <= 2:
                h[i][j] = 0
    # A knoll on the left.
    h[1][6] = 2
    return h


# Textures by how a triangle faces the light, which comes from the upper left. They're regular: a base
# color with a diagonal lattice of dots, darker on lit and flat land, lighter in shade.
def _lattice(base, dot, alt=None):
    def at(x, y):
        r = noise(23, x, y)
        on = x % 2 == 0 and (y + x // 2) % 2 == 0
        if on != (r < 0.12):
            return dot
        if alt is not None and r > 0.93:
            return alt
        return base
    return at


def _grass(band):
    return {
        2: _lattice(PAL["grass_light"], PAL["grass_hi"], PAL["grass_light"]),
        1: _lattice(PAL["grass_light"], PAL["grass"], PAL["grass_hi"]),
        0: _lattice(PAL["grass_light"], PAL["grass"]),
        -1: _lattice(PAL["grass_dim"], PAL["grass_dark"], PAL["grass"]),
        -2: _lattice(PAL["grass_dark"], PAL["grass_night"], PAL["grass_dim"]),
    }[band]


def _beach(band):
    """Land running down to the sea: ochre and olive, patched with grass, darker where it faces away from the light."""
    lit = band > 0
    def at(x, y):
        n = noise(17, x // 2, y // 2)
        if n < 0.18:
            return PAL["grass"] if lit else PAL["grass_dim"]
        k = (x - 2 * y) % 6
        if lit:
            return PAL["dirt_top"] if k == 0 else PAL["dirt_light"] if n < 0.7 else PAL["dirt"]
        return PAL["dirt_light"] if k == 0 else PAL["dirt"] if n < 0.6 else PAL["dirt_mid"]
    return at


def _band(a, b, c):
    """How a triangle with corners a, b, c (each (i, j, h)) faces the light: 2 brightest to -2 darkest."""
    (ax, ay, az), (bx, by, bz), (cx, cy, cz) = a, b, c
    ux, uy, uz = bx - ax, by - ay, (bz - az) * 0.5
    vx, vy, vz = cx - ax, cy - ay, (cz - az) * 0.5
    nx, ny, nz = uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx
    if nz < 0:
        nx, ny, nz = -nx, -ny, -nz
    if abs(nx) < 1e-9 and abs(ny) < 1e-9:
        return 0
    # The light comes from the screen's upper left: from -i (up and to the right on screen) and +j (down and to
    # the left), more from the left.
    lit = (-nx * 0.8 + ny * 1.0) / (nx * nx + ny * ny + nz * nz) ** 0.5
    if lit > 0.35:
        return 2
    if lit > 0.05:
        return 1
    if lit > -0.25:
        return -1
    return -2


def draw_land(img, h, water_phase=0):
    """Draws the map's tiles back to front, then the dirt sides of its front edges."""
    for s in range(2 * TILES - 1):
        for i in range(TILES):
            j = s - i
            if not 0 <= j < TILES:
                continue
            n = (i, j, h[i][j])
            e = (i + 1, j, h[i + 1][j])
            so = (i + 1, j + 1, h[i + 1][j + 1])
            w = (i, j + 1, h[i][j + 1])
            P = lambda c: corner(c[0], c[1], c[2])
            if n[2] == e[2] == so[2] == w[2] == 0:
                fill_poly(img, [P(n), P(e), P(so), P(w)], water_texture(water_phase))
                continue
            # Two triangles, split down the tile's middle from its top corner to its bottom.
            for tri in ((n, w, so), (n, e, so)):
                band = _band(*tri)
                tex = _beach(band) if min(c[2] for c in tri) == 0 else _grass(band)
                fill_poly(img, [P(c) for c in tri], tex)
            # A flat tile shows its edges faintly, as a faint grid.
            if n[2] == e[2] == so[2] == w[2]:
                px = img.load()
                for a, b in ((P(n), P(e)), (P(n), P(w))):
                    steps = 16
                    for k in range(0, steps, 2):
                        x = round(a[0] + (b[0] - a[0]) * k / steps)
                        y = round(a[1] + (b[1] - a[1]) * k / steps)
                        if 0 <= x < img.width and 0 <= y < img.height:
                            px[x, y] = PAL["grass"]
    draw_sides(img, h)


def water_texture(phase):
    """Water: dark blue under rows of lighter waves, which drift with `phase` (0 to 7)."""
    def at(x, y):
        wave = (y * 2 + ((x // 4 + phase) % 8 // 2) - (1 if (x // 2) % 2 else 0)) % 6
        if wave == 0:
            return PAL["water_crest"]
        if wave == 1:
            return PAL["water_light"]
        if wave in (2, 3):
            return PAL["water"]
        return PAL["water_deep"]
    return at


def draw_sides(img, h):
    """The dirt faces of the map's two front edges, from the land down to sea level."""
    def dirt(seed, light):
        """The cliff's earth: ochre in slanted streaks, lighter on the lit left face."""
        def at(x, y):
            k = (x + (2 if light else -2) * (y // 2)) % 5
            r = noise(seed, x, y)
            if light:
                return PAL["dirt_mid"] if k == 0 else PAL["dirt"] if k == 1 or r < 0.2 else PAL["dirt_light"]
            return PAL["dirt_dark"] if k == 0 else PAL["dirt_mid"] if k == 1 or r < 0.2 else PAL["dirt"]
        return at
    # Front left: along j = TILES; front right: along i = TILES.
    for i in range(TILES):
        a, b = (i, TILES), (i + 1, TILES)
        ha, hb = h[a[0]][a[1]], h[b[0]][b[1]]
        if ha or hb:
            pts = [corner(*a, ha), corner(*b, hb), corner(*b, 0), corner(*a, 0)]
            fill_poly(img, pts, dirt(11, True))
            top_edge(img, corner(*a, ha), corner(*b, hb))
    for j in range(TILES):
        a, b = (TILES, j), (TILES, j + 1)
        ha, hb = h[a[0]][a[1]], h[b[0]][b[1]]
        if ha or hb:
            pts = [corner(*a, ha), corner(*b, hb), corner(*b, 0), corner(*a, 0)]
            fill_poly(img, pts, dirt(13, False))
            top_edge(img, corner(*a, ha), corner(*b, hb))


def top_edge(img, a, b):
    px = img.load()
    steps = max(abs(b[0] - a[0]), abs(b[1] - a[1]), 1)
    for k in range(steps + 1):
        x = round(a[0] + (b[0] - a[0]) * k / steps)
        y = round(a[1] + (b[1] - a[1]) * k / steps) + 1
        if 0 <= x < img.width and 0 <= y < img.height:
            px[x, y] = PAL["dirt_top"]
