# Threat model

This document lists what Rubrica protects, who may attack it, where trust changes, and which
gate covers each pair, by release. The gates are tested in CI before a release candidate.

## Assets

- **Content and history**: the wiki's pages and git history.
- **Sessions**: editors' signed-in sessions (0.3 on).
- **The host**: the machine that builds or serves.
- **The release pipeline**: the repository, its workflows, signing, and published artifacts.

## Actors

1. Anonymous reader.
2. Signed-in editor (0.3 on).
3. Repository pusher: someone who can push to the repository serve mirrors.
4. Hostile page author: writes Markdown, front matter and raw HTML.
5. Hostile repository: a whole repository crafted to exhaust the builder or server.
6. Network attacker.
7. The forge that hosts the source repository.
8. **A fork-PR author**, whose code runs in the pull-request fuzz and test jobs.
9. **The GitHub account itself**, whose compromise is a supply-chain compromise.

## Trust boundaries

- Page source to HTML: untrusted Markdown crosses into a document served to browsers. The
  parser, writer and sanitizer sit on this line.
- Repository to builder or server: a repository is input, not instructions.
- Network to `serve` (0.2 on): requests, the webhook, and the dumb-HTTP git endpoint.
- Identity provider to Rubrica (0.3 on): sign-in and the permissions record.
- Contributor to CI: fork code runs with a read-only token and no secrets.
- CI to the world: the release and publish environments, which require the owner's review.

## Gate by pair

| actor, asset | gate | from |
|---|---|---|
| hostile page author, content | bounds on page size, front matter, attributes and footnote back-references, each with a finding; linear-time tests; sanitizer allow-list tests (no `javascript:` or `data:` survives, no id or class from raw HTML); the no-JavaScript check | 0.1.0 |
| hostile page author, readers | a cap on the renderer's output size, tested with a crafted page at the page cap | 0.1.0 |
| hostile page author, readers (WebAssembly search) | the no-JavaScript check, version 2: the only script on any page is the generated loader whose hash equals the bundle's, every `.wasm` is the bundle's bytes; a per-page Content-Security-Policy (`'wasm-unsafe-eval'` and `connect-src 'self'` only on pages that load the search); the bundle is reproducible (published bytes equal a fresh build) and within its size budget | 0.1.0 |
| hostile page author, readers (diagrams) | an SVG allow-list in the sanitizer; `click`, `%%{init}%%` and HTML labels rejected | 0.1.x |
| hostile repository, the host | the repository budgets (files, path bytes, bytes, output); output-path safety (build writes only under the output directory and refuses reserved names); fuzz targets for the parser and front matter | 0.1.0 |
| hostile repository, the host (serve) | delta-work charge before reading an unbuilt repository; render-concurrency limit; an end-to-end hostile-repository test | 0.2.0 |
| anonymous reader, the host | request body and header caps; per-IP rate limits on search, webhook and raw `.md`; non-root, read-only container | 0.2.0 |
| anonymous reader, content (dumb-HTTP endpoint) | serves only `HEAD`, `info/refs`, `objects/info/packs` and `objects/**`, path-confined (no `..`, no symlink escape, no other file of the data directory), with a traversal test | 0.2.0 |
| anonymous reader, content of a read-private wiki (endpoint, search, feeds, raw, history, timing) | the read-private gate: nothing leaks, tested with anonymous requests including `info/refs` and `objects/**` | 0.4.0 |
| network attacker, the host | trusted-proxy rules; the TLS dependency check; secrets from files or environment, never flags | 0.2.0 |
| network attacker, webhook | HMAC with constant-time compare, replay window, body cap | 0.2.0 |
| signed-in editor, sessions | CSRF (same-site cookies plus per-form token), session fixation, sign-in redirect allow-list, sign-in rate limits, an SSRF guard on every outbound fetch | 0.3.0 |
| signed-in editor, content | write path traversal (paths normalised, must stay under the wiki root); upload limits and metadata stripping; author is the signed-in DID; authorisation re-checked on every write; the preview renders with the page's bounds | 0.3.0 |
| editor without rights, content | roster record verification (signed, fresh, from the configured DID), fail-closed on fetch error | 0.4.0 |
| hostile page author or importer, the host (import) | path traversal and symlink rejection in archives, decompression-bomb caps, per-file and total caps, a dry run | 0.7.0 |
| the forge, content | serve fetches over HTTPS only, keeps the upstream under `refs/remotes/<name>/*`, and never mirrors with `+refs/*:refs/*`; at 0.3 the forge is a read-only mirror of serve | 0.2.0, 0.3.0 |
| repository pusher, content | a pusher can change content but not the builder: the budgets above apply to what they push | 0.1.0 |
| fork-PR author, the release pipeline | pull-request jobs run on `pull_request` with a read-only token and no secrets | 0.1.0 |
| the GitHub account, the release pipeline | hardware-key two-factor authentication and no long-lived tokens; the release and publish environments require the owner's review; every artifact is attested, so a consumer can check it against the repository identity | 0.1.0 |
| any actor, dependencies and secrets | gitleaks, CodeQL, Scorecard, `cargo deny`, a daily `cargo audit`, mutation testing on the sanitizer, the authoriser and the HMAC check | every release |

## Accepted risks

- **A hostile commit merged to `main` (0.1).** The project's documentation site is built only
  from a signed release tag, with a binary whose attestation has been verified. The exposure
  is the window between a bad merge and the owner's release review, and the damage is a
  defaced documentation site. This is accepted at 0.1. At 0.2, when the host renders
  untrusted input by design, serve's gates (above) are the defence.
- **The Developer Certificate of Origin is provenance, not a security control.** A
  `Signed-off-by` line states who contributed a change and under what terms. Nothing in a
  deployment trusts it.
