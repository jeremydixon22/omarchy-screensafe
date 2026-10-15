"""The stone tabletop the close-up map sits in, and the raised rim around the map."""
from art import PAL, TILES, TOP, corner


def draw_table(img):
    """Stone tiles on the map's grid, as far as the screen goes, with dotted grout."""
    px = img.load()
    for y in range(img.height):
        for x in range(img.width):
            # Only a cell's width beyond the map's back edges; black above that.
            i = ((x - TOP[0]) / 16 + (y - TOP[1]) / 8) / 2
            j = ((y - TOP[1]) / 8 - (x - TOP[0]) / 16) / 2
            px[x, y] = PAL["stone"] if i >= -1 and j >= -1 else PAL["black"]
    # Grout: dotted lines along both of the grid's diagonals, every other pixel.
    for y in range(img.height):
        for x in range(img.width):
            u = (x - TOP[0]) + 2 * (y - TOP[1])  # constant along lines running down to the left
            v = (x - TOP[0]) - 2 * (y - TOP[1])  # constant along lines running up to the left
            if (u % 32 == 0 or v % 32 == 0) and x % 2 == 0 and px[x, y] != PAL["black"]:
                px[x, y] = PAL["black"]


def draw_back_rim(img, h):
    """The rim behind the land: a band just above the map's two back edges, at the edges' own heights."""
    px = img.load()
    top = {}
    for k in range(TILES):
        for a, b in (((k, 0), (k + 1, 0)), ((0, k), (0, k + 1))):
            pa, pb = corner(*a, h[a[0]][a[1]]), corner(*b, h[b[0]][b[1]])
            for x in range(min(pa[0], pb[0]), max(pa[0], pb[0]) + 1):
                t = (x - pa[0]) / (pb[0] - pa[0]) if pb[0] != pa[0] else 0
                y = round(pa[1] + (pb[1] - pa[1]) * t)
                top[x] = min(top.get(x, y), y)
    for x, y in top.items():
        for d, c in ((1, "rim_mid"), (2, "white"), (3, "white"), (4, "rim_light"), (5, "grout_dark")):
            if 0 <= x < img.width and 0 <= y - d < img.height:
                px[x, y - d] = PAL[c]


def draw_rim(img):
    """The rim: a band around the map's sea-level diamond, light grey either side of a white middle."""
    px = img.load()
    t, r, b, l = corner(0, 0, 0), corner(TILES, 0, 0), corner(TILES, TILES, 0), corner(0, TILES, 0)
    cx = TOP[0]
    for y in range(img.height):
        for x in range(img.width):
            # Distance outside the diamond, in rows: |x - cx| / 2 + |y - cy| measured against the diamond's size.
            cy = (t[1] + b[1]) / 2
            d = abs(x - cx) / 2 + abs(y - cy) - (b[1] - t[1]) / 2
            if 0 <= d < 1:
                px[x, y] = PAL["rim_mid"]
            elif 1 <= d < 3:
                px[x, y] = PAL["white"]
            elif 3 <= d < 4:
                px[x, y] = PAL["rim_light"]
            elif 4 <= d < 5:
                px[x, y] = PAL["grout_dark"]
