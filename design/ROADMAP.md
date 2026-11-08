# Roadmap

Each release has what it **delivers** and its **exit**: what must hold before it is released.
Nothing in `x.y.0` depends on a later release. Every release candidate is an attested build,
soaked for 24 hours on a staging host that is separate from the host serving the project's
own documentation.

## 0.1.0: static

Delivers:
- `rubrica check` and `rubrica build` on Rubrica's own parser.
- A theme with no script of its own. Dark mode is CSS (`prefers-color-scheme`); the `/` search
  shortcut is dropped.
- Syntax highlighting at build time.
- Mermaid fences written as escaped source in `<pre><code class="language-mermaid">`.
- Search: `rubrica-wasm` searches `_search.json` on the search page, through the one permitted
  generated loader. The WebAssembly ships in a published bundle crate, and a
  Content-Security-Policy that allows `'wasm-unsafe-eval'` and `connect-src 'self'` applies only
  to the page that loads it, within a size budget.
- Static binaries for Linux x86_64 and aarch64, macOS on Apple Silicon on a best-effort basis,
  and a multi-architecture container image, as a GitHub release with attestations.
- The ordered multi-crate publish (`rubrica-markdown`, `rubrica-render`, `rubrica-wasm-bundle`,
  `rubrica-site`, `rubrica`), so `cargo install rubrica` works.

Exit:
- Gates: build, MSRV, the specification suite, linear-time tests, render-diff, `cargo deny`,
  `cargo audit`, gitleaks, CodeQL, Scorecard.
- The no-JavaScript check: zero `on*=` attributes and zero `javascript:` URLs in a built site;
  the only script is the loader whose hash equals the bundle's, on the search page only; every
  `.wasm` is the bundle's bytes; no other `.js` or `.mjs` file; the bundle is reproducible.
- A cap on the renderer's output size, tested with a crafted page at the page cap.
- The attestation-verified release-candidate artifact has soaked for 24 hours.

## 0.1.x: diagrams

Delivers: `rubrica-diagram` renders Mermaid-style diagrams to SVG at build time. Not a blocker
for 0.2.

Exit: an SVG allow-list in the sanitizer (elements, attributes and attribute grammars limited
to what the renderer emits); `click`, `%%{init}%%` and HTML labels rejected with a finding; a
diagram case in the linear-time tests.

## 0.2.0: server

Delivers: `rubrica serve` from a bare clone. A snapshot per commit; a webhook with an HMAC and
a polling fallback; raw `.md`; server-side search over the same index; health; secrets from
files; trusted-proxy rules; an upgrade test.

At 0.2 serve is a **read-only mirror of the forge**: it fetches an upstream over HTTPS and
publishes a dumb-HTTP read-only git endpoint. The upstream lives under `refs/remotes/<name>/*`
with a configured remote, and `refs/heads/*` is fast-forwarded from it; never a
`+refs/*:refs/*` mirror layout.

Exit: delta-chain work is charged against a budget before serve reads any repository it did
not build; a render-concurrency limit; limits and rate limits on every endpoint; an end-to-end
test that a hostile repository is refused within its budgets; the git endpoint is
path-confined, with a traversal test; the project's own documentation site is served by
`rubrica serve` as a read-only site; the TLS dependency check passes.

## 0.3.0: editing

Delivers: sign-in with atproto identities; CSRF protection; an editor as a plain form with a
server-rendered preview (0.3.0), then a live preview in WebAssembly (0.3.1); saving as a commit
through gix; uploads with limits and metadata stripping; rename, move and delete with
redirects (0.3.2); history and revert (0.3.3).

At 0.3 serve **writes and becomes the source of truth**, and the forge mirrors serve's
repository (a forge pull-mirror, or a mirror job that pushes with a `git` binary outside the
image).

Exit: the flip. A new pull-mirror repository against serve's git endpoint replaces the forge's
push-based repository, which a forge cannot convert in place, and push-based flows into the
wiki repository stop.

## The owned push milestone

Delivers: an owned push over `gix-transport` and `gix-pack` (negotiation, pack writing, the
push protocol, credentials over HTTPS), so operators can keep the forge primary. It comes after
0.3.0 and has its own tests, fuzzing and threat-model rows. It uses the same bare-repository
layout as 0.2, so it needs no migration.

## 0.4.0: permissions

Delivers: a roster record (editors, groups, per-path rules) in the owner's atproto repository,
and read-private wikis.

Exit: the roster record is verified (signed, fresh, from the configured DID) and fails closed
on a fetch error. A read-private wiki leaks nothing through search, feeds, raw `.md`, history,
404 timing or the git endpoint, which is served only to authorised readers or not at all,
tested with anonymous requests.

## 0.5.0: search and structure

Delivers: a real full-text index in serve (pure Rust); tags, backlinks and templates.

## 0.6.0: collaboration

Delivers: comments, notifications, review before publish.

## 0.7.0: import and export

Delivers: `rubrica import wikijs`, with the compatibility profile public; MediaWiki import;
plain folders; static export.

Exit: archive import rejects path traversal and symlinks and resists decompression bombs (ratio
and absolute caps); per-file and total caps; a dry run lists what would be written.

## 1.0.0

Delivers: the storage layout, configuration, URL scheme and HTTP API are frozen and an upgrade
path is promised. Exit: the threat model (`THREAT-MODEL.md`) is reviewed against every
milestone's gates.
