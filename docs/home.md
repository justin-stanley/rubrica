---
title: Rubrica
description: A wiki for Markdown repositories, written in Rust, with no JavaScript of its own.
---

# Rubrica

Rubrica is wiki software written entirely in Rust. A wiki is a git repository of Markdown
pages; Rubrica checks the repository for dead links, bad front matter and broken anchors, and
builds it into a static site. The parser, the renderer, the sanitizer and the site generator
are Rubrica's own code. The pages it writes carry no JavaScript, except the search page, which loads the WebAssembly
search Rubrica ships (one generated loader, under a policy that admits it on that page alone).

The first release is a command-line tool: `rubrica check` reports what is wrong with a
repository, and `rubrica build` writes the site for a commit. A server that serves a
repository directly, editing through a signed-in browser session, permissions, search and
collaboration follow in later releases, in that order.

## Where to go next

- [Install Rubrica](/install): the release binary, `cargo install` and the container image,
  and how to verify what you downloaded.
- [Configure a site](/configuration): the `site.toml` keys and the flags of `check` and
  `build`.
- [Write pages](/markdown): the Markdown that renders, front matter, and `@sec` markers.
- [The specification](/spec/home): the exact rules, including where Rubrica deviates from
  CommonMark and the compatibility profile for wikis that came from elsewhere.
