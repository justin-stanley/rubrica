# Security policy

Rubrica renders text written by other people and builds sites from repositories it did not
write, so security reports get priority over everything else.

## Reporting a vulnerability

**Please don't open a public issue.** Report it privately through GitHub's
[private vulnerability reporting](https://github.com/justin-stanley/rubrica/security/advisories/new)
on `justin-stanley/rubrica`. Include what you found, how to reproduce it (a repository or a
Markdown file that shows it is ideal), and what an attacker gains.

What to expect:

| | |
|---|---|
| Acknowledgement | within 72 hours |
| Fix | as fast as the severity warrants; the fixed versions are named in [CHANGELOG.md](CHANGELOG.md) |
| Credit | in the advisory and the changelog, unless you would rather not be named |

Rubrica has one maintainer, so these are targets, not guarantees. There is no bug bounty.

## Supported versions

The latest minor release gets security fixes. Until the first release, fixes land on `main`.

## What counts

- Any denial of service over the documented bounds: input that makes `check` or `build` use
  time or memory beyond the limits in the README, or superlinear in the input size.
- Any script execution from page content: a page, a front-matter field, a file name or an
  asset that makes a built page run JavaScript.
- A sanitizer bypass: raw HTML, a URL scheme or an attribute that survives the sanitizer and
  should not.
- A path escape in `build`: any way for repository content to make the build read or write a
  file outside the repository or the output directory.

## What does not

- A hostile commit merged to `main` before a release review is an accepted defacement risk at
  0.1: the public site is built from a reviewed release tag, so the exposure is a defaced
  documentation site. The reasoning is in `design/THREAT-MODEL.md`.
- Vulnerabilities in dependencies, unless Rubrica uses them unsafely. Report those upstream;
  `cargo audit` runs in CI.
