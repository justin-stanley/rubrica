"""Build every Rubrica logo asset from one layout and the Alegreya outlines.

The mark is a manuscript capital R in ink beside a red brick wall laid in running bond, and the
name sets its middle, "bric", in red. It is drawn on a 64-unit grid.
"""
import os, re, subprocess, sys, urllib.request
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.boundsPen import BoundsPen

OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

RED, RED_DARK = "#b8321f", "#e0664f"    # red ochre; and lifted for dark grounds
INK, PAPER = "#1c1a17", "#f4eee2"       # warm ink and parchment

FONT_CSS = ("https://fonts.googleapis.com/css2?family=Alegreya:ital,wght@1,700"
            "&text=Rrubica.wk")

def alegreya():
    """Alegreya Bold Italic (SIL Open Font License 1.1), subset to the letters used,
    from Google Fonts."""
    path = os.path.join(OUT, ".alegreya-bold-italic-subset.ttf")
    if not os.path.exists(path):
        css = urllib.request.urlopen(FONT_CSS).read().decode()
        url = re.search(r"url\((https://fonts\.gstatic\.com/[^)]+)\)", css).group(1)
        urllib.request.urlretrieve(url, path)
    return path

FONT = TTFont(alegreya())
GS, CMAP, HMTX = FONT.getGlyphSet(), FONT.getBestCmap(), FONT["hmtx"]

def text_path(text, fills=None):
    """The text as paths in font units (y up), one fill per character if given, with its bounds."""
    x, parts, xmin, xmax, ymin, ymax = 0, [], 1e9, -1e9, 1e9, -1e9
    for i, ch in enumerate(text):
        g = CMAP[ord(ch)]
        pen = SVGPathPen(GS); GS[g].draw(pen)
        b = BoundsPen(GS); GS[g].draw(b)
        if b.bounds:
            x0, y0, x1, y1 = b.bounds
            xmin, xmax = min(xmin, x + x0), max(xmax, x + x1)
            ymin, ymax = min(ymin, y0), max(ymax, y1)
        fill = f' fill="{fills[i]}"' if fills else ""
        parts.append(f'<path{fill} transform="translate({x} 0)" d="{pen.getCommands()}"/>')
        x += HMTX[g][0]
    return "".join(parts), (xmin, ymin, xmax, ymax)

def placed(text, x, baseline, height, fill):
    """The text with its ink box `height` units tall, its left ink edge at x, sitting on baseline.
    `fill` is one colour, or a list with one colour per character."""
    fills = fill if isinstance(fill, list) else None
    glyphs, (x0, y0, x1, y1) = text_path(text, fills)
    fill = "none" if fills else fill
    s = height / y1                       # cap/ascender top to baseline
    return (f'<g fill="{fill}" transform="translate({x - x0 * s:.3f} {baseline:.3f}) '
            f'scale({s:.5f} {-s:.5f})">{glyphs}</g>'), (x1 - x0) * s

# The mark on the 64-unit grid. The italic R is wide, so the wall starts after its ink.
R_X, R_BASE, R_H = 7.0, 47.0, 31.0      # the capital: left edge, baseline, cap height
_, R_W = placed("R", 0, 0, R_H, "#000")
WALL_X0, WALL_X1 = R_X + R_W + 2.5, 57.0
COURSES, MORTAR = 5, 1.4                # five courses spanning the R's cap height
BRICK_H = (R_H - (COURSES - 1) * MORTAR) / COURSES

def wall(x0, x1, top, courses, brick_h, mortar, fill):
    """A running bond: even courses are two whole bricks, odd ones a half, a whole and a half."""
    whole = (x1 - x0 - mortar) / 2
    half = (whole - mortar) / 2
    out = ""
    for k in range(courses):
        y = top + k * (brick_h + mortar)
        widths = [whole, whole] if k % 2 == 0 else [half, whole, half]
        x = x0
        for w in widths:
            out += f'<rect x="{x:.2f}" y="{y:.2f}" width="{w:.2f}" height="{brick_h:.2f}" rx="0.8" fill="{fill}"/>'
            x += w + mortar
    return out

def mark_body(red, ink):
    r, _ = placed("R", R_X, R_BASE, R_H, ink)
    return r + wall(WALL_X0, WALL_X1, R_BASE - R_H, COURSES, BRICK_H, MORTAR, red)

def word_fills(word, red, ink):
    """"bric" in red, the rest of the name in ink."""
    i = word.index("bric")
    return [red if i <= j < i + 4 else ink for j in range(len(word))]

def svg(body, w=64, h=64, px=None, label="Rubrica"):
    px = px or w * 4
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.1f} {h:.1f}" '
            f'width="{px:.0f}" height="{px * h / w:.0f}" role="img" aria-label="{label}">'
            f'<title>{label}</title>{body}</svg>\n')

def tile(fill, stroke=None):
    if stroke:
        return f'<rect x="1.5" y="1.5" width="61" height="61" rx="12" fill="{fill}" stroke="{stroke}" stroke-width="3"/>'
    return f'<rect width="64" height="64" rx="13" fill="{fill}"/>'

def write(name, body):
    with open(os.path.join(OUT, name), "w") as f:
        f.write(body)

