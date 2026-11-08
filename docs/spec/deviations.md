---
title: Deviations
description: Every known place where Rubrica's output differs from CommonMark, from GFM, from the reference implementations, or from the renderer it replaced.
---

# Deviations

Rubrica's parser is CommonMark 0.31.2 with the GFM extensions; the GFM specification file it
is tested against is 0.29, which differs from 0.31.2 on nine base examples (the `spec version`
row). Rubrica first rendered with another Markdown library (the previous renderer), configured as Wiki.js 2's markdown-it; the
renderer that replaced it was compared with it on every public test input and on a
private corpus of Wiki.js pages and their stored renders, and every difference the comparison found is a row below, under the
compatibility profile unless the row says otherwise. Inputs are written as test literals
(`\n` is a line end, `\t` a tab). A row marked *unverified against Wiki.js* is a difference
from the previous renderer that no page of that corpus exercises: which side Wiki.js agrees with is not known.
The sanitizer's own removals (scripts, event handlers, `javascript:` and `data:` links, ids
and classes from raw HTML) are not deviations and are not listed; see the
[compatibility profile](/spec/compat-profile) and [autolinks](/spec/autolinks).

| area | input | Rubrica | before, or the reference | status |
|---|---|---|---|---|
| task item line | `- [y]: /w\n  [ ]  \n  z\n` | the task item on the checkbox's line (2): the line markdown-it starts the item's paragraph on | on the item's first line (1), the definition's | the new line is right (the checkbox spacing now agrees) |
| task item line | `- [y]: /w\n  [ ] x [l](/u)\n` | the task item on the checkbox's line (2): the line markdown-it starts the item's paragraph on | on the item's first line (1), the definition's | the new line is right |
| task item line | `- [y]: /w\n  [ ] see [a](/u\n  \"t\") [c](/v)\n` | the task item on the checkbox's line (2): the line markdown-it starts the item's paragraph on | on the item's first line (1), the definition's | the new line is right; `what_follows_a_link_that_spans_lines_is_on_the_line_it_ends_on` expects line 2 |
| task item line | `> 1.  [y]: /w\n>\t[ ] x\n` | the task item on the checkbox's line (2): the line markdown-it starts the item's paragraph on | on the item's first line (1), the definition's | the new line is right; `text_after_a_half_consumed_tab_is_read_at_its_own_byte` expects line 2 |
| task item line | `- [y]: /w\n  [ ] x\n` | the task item on the checkbox's line (2): the line markdown-it starts the item's paragraph on | on the item's first line (1), the definition's | the new line is right; tests/markdown.rs `a_reference_definition_opening_an_item_does_not_hide_its_task_marker` expects line 2 |
| task item line | `- [y]: /w\n  [x] x\n` | the task item on the checkbox's line (2): the line markdown-it starts the item's paragraph on | on the item's first line (1), the definition's | the new line is right; tests/markdown.rs `a_reference_definition_opening_an_item_does_not_hide_its_task_marker` expects line 2 |
| footnote ids | `> - a[^n]\n>\n>   [^n]: [b](/u\n>   \"t\") [c](/v)\n` | `fn-N`/`fnref-N` ids | `fn-NAME`/`fnref-NAME` | Rubrica's footnote ids; the only difference |
| footnote ids | `<!-- @sec id=a/b -->\n## ⚠️ Open item\n\n### Q&A\n\nx[^n]\n\n[^n]: note\n` | `fn-1`/`fnref-1` | `fn-NAME`/`fnref-NAME` | Rubrica's footnote ids; tests/sanitize.rs `ammonia_keeps_the_ids_rubrica_emits` expects the numbered ids |
| footnote ids | `x[^a] y[^a]\n\n### fnref-a-2\n\n[^a]: z\n` | `fnref-1-2` for the second reference, so the heading `fnref-a-2` collides with nothing and no `HeadingIdIsAFootnoteId` is reported | `fn-NAME`/`fnref-NAME` | Rubrica's footnote ids; tests/footnotes.rs `a_heading_id_that_is_a_footnote_id_is_a_finding` now plants the heading `fnref-1-2`, which is the finding |
| footnotes | `# fn-1\n\n[^1]: y\n` | the definition, unreferenced, is not written and nothing in it is collected  | the node dropped | resolved: the same as before |
| footnotes | `x[^1] y[^2]\n\n[^1]: [^2]: b x[^2]\n` | a reference inside an unreferenced definition does not count; ordinals body first, then per written definition (cmark-gfm's order) | counts it, numbers in document order | deliberate |
| footnotes | `x[^1]\n\n[^1]: a\n\n[^1]: b\n` | the first definition wins; the later ones are not written and nothing in them is collected | both kept in place, with a duplicate `fn-1` id (malformed) | deliberate; `Rendered` agrees with the HTML |
| parser, tables | `x\na \| b\n-\|-\n` | a paragraph `x` and a table whose header row is `a \| b`. A header row with no leading pipe interrupts a paragraph only when a table actually opens there: its cell count equals the delimiter row's, and the line would not, at the start of a block, open a list item (`.\n3.\n-\|`), an HTML block or a link reference definition (`f\n[\n-\|\n]:a`), nor end in a hard break (`\|\n\\\n-\|`); otherwise the paragraph continues, as upstream's does. A header on a lazy continuation line opens the table inside the quote (`>\|\n\|\|\n>-\|`) or the list item (`+ u\n\|2\n  -\|\n2\|`: the table inside the item, `2\|` a paragraph after it), as cmark-gfm does | upstream pulldown-cmark: one paragraph, unless the header row starts with a pipe and is not a lazy line | deliberate: a table with no leading pipe interrupts a paragraph, as in cmark-gfm and markdown-it; the same holds with reference definitions and a `=` line before the table, for a one-cell header (`=` before `\|-`), and through a lazy line; the fuzz targets skip exactly these tables (`fuzz/common.rs`) |
| parser, offsets | ` - >\t-` | the inner list and item start at 5, inside their quote (3..6): a block's range starts at the byte where its indent starts | the inner list and item at 2..6, before the quote: the indent's columns, the rest of a TAB the quote marker consumed, are subtracted from the marker's byte offset | fixed: upstream 0.13.6 breaks the ranges' nesting; tests/offsets.rs `a_list_item_indented_by_a_partial_tab_starts_inside_its_quote` |
| link line | `a \\\| b [l](/u\n\"t\") [m](/v)\n\|c\|\n\|-\|\n` | the second link on line 2 | line 1 (the previous renderer's own position, which the old path flagged as approximate) | the new line is right |
| autolinks | `<https://foo.bar/baz bim>\n` | text: after an `<` that opens no angle autolink the extended-autolink scanner links nothing | the bare URL linked | GFM 0.29 example; pinned in tests/writer_spec.rs |
| autolinks | `<http://foo.bar/baz bim>\n` | text: after an `<` that opens no angle autolink the extended-autolink scanner links nothing | the bare URL linked | GFM 0.29 example; pinned in tests/writer_spec.rs |
| autolinks | `<foo\\+@bar.example.com>\n` | `+@bar.example.com` linked as an e-mail address (`mailto:`) | text | pinned in tests/writer_spec.rs |
| nesting depth | a task item inside 100 nested quotes (`gen-task-in-100-quotes`, tests/render_v2.rs) | nested on; the task item is one | stops nesting lists at its MAX_LIST_DEPTH 100 (counting every open container) and writes `<p>- [ ] a</p>` | parser; the writer's own container bound is 100 |
| nesting depth | 150 nested lists (`gen-150-nested-lists`, tests/render_v2.rs) | nested on (the writer's container bound, 100, drops the tags past it and keeps the content) | stops nesting at MAX_LIST_DEPTH 100 and writes the rest as text | parser |
| nesting depth | a footnote definition inside 150 nested quotes (`gen-footnote-in-150-quotes`, tests/render_v2.rs) | the definition, past the container bound, is unlinked and not written | hoisted and linked | deliberate |
| raw HTML, escape mode | `<sub>2</sub> [x](javascript:alert(1))` with `RawHtml::Escape` | `<a>x</a>`: the dangerous href dropped, as the default mode drops it | `<a href="">x</a>` (the previous renderer blanked it) | as safe; tests/sanitize.rs `escape_mode_writes_raw_html_as_text_and_still_runs_the_final_pass` expects `<a>x</a>` and asserts no `javascript:` |
| footnotes | a heading, link or task item inside a footnote definition the writer does not write (a duplicate, unreferenced, or past the container bound) | not collected: every heading id in `Rendered::headings` is in the HTML | unreferenced: dropped; duplicates: kept; too deep: hoisted | fixed before release; `nothing_from_an_unwritten_footnote_definition_reaches_rendered` |
| footnote ids | every footnote | `fn-N`, `fnref-N`, `fnref-N-K`: N the order of the name's first reference, K the reference's number | `fn-NAME`, `fnref-NAME` | Rubrica's footnote ids, stable when a footnote is renamed |
| autolinks | `a@b.c1`; `www.a.com'`, `www.a.com"` | a link; links ending in the quote | text; links without the quote (cmark-gfm) | documented deviations from cmark-gfm, see [autolinks](/spec/autolinks) |
| autolinks | `[www.a.com]`; `1http://x` | text | links (cmark-gfm) | suspected deviations, settled by reading cmark-gfm's source, not by a run: see [autolinks](/spec/autolinks) |
| spec version | GFM 0.29 base examples 398, 426, 434, 435, 436, 473, 474, 475, 477 (positions in `gfm-0.29.json`) | CommonMark 0.31.2's HTML: the nested `<strong>` of its examples 389, 417, 425, 426, 427, 464, 465, 466, 468 | GFM 0.29's single `<strong>` | the parser is CommonMark 0.31.2; the GFM spec file is 0.29 and differs on these 9 base examples (tests/writer_spec.rs `SPEC_VERSION`) |
| writer | a fence info string with a tab, `` ```a\tb `` | the language word ends at the tab | — | a deviation from pulldown-cmark, which ends it at a space only; cmark-family writers also cut at a tab |
| writer | `'` in a title or alt attribute | written raw, as cmark does | — | pulldown-cmark writes `&#39;` |
| writer | a `%` in a link destination that starts no `%XX` escape (`[x](a%2)`) | `%25` (`href="a%252"`), as markdown-it and the previous renderer write it, in both profiles | the same | cmark keeps it raw; one rule for the HTML and the site's link checks (`rubrica_render::writer::escape_href`) |
| writer | `^` in a link destination | left raw, as pulldown-cmark does | — | cmark writes `%5E` |
| parser | U+0000 (NUL) | the parser passes it through and the writer writes it raw; `render` writes U+FFFD in text, code and attributes | the previous renderer replaced it before the sanitizer | CommonMark §2.3 asks for U+FFFD; the final html5ever pass also replaces it in text |
| parser | an indented code block at the end of the input | no final newline in its content | — | pulldown-cmark's behaviour |
| parser | `\r\n` and a bare `\r` | `render` reads both as `\n` before parsing, as markdown-it does; lines and offsets are the normalised body's | the same | the parser alone reads them as line ends itself |
| positions | a merged run of text (consecutive text events) | only the run's start offset is exact; its range spans stripped indentation and escapes | — | checked per parser `Text` or per piece, not per merged run |
| heading ids, native | a heading whose slug is a DOM-clobbering name (`# Location`) | no id | — | for safety over GitHub parity |
| `@sec` | a marker next to raw HTML | markers are taken before the raw-HTML filter runs, as before | the same | order kept from the previous renderer's path |
| raw HTML | a raw HTML block | filtered per block, its lines joined, as before | the same | |
