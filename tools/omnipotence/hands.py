"""Cuts the Hand of Omnipotence's poses out of a sheet generated on chroma green, and makes a sprite sheet of them:
each pose keyed out and despilled, its forearm fading into a soft glow (so the hand emerges from light rather than
ending at a cut), and placed so that the point between thumb and finger falls at the same place in every frame."""
import math
from collections import deque

from PIL import Image, ImageFilter

GLOW = (255, 236, 205)


def key(im):
    """Alpha from how far a pixel is from the chroma green, and the green spill taken out of the edges."""
    im = im.convert("RGB")
    w, h = im.size
    src = im.load()
    out = Image.new("RGBA", (w, h))
    dst = out.load()
    for y in range(h):
        for x in range(w):
            r, g, b = src[x, y]
            spill = g - max(r, b)
            a = 255 if spill <= 20 else 0 if spill >= 90 else round(255 * (90 - spill) / 70)
            g2 = min(g, max(r, b) + 8)
            dst[x, y] = (r, g2, b, a)
    return out


def components(alpha, threshold=128):
    w, h = alpha.size
    px = alpha.load()
    seen = bytearray(w * h)
    out = []
    for y in range(0, h, 2):
        for x in range(0, w, 2):
            if px[x, y] > threshold and not seen[y * w + x]:
                comp, dq = [], deque([(x, y)])
                seen[y * w + x] = 1
                while dq:
                    cx, cy = dq.popleft()
                    comp.append((cx, cy))
                    for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                        if 0 <= nx < w and 0 <= ny < h and not seen[ny * w + nx] and px[nx, ny] > threshold:
                            seen[ny * w + nx] = 1
                            dq.append((nx, ny))
                if len(comp) > 2000:
                    out.append(comp)
    return out


def pose(keyed, comp, fade):
    xs = [p[0] for p in comp]
    ys = [p[1] for p in comp]
    box = (max(0, min(xs) - 4), max(0, min(ys) - 4), max(xs) + 5, max(ys) + 5)
    part = keyed.crop(box)
    w, h = part.size
    # The pinch: the lowest point of the hand, a little toward its right.
    a = part.split()[3].load()
    tip = max(((x, y) for y in range(h) for x in range(w) if a[x, y] > 200), key=lambda p: p[1] + 0.25 * p[0])
    # The forearm comes in from the top left: fade it out along that direction.
    px = part.load()
    for y in range(h):
        for x in range(w):
            d = (x + y) / (w + h)
            f = min(1.0, max(0.0, (d - fade[0]) / (fade[1] - fade[0])))
            f = f * f * (3 - 2 * f)
            r, g, b, al = px[x, y]
            px[x, y] = (r, g, b, round(al * f))
    return part, tip


def sheet(src, dst, poses=(0, 1), fade=(0.12, 0.42), pad=60):
    keyed = key(Image.open(src))
    comps = sorted(components(keyed.split()[3]), key=lambda c: min(p[0] for p in c))
    parts = [pose(keyed, comps[k], fade) for k in poses]
    # Frames the same size, each pose placed so its pinch falls at the same point.
    left = max(t[0] for _, t in parts) + pad
    top = max(t[1] for _, t in parts) + pad
    right = max(p.width - t[0] for p, t in parts) + pad
    bottom = max(p.height - t[1] for p, t in parts) + pad
    fw, fh = left + right, top + bottom
    out = Image.new("RGBA", (fw * len(parts), fh), (0, 0, 0, 0))
    for k, (p, t) in enumerate(parts):
        frame = Image.new("RGBA", (fw, fh), (0, 0, 0, 0))
        frame.alpha_composite(p, (left - t[0], top - t[1]))
        # A soft warm light around the hand, as in the header.
        halo = frame.split()[3].filter(ImageFilter.GaussianBlur(18)).point(lambda v: round(v * 0.55))
        glow = Image.new("RGBA", (fw, fh), GLOW + (0,))
        glow.putalpha(halo)
        glow.alpha_composite(frame)
        out.alpha_composite(glow, (k * fw, 0))
    out.save(dst, optimize=True)
    return (fw, fh), (left, top)


if __name__ == "__main__":
    import sys
    print(sheet(sys.argv[1], sys.argv[2]))
