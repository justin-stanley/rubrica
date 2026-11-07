---
title: Writing pages
description: What renders, the front matter keys, and @sec markers.
---

# Writing pages

## What renders

Pages are CommonMark 0.31.2 with the GitHub Flavored Markdown extensions: tables,
strikethrough, task lists, extended autolinks and footnotes. Raw HTML is filtered to an
allow-list and then sanitized, so a page cannot add a script, an event handler or a
`javascript:` link, and an `id` or `class` written in raw HTML is dropped. The exact rules,
and where they differ from the CommonMark text, are in [the specification](/spec/home).

## Front matter

Every page starts with a YAML block between two `---` lines. Only `title` is required.

| key | meaning |
|---|---|
| `title` | The page's title. Required, not empty. |
| `description` | A one-line summary. |
| `published` | `false` keeps the page off the built site. `true` when absent. |
| `date` | When the page was last updated. |
| `dateCreated` | When the page was created. |
| `tags` | A comma-separated string or a YAML list. |
| `editor` | The editor the page was written in. |
| `redirect_to` | Marks the page as a stub left by a move. |

Other keys are kept and ignored. The block is limited to 64 KiB and 32 levels of nesting.

## `@sec` markers

A `@sec` marker names a section so that links to it survive a heading rename. It is an HTML
comment alone on its line, immediately before an ATX heading. The comment holds the word
`@sec`, then `id=` and an identifier such as `install/linux`, and optionally `tags=` and a
comma-separated list. The heading it precedes gets the anchor `sec-install/linux`.
The rules are strict, because a marker that
silently attaches to nothing is worse than none:

- The comment is alone on its line, and the next line is an ATX heading (`#` to `######`).
  A blank line between them, a setext heading, or anything else loses the heading.
- It has an `id`. Tags are a comma-separated list.
- It is not inside a blockquote, a list item or a footnote definition, and not in code or in
  the front matter.

`rubrica check` reports every marker that does not attach, with the line it is on.

## Code

A fenced block with a language Rubrica knows (`rust`, `js`, `json`, `html`, `python`, and the
rest of syntect's default syntaxes) is highlighted when the site is built: the code is written
as `<span class="hl-…">` elements, and the theme colours them for light and dark. There is no
JavaScript. A block with an unknown language, or none, is written as escaped text.

A line over 2,000 bytes, or a block over 64 KiB, is written as escaped text too, and
`rubrica check` reports it as the warning `code-block-not-highlighted`, with the line of the
fence. It never stops a build.

A page also has a highlighting budget of 16 MiB of highlighted HTML. When the next block would
pass it, that block and every later one on the page are written as escaped text and reported
the same way, so a page of very dense code stays renderable.

## Diagrams

A fenced block with the language `mermaid` is written to the page as escaped source in
`<pre><code class="language-mermaid">`. Until diagram rendering lands (see the roadmap), it is
shown as text; it does not run anything in the reader's browser.
