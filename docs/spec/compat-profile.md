---
title: The compatibility profile
description: The rules that differ between Rubrica's native Markdown and the profile for wikis that came from Wiki.js.
---

# The compatibility profile

Rubrica renders with one of two sets of rules. The **native** rules are the default: they
follow CommonMark and GitHub Flavored Markdown. The **wikijs-compat** profile reproduces the
output of Wiki.js, for a wiki that was migrated from it and whose pages were written against
that output. A wiki selects the profile with the `render` key of its `site.toml`
(`render = "wikijs-compat"`; see [configuration](/configuration)). That key arrives in 0.1.0
and does not exist yet: today every site renders with the native rules, and `render` is an
unknown key. The import tool in 0.7.0 sets it for the wikis it imports. Everything not in
the table is the same under both.

| rule | native | wikijs-compat |
|---|---|---|
| soft line break | `\n` (CommonMark) | `<br>\n` (markdown-it `breaks: true`) |
| strikethrough | `~` or `~~` (GFM) | `~~` only; a single tilde is text |
| task items | GFM: `<li><input type="checkbox" disabled checked>` | markdown-it-task-lists's markup and loose rules (`wikijs_task_items`) |
| heading ids | Rubrica slug: NFKC, lowercase, alphanumerics and `-`, deduplicated `-1`, `-2`; current Unicode | uslug port with `h-` prefix, Unicode pinned |
| autolinks | GFM extended (`www.`, scheme, email) | the same; never bare domains |
| raw HTML | filtered to the allow-list, then ammonia | the same |
| footnotes | GFM markup, back-references capped | the same |
| fence info string | ends at whitespace | ends at a form feed too |
| `@sec` | both; the span is `<span class="sec" id="sec-…">` | both |
| Mermaid | `rubrica-diagram` SVG, or escaped source | the same |
| TOC and headings' `¶` | theme pass, both | both |

## The rules, one by one

**Soft line break.** CommonMark renders a single newline inside a paragraph as a newline,
which a browser shows as a space. Wiki.js enables markdown-it's `breaks` option, which turns
it into `<br>`. A wiki migrated from Wiki.js has pages that rely on the line breaks they were
typed with, so the profile keeps them.

**Strikethrough.** GFM accepts one or two tildes around the struck text. markdown-it accepts
two only, so `~5 minutes` stays text under the profile, where the native rules could read it
as the start of a strikethrough.

**Task items.** A task list item (`- [x] done`) renders in GFM as a list item that begins with
a disabled checkbox. The markdown-it-task-lists plugin, which Wiki.js uses, writes different
markup and has its own rule for which lists are loose. The profile reproduces both.

**Heading ids.** Every heading gets an `id` so it can be linked. The native slug normalizes
to NFKC, lowercases, keeps letters, digits and `-`, and numbers repeats `-1`, `-2`. It follows
the Unicode version of the toolchain, and the specification says so. The profile ports the
slug function Wiki.js uses (uslug), prefixes `h-`, and carries its own Unicode tables so that
links into an old wiki keep working whatever the toolchain.

**Autolinks.** Both profiles use the GFM extended autolink: `www.` addresses, URLs with a
scheme and email addresses become links. Neither links a bare domain such as `example.org`,
which would turn file names and prose into links. See [autolinks](/spec/autolinks).

**Raw HTML.** HTML in a page is filtered to an allow-list of elements and attributes and then
sanitized, in both profiles. No script, event handler, `javascript:` or `data:` link
survives, and an `id` or `class` from raw HTML is dropped so that a page cannot impersonate
the site's own markup.

**Footnotes.** Both use the GFM footnote markup. The number of back-reference links a single
footnote can collect is capped, so that one footnote cited thousands of times cannot make a
page's size explode.

**Fence info string.** The language of a fenced code block is the info string up to the first
whitespace. Under the profile it ends at a form feed as well, which is where Wiki.js's parser
ends it.

**`@sec`.** A section marker comment gives the heading after it a stable anchor,
`<span class="sec" id="sec-…">`, in both profiles. See [writing pages](/markdown).

**Mermaid.** A `mermaid` fence is rendered to SVG by Rubrica's own diagram renderer when the
site was built with one, and is otherwise written as escaped source. Both profiles do the
same.

**Table of contents and the `¶` on headings.** The theme adds both after rendering, from the
headings. They are the same in both profiles.
