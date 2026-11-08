# Rubrica logo

The mark is a manuscript capital R in ink beside a red brick wall, laid in running bond. The name
sets its middle, *bric*, in red. *Rubrica* is Latin for red ochre, the earth pigment of red brick
and of the red headings (*rubrics*) scribes painted in manuscripts. A wiki is built the same way,
a page at a time. The mark is a sister to
[calamus](https://github.com/justin-stanley/calamus)'s reed pen.

| file | use |
|---|---|
| `rubrica-lockup-light.svg`, `rubrica-lockup-dark.svg` | mark and name, for light and dark backgrounds |
| `rubrica-lockup-light.png`, `rubrica-lockup-dark.png` | the same at 760 px wide: the README header, for viewers that draw no SVG |
| `rubrica-wordmark-light.svg`, `rubrica-wordmark-dark.svg` | "rubrica.wiki" alone, with "bric" in red |
| `rubrica-mark-light.svg`, `rubrica-mark-dark.svg` | the mark alone, transparent |
| `rubrica-mark-mono.svg`, `rubrica-mark-mono-inverted.svg` | one colour: ink on any light ground, and parchment on an ink tile |
| `rubrica-icon.svg`, `rubrica-icon-512.png`, `rubrica-icon-1024.png`, `apple-touch-icon.png` | the app icon: the mark on parchment with an ink edge |
| `favicon.svg`, `favicon.ico`, `favicon-16.png`, `favicon-32.png` | the favicon: the R knocked out of a red brick tile; a full wall turns to noise below 48 px, so only a few joints show |
| `social-preview.svg`, `social-preview.png` | GitHub's social preview, 1280×640 |

## Colours

| | hex | where |
|---|---|---|
| red ochre | `#b8321f` | the wall and "bric" on light grounds; the favicon tile |
| red ochre, lifted | `#e0664f` | the wall and "bric" on dark grounds |
| ink | `#1c1a17` | the R and the rest of the name on light grounds; the dark ground |
| parchment | `#f4eee2` | the light ground; the R and the rest of the name on dark grounds |

## Type

The R and the name are set in **Alegreya Bold Italic** and converted to outlines, so they render
the same everywhere with no font installed. Alegreya is © Juan Pablo del Peral and Huerta
Tipográfica, under the [SIL Open Font License 1.1](https://openfontlicense.org).

## Rebuilding

Every file is generated from one layout (`build/build.py`, a 64-unit grid), so the variants
can't drift apart:

```sh
python3 -m pip install fonttools pillow   # and rsvg-convert, from librsvg
python3 assets/build/build.py             # writes into assets/; fetches the font subset from Google Fonts
```

The logo is licensed with the rest of the repository (AGPL-3.0-only).
