"""ZeroFoundry steel letter mark generator.

One formula for every ZeroFoundry product letter (ZeroConverter "C" and whatever
comes next). The lighting, metal, bevel and tile follow the studio mark, so a
letter made here sits next to (or on top of) the other marks with identical light.

Usage:
    python steel_mark.py <glyph> <out-dir>        # writes <glyph>-icon.svg, <glyph>-mark.svg
    python steel_mark.py --list

See STEEL_MARK.md for the rules a new glyph must follow.
"""
import math
import os
import sys

# ── Grid ────────────────────────────────────────────────────────────────────
# Glyph space is the 52-unit square 6..58. Letters live in the box
# x 8..56, y 10..54 (the studio Z fills it exactly). Optical size may shrink
# a glyph inside the box but never outside it.
VIEWBOX = (6, 6, 52, 52)
BEVEL = 1.6          # outer contour → face inset
OUTLINE = "#24343F"  # hairline around the contour
UNDERLAY = "#A8B8C3"

# Face metal: one gradient in absolute glyph space, so every letter is lit
# from the same place. Stops = studio palette. The vector runs along the diagonal of
# the glyph box (top-left → bottom-right), so the dark sheen band sweeps across
# the letter instead of pooling on a single vertical stem.
FINISHES = {
    # App icon / taskbar: brushed steel, brighter highlights.
    "brushed": {
        "face": [(0, "#E7EEF1"), (.23, "#BCCAD3"), (.43, "#94A6B4"), (.64, "#E1E8ED"), (1, "#B7C5CE")],
        "highlight": ("#F6FAFC", .42),
        # bevel colour by outward-normal direction (y down): (outer edge, inner edge)
        "bevel": {
            0: ("#6A7C8A", "#899CA8"),     # → right
            45: ("#485D6D", "#6B8291"),    # ↘
            90: ("#556877", "#768B9A"),    # ↓
            135: ("#8595A0", "#A1B0BB"),   # ↙
            180: ("#BECAD1", "#D4DCE1"),   # ←
            225: ("#DFE9ED", "#F2F6F8"),   # ↖
            270: ("#D3DEE3", "#E7EDF0"),   # ↑
            315: ("#A3B1BA", "#BCC8CF"),   # ↗
        },
    },
    # In-app chrome: satin steel, quieter reflections.
    "satin": {
        "face": [(0, "#DEE6EB"), (.23, "#BBC9D3"), (.43, "#9BADBA"), (.64, "#D9E1E8"), (1, "#B7C5CF")],
        "highlight": ("#DCE5EB", .22),
        "bevel": {
            0: ("#7A8B98", "#92A5B0"),
            45: ("#5E7281", "#7A909E"),
            90: ("#697B89", "#8397A5"),
            135: ("#8F9FAA", "#A6B5C0"),
            180: ("#BDC9D1", "#CED8DE"),
            225: ("#D7E2E8", "#E6EDF0"),
            270: ("#CED9E0", "#DEE5EA"),
            315: ("#A7B5BF", "#BBC8D0"),
        },
    },
}
FACE_VECTOR = (8, 10, 56, 54)  # x1 y1 x2 y2 = glyph box diagonal

# ── Glyphs ──────────────────────────────────────────────────────────────────
# Clockwise outer contour (y down). Rules (see STEEL_MARK.md):
#   stroke 12–13, primary corner chamfer 6, terminal chamfer 4, 45° or the
#   letter's own diagonal only, inner corners sharp.
GLYPHS = {
    "Z": [(14, 10), (50, 10), (56, 16), (56, 22), (30, 42), (54, 42), (54, 48), (48, 54),
          (14, 54), (8, 48), (8, 44), (34, 22), (10, 22), (10, 16)],
    "C": [(17, 10), (49, 10), (53, 14), (53, 23), (24, 23), (24, 41), (53, 41), (53, 50),
          (49, 54), (17, 54), (11, 48), (11, 16)],
}


