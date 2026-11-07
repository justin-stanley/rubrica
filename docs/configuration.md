---
title: Configuration
description: The site.toml keys and the flags of rubrica check and rubrica build.
---

# Configuration

`rubrica check` needs no configuration, and `rubrica build` uses a neutral site (named "Wiki",
in English, with "Recent changes" and "Search" in the header) unless you give it a settings
file with `--config`. Both commands read the same file: `check` uses its `[site]` section
and ignores the rest.

## site.toml

Only `name` is required. An unknown key is an error, so a typo is found rather than ignored.

| key | type | meaning |
|---|---|---|
| `name` | string, required | The site's name: the wordmark and the suffix of every page title. |
| `base_url` | string, optional | The site's origin, such as `https://wiki.example.org`, with no path and no trailing slash. Used for canonical links and the feed's absolute URLs. |
| `lang` | string, optional | The `lang` of every page. `en` when absent. Letters, digits and `-`. |
| `version` | string, optional | The release the site describes, shown in the footer. |
| `license` | string, optional | The content's licence, shown in the footer. |
| `source_url` | string, optional | Where the source lives, shown in the footer. `https://` only. |
| `feed_url` | string, optional | Where the changelog feed is written: `/name.xml`, not under a directory beginning with `_`. Set together with `feed_source`, or not at all. |
| `feed_source` | string, optional | The page the feed is made from, a `.md` file (one entry per `##` heading). Needs `base_url`. |
| `edit_url` | string, optional | The prefix of each page's "Edit" link; the page's file path is appended. `https://` only. |
| `history_url` | string, optional | The prefix of each page's "History" link, likewise. |
| `header_links` | list, optional | Links in the header after the wordmark, each `{ label, href }`. An `href` is `https://` or root-relative (`/docs`). |

Every URL is checked: it must be `https://` (or, for `header_links`, root-relative) and may
hold no blank, control character, quote, backslash, backtick, `<` or `>`. No setting can
therefore write a `javascript:` link into the site.

### The `[site]` section

`[site]` says which files are pages and how they render. It is optional, every key in it is
optional, and an absent key has the default shown. An unknown key is an error that names it.
Put it after the top-level keys and `header_links`, since TOML ends a table at the next header.

| key | type | default | meaning |
|---|---|---|---|
| `ignore_dirs` | list of strings | `["node_modules"]` | Directory names that are never page directories, at any depth. A directory starting with `.` is always skipped. |
| `ignore_files` | list of strings | `["README.md"]` | Repository-relative files that are not pages. |
| `wiki_origin` | string | none | The wiki's own origin, such as `https://wiki.example.org`, with no path. A link to `<origin>/…` is read as the internal link `/…`. |
| `render` | `"native"` or `"wikijs-compat"` | `"native"` | The Markdown rules the site renders with; see [the compatibility profile](/spec/compat-profile). Any other value is an error that lists both. |
| `wikijs_limits` | boolean | `false` | Apply the limits of the wiki software Rubrica can import from, as `rubrica check --wikijs-limits` does. |

A wiki migrated from that software selects the compatibility profile and keeps its lists:

```toml
name = "Wiki"

[site]
render = "wikijs-compat"
ignore_dirs = ["node_modules"]
ignore_files = ["README.md"]
wiki_origin = "https://wiki.example.org"
```

An example:

```toml
name = "Example Wiki"
base_url = "https://wiki.example.org"
lang = "en"
version = "1.0"
license = "CC-BY-4.0"
source_url = "https://github.com/example/wiki"
feed_url = "/changelog.xml"
feed_source = "changelog.md"
edit_url = "https://github.com/example/wiki/edit/main/"
history_url = "https://github.com/example/wiki/commits/main/"
header_links = [
  { label = "Recent changes", href = "/_recent" },
  { label = "Search", href = "/_search" },
]
```

## rubrica check

```sh
rubrica check [--strict] [--same-page] [--wikijs-limits] [--config <site.toml>] [--format text|json] <repo-dir>
```

Checks links, anchors, front matter, `@sec` ids and paths. It exits 0 when the repository is
clean (warnings do not fail it), 1 when there is an error, and 2 when the repository cannot be
read or the arguments are wrong.

- `--strict`: a dead anchor is an error, and any warning fails the check.
- `--same-page`: also report dead same-page `](#anchor)` links.
- `--wikijs-limits`: apply the limits of the wiki software Rubrica can import from (column
  lengths, its front matter reading, its path rules).
- `--config`: a `site.toml` as above. Its `[site]` section applies; the other keys are read and
  ignored, and `name` is still required. `--wikijs-limits` and `[site] wikijs_limits` are
  combined: either turns the limits on, and the JSON report's `options.wikijs_limits` shows
  the result.
- `--format`: `text` (the default) or `json`.

## rubrica build

```sh
rubrica build [--rev <rev>] [--subtree <dir>] [--config <site.toml>] <repo-dir> <out-dir>
```

Reads the repository's git tree at a commit and writes a static site into `out-dir`, which
must not exist yet or must be empty. It reads the committed tree, not the working directory,
so commit before you build.

- `--rev`: the commit to build, as an id, a branch or a tag. `HEAD` when absent.
- `--subtree`: build one directory of the tree (such as `docs`) instead of its root.
- `--config`: a `site.toml` as above.

A site needs a home page, `home.md`, and every page needs front matter with a `title`. The
build refuses a repository with errors, prints them as `check` does, and writes nothing. It
exits 0 when the site is written, 1 when the repository has findings it cannot be built with,
and 2 when the repository cannot be read or `out-dir` cannot be written.
