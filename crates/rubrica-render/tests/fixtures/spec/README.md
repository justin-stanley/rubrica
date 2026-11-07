# Spec files

`commonmark-0.31.2.json` is the CommonMark spec 0.31.2's `spec.json` by John MacFarlane,
CC-BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/); `gfm-0.29.json` is the
GitHub Flavored Markdown spec 0.29's examples (GitHub, CC-BY-SA 4.0). They are test
inputs, not part of the Rubrica binary.

Nine of `gfm-0.29.json`'s base examples (positions 398, 426, 434, 435, 436, 473, 474, 475,
477) carry HTML with nested `<strong>` flattened to one, where CommonMark 0.31.2's twin
example nests it; the writer's spec test lists them as "spec version" and does not compare
them.
