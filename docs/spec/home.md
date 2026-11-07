---
title: Specification
description: The rules Rubrica renders by, as pages.
---

# Specification

These pages state the rules Rubrica's renderer follows.

- [The compatibility profile](/spec/compat-profile): the rules that differ between Rubrica's
  native Markdown and the profile for wikis that came from Wiki.js.
- [Deviations](/spec/deviations): every known place where Rubrica's output differs from
  CommonMark, from GFM, from the reference implementations, or from the renderer it replaced,
  with the reason.
- [Autolinks](/spec/autolinks): how bare URLs and addresses become links.

The specification is tested: the renderer's output for the examples on these pages is
compared with the expected HTML on every change.
