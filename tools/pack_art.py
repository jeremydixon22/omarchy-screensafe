"""Small deterministic TOML writer for the original Seasons and Clockwork art.

Only the generated data packs are loaded at run time. This module uses Python's
standard library and is a development tool, never a pack capability.
"""
import json
import math


def value(v):
    if isinstance(v, bool):
        return str(v).lower()
    if isinstance(v, (float, int)):
        return f"{v:.6f}".rstrip("0").rstrip(".") if isinstance(v, float) else str(v)
    if isinstance(v, (tuple, list)):
        return "[" + ", ".join(value(x) for x in v) + "]"
    return json.dumps(v, ensure_ascii=False)


class Art:
    def __init__(self, header):
        self.parts = [header.strip() + "\n"]

    def raw(self, text):
        self.parts.append(text.strip() + "\n")

    def draw(self, kind, depth=0, **fields):
        self.parts.append("[[scene" + ".draw" * (depth + 1) + "]]\n" +
                          "\n".join(f"{key} = {value(v)}" for key, v in dict(kind=kind, **fields).items()) + "\n")

    def polygon(self, points, fill, depth=0, **fields):
        self.draw("lines", depth, points=points, closed=True, width=0, fill=fill, **fields)

    def ellipse(self, x, y, rx, ry, fill, depth=0, samples=48, **fields):
        points = [(rx * math.cos(i * math.tau / samples), ry * math.sin(i * math.tau / samples))
                  for i in range(samples)]
        self.polygon(points, fill, depth, x=x, y=y, **fields)

    def write(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("\n".join(self.parts), encoding="utf-8")