def _normal(a, b):
    """Outward unit normal of a clockwise (y-down) edge a→b."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    length = math.hypot(dx, dy)
    return dy / length, -dx / length


def _inset(poly, d):
    out = []
    n = len(poly)
    for i in range(n):
        p0, p1, p2 = poly[i - 1], poly[i], poly[(i + 1) % n]
        n1, n2 = _normal(p0, p1), _normal(p1, p2)
        a1 = (p0[0] - n1[0] * d, p0[1] - n1[1] * d)
        a2 = (p1[0] - n1[0] * d, p1[1] - n1[1] * d)
        b1 = (p1[0] - n2[0] * d, p1[1] - n2[1] * d)
        b2 = (p2[0] - n2[0] * d, p2[1] - n2[1] * d)
        den = (a1[0] - a2[0]) * (b1[1] - b2[1]) - (a1[1] - a2[1]) * (b1[0] - b2[0])
        t = ((a1[0] - b1[0]) * (b1[1] - b2[1]) - (a1[1] - b1[1]) * (b1[0] - b2[0])) / den
        out.append((a1[0] + t * (a2[0] - a1[0]), a1[1] + t * (a2[1] - a1[1])))
    return out


def _mix(c1, c2, t):
    a = [int(c1[i:i + 2], 16) for i in (1, 3, 5)]
    b = [int(c2[i:i + 2], 16) for i in (1, 3, 5)]
    return "#%02X%02X%02X" % tuple(round(a[k] + (b[k] - a[k]) * t) for k in range(3))


def _bevel_colours(table, nx, ny):
    ang = math.degrees(math.atan2(ny, nx)) % 360
    lo = int(ang // 45) * 45 % 360
    hi = (lo + 45) % 360
    t = (ang - int(ang // 45) * 45) / 45
    return _mix(table[lo][0], table[hi][0], t), _mix(table[lo][1], table[hi][1], t)


def _path(points):
    return "M" + "L".join("%.3f %.3f" % p for p in points) + "Z"


def glyph_svg(name, finish="brushed", prefix=None):
    """Inner SVG (defs + artwork) for a glyph, in glyph space."""
    outer = GLYPHS[name]
    face = _inset(outer, BEVEL)
    spec = FINISHES[finish]
    p = prefix or f"zf-{name.lower()}-{finish}"
    defs = [
        f'<path id="{p}-outline" d="{_path(outer)}"/>',
        f'<path id="{p}-face" d="{_path(face)}"/>',
        '<linearGradient id="%s-metal" x1="%g" y1="%g" x2="%g" y2="%g" gradientUnits="userSpaceOnUse">%s</linearGradient>'
        % (p, *FACE_VECTOR, "".join(f'<stop offset="{o}" stop-color="{c}"/>' for o, c in spec["face"])),
    ]
    art = [f'<use href="#{p}-outline" fill="{UNDERLAY}"/>']
    hi_col, hi_op = spec["highlight"]
    highlights = []
    n = len(outer)
    for i in range(n):
        a, b = outer[i], outer[(i + 1) % n]
        fa, fb = face[i], face[(i + 1) % n]
        nx, ny = _normal(a, b)
        c_out, c_in = _bevel_colours(spec["bevel"], nx, ny)
        m_out = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
        m_in = ((fa[0] + fb[0]) / 2, (fa[1] + fb[1]) / 2)
        defs.append(
            '<linearGradient id="%s-bevel-%d" x1="%.3f" y1="%.3f" x2="%.3f" y2="%.3f" gradientUnits="userSpaceOnUse">'
            '<stop stop-color="%s"/><stop offset="1" stop-color="%s"/></linearGradient>'
            % (p, i, *m_out, *m_in, c_out, c_in))
        art.append(f'<path d="{_path([a, b, fb, fa])}" fill="url(#{p}-bevel-{i})"/>')
        # Light catches face edges that look up or left (studio rule).
        if ny < -1e-6 or (abs(ny) < 1e-6 and nx < 0):
            highlights.append('<path d="M%.3f %.3fL%.3f %.3f" stroke="%s" stroke-opacity="%s" stroke-width=".22"/>'
                              % (*fa, *fb, hi_col, hi_op))
    art.append(f'<use href="#{p}-face" fill="url(#{p}-metal)"/>')
    art += highlights
    art.append(f'<use href="#{p}-outline" stroke="{OUTLINE}" stroke-width=".5" stroke-linejoin="miter"/>')
    return "<defs>%s</defs>%s" % ("".join(defs), "".join(art))


def mark_svg(name):
    vb = " ".join(str(v) for v in VIEWBOX)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" width="512" height="512" fill="none">'
            f'<title>{name} mark</title>{glyph_svg(name, "satin")}</svg>\n')


def icon_svg(name):
    vb = " ".join(str(v) for v in VIEWBOX)
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="512" height="512">'
            f'<title>{name} icon</title>'
            '<defs><linearGradient id="zf-tile" x1="0" y1="0" x2="1" y2="1">'
            '<stop stop-color="#303840"/><stop offset="1" stop-color="#12171C"/></linearGradient></defs>'
            '<rect x="4" y="4" width="120" height="120" rx="25" fill="url(#zf-tile)" stroke="#5B6873" stroke-width="1"/>'
            f'<svg x="15" y="15" width="98" height="98" viewBox="{vb}" fill="none">{glyph_svg(name, "brushed")}</svg>'
            '</svg>\n')


if __name__ == "__main__":
    if sys.argv[1:] == ["--list"]:
        print(" ".join(GLYPHS))
        sys.exit(0)
    glyph, out_dir = sys.argv[1], sys.argv[2]
    os.makedirs(out_dir, exist_ok=True)
    for kind, fn in (("icon", icon_svg), ("mark", mark_svg)):
        path = os.path.join(out_dir, f"{glyph.lower()}-{kind}.svg")
        open(path, "w", encoding="utf-8").write(fn(glyph))
        print(path)