# 1. The mark alone, transparent.
write("rubrica-mark-light.svg", svg(mark_body(RED, INK)))
write("rubrica-mark-dark.svg", svg(mark_body(RED_DARK, PAPER)))
# 2. One-colour versions.
write("rubrica-mark-mono.svg", svg(mark_body(INK, INK)))
write("rubrica-mark-mono-inverted.svg", svg(tile(INK) + mark_body(PAPER, PAPER)))
# 3. The app icon: a parchment page with an ink edge.
write("rubrica-icon.svg", svg(tile(PAPER, INK) + mark_body(RED, INK), px=512))
# 4. The favicon: a full wall turns to noise below 48 px, so it is a red tile with two mortar
# joints behind the R, knocked out in parchment.
FAV_H = 44.0
_, fav_w = placed("R", 0, 0, FAV_H, PAPER)
fav_r, _ = placed("R", (64 - fav_w) / 2, 32 + FAV_H / 2, FAV_H, PAPER)
# A red outline round the R cuts the joints short of it, so the letter stays clean at 16 px.
fav_r = fav_r.replace(f'fill="{PAPER}"', f'fill="{PAPER}" stroke="{RED}" stroke-width="70" paint-order="stroke" stroke-linejoin="round"', 1)
joints = "".join(f'<rect x="0" y="{y}" width="64" height="2.2" fill="{PAPER}" opacity="0.6"/>' for y in (20.5, 41.5))
joints += "".join(f'<rect x="{x}" y="{y0}" width="2.2" height="{h}" fill="{PAPER}" opacity="0.6"/>'
                  for x, y0, h in ((50, 0, 20.5), (12, 22.7, 18.8), (50, 43.7, 21)))
write("favicon.svg", svg(tile(RED) + f'<g clip-path="url(#t)">{joints}</g>' + fav_r)
      .replace("<title>", '<defs><clipPath id="t"><rect width="64" height="64" rx="13"/></clipPath></defs><title>', 1))

# 5. Lockups: the mark, then the word, sharing the R's baseline and cap height.
def lockup(word, red, ink, bg=None):
    word_svg, word_w = placed(word, 64 + 5, R_BASE, R_H * 0.98, word_fills(word, red, ink))
    W = 64 + 5 + word_w + 4
    rect = f'<rect width="{W:.1f}" height="64" fill="{bg}"/>' if bg else ""
    return W, rect + mark_body(red, ink) + word_svg

for name, red, ink in (("light", RED, INK), ("dark", RED_DARK, PAPER)):
    W, body = lockup("rubrica", red, ink)
    write(f"rubrica-lockup-{name}.svg", svg(body, W, 64, W * 4))
    word, ww = placed("rubrica.wiki", 2, R_BASE, R_H * 0.98, word_fills("rubrica.wiki", red, ink))
    write(f"rubrica-wordmark-{name}.svg", svg(word, ww + 4, 64, (ww + 4) * 4, "rubrica.wiki"))

# 6. GitHub's social preview (1280x640): the lockup on parchment, and the domain beneath.
W, body = lockup("rubrica", RED, INK)
k = min(1280 * 0.62 / W, 640 * 0.40 / 64)
domain, dw = placed("rubrica.wiki", 0, 0, 22, word_fills("rubrica.wiki", RED, INK))
dk = k * 0.9
write("social-preview.svg",
      f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 640" width="1280" height="640">'
      f'<rect width="1280" height="640" fill="{PAPER}"/>'
      f'<g transform="translate({(1280 - W * k) / 2:.1f} {250 - 32 * k:.1f}) scale({k:.3f})">{body}</g>'
      f'<g opacity="0.72" transform="translate({(1280 - dw * dk * 0.5) / 2:.1f} 470) scale({dk * 0.5:.3f})">{domain}</g>'
      f'<rect x="0" y="616" width="1280" height="24" fill="{RED}"/></svg>\n')

# 7. Rasters (rsvg-convert, from librsvg) and the .ico (Pillow).
def png(src, dst, width):
    subprocess.run(["rsvg-convert", "-w", str(width), os.path.join(OUT, src),
                    "-o", os.path.join(OUT, dst)], check=True)
png("favicon.svg", "favicon-16.png", 16)
png("favicon.svg", "favicon-32.png", 32)
png("favicon.svg", "favicon-48.png", 48)
png("rubrica-icon.svg", "apple-touch-icon.png", 180)
png("rubrica-icon.svg", "rubrica-icon-512.png", 512)
png("rubrica-icon.svg", "rubrica-icon-1024.png", 1024)
png("social-preview.svg", "social-preview.png", 1280)
# The README header as PNG too: some README viewers (the GitHub mobile app) draw no SVG.
png("rubrica-lockup-light.svg", "rubrica-lockup-light.png", 760)
png("rubrica-lockup-dark.svg", "rubrica-lockup-dark.png", 760)
from PIL import Image
Image.open(os.path.join(OUT, "favicon-48.png")).save(
    os.path.join(OUT, "favicon.ico"), sizes=[(16, 16), (32, 32), (48, 48)])
os.remove(os.path.join(OUT, "favicon-48.png"))
os.remove(alegreya())
print("built into", OUT)
