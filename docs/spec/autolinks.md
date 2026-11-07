---
title: Autolinks
description: How bare URLs and email addresses become links, and why the scan is linear.
---

# Autolinks

Rubrica's parser finds GFM extended autolinks in its own pass over the event stream
(`rubrica_markdown::events`): `www.` addresses, URLs with an `http://`, `https://` or `ftp://`
scheme, and email addresses become links; a bare domain does not. The pass runs over each run
of text (consecutive text events merged, with each piece's source range kept), never inside a
link, an image or a code block, and reads every run in one forward sweep: a candidate is
checked once and the sweep never restarts behind the bytes a check consumed. Every email link
the stream carries, the pass's own and the parser's `<a@b.c>` autolinks alike, has the
`mailto:` scheme in its destination. This page states the rule, the algorithm, the invariants
with the linearity argument, Rubrica's decisions on nested candidates, the known differences
from cmark-gfm, and the tests that pin all of it.

## The rule

The GFM specification (0.29, "Autolinks (extension)"), as Rubrica reads it:

1. **Where a link may start.** After whitespace, or after one of `*`, `_`, `~` and `(`, or at
   the start of a run that begins a block, follows a line break (a soft or hard break) or
   follows an emphasis or strikethrough delimiter. A run that starts right after a code span,
   inline HTML, a footnote reference, the end of a link or image, or a superscript or
   subscript delimiter (options Rubrica does not enable) is not delimited: its first byte
   follows that construct's last byte, which is none of the delimiters.
   Whitespace is ASCII whitespace: space, tab, line feed, form feed, carriage return and
   vertical tab (the standard library's `is_ascii_whitespace` leaves the vertical tab out;
   Rubrica adds it).
2. **`www.` autolinks.** The text `www.` (lower case, as cmark-gfm reads it) followed by a
   valid domain: segments of alphanumeric characters, underscores and hyphens separated by
   periods, at least one period after the `www.`, no empty segment, and no underscore in the
   last two segments of the whole domain. Trailing periods belong to the path, not the
   domain. The scheme `http` is inserted into the destination.
3. **The path and its validation.** After a valid domain, zero or more non-space non-`<`
   characters follow. Then, from the end: `?`, `!`, `.`, `,`, `:`, `*`, `_` and `~` are not
   part of the link; a `)` is dropped while the link holds more closing than opening
   parentheses (counted over the whole link); a `;` preceded by `&` and one or more
   alphanumerics, or by `&#` and one or more alphanumerics (`&#46;`, `&#x2e;`), is an entity
   reference and is dropped with it, never split; any other trailing `;` is dropped alone (as
   cmark-gfm drops it); anything else ends the trimming. `<` ends a link at
   once.
4. **URL autolinks.** One of the schemes `http://`, `https://`, `ftp://` (any letter case)
   followed by a valid domain whose first character is alphanumeric (cmark-gfm's rule:
   `http://-` is text), then a path as in rule 3. A URL's domain may be a single segment
   (`http://a`); the underscore rule still applies; the scheme is not part of the domain.
5. **Email autolinks.** One or more of alphanumeric, `.`, `-`, `_`, `+`; an `@`; then one or
   more of alphanumeric, `-`, `_`, and `.` where an alphanumeric follows it (so a `.` never
   ends the address and never doubles), with at least one period, no empty segment, and a last
   character that is alphanumeric. The scheme `mailto:` is added to the destination. Email
   candidates take no path and no trim.

**What the rules read.** The scan reads the run's **source bytes**, not its decoded text: an
entity reference is its `&name;` spelling there and an escape keeps its backslash, so the
`&name;` exclusion of rule 3, the terminators and the delimiters see what the author wrote.
`www.a.com&nbsp;` is the link `www.a.com` followed by a non-breaking space; `www.a.com/&amp;`
is the link `www.a.com/` followed by `&`; `www.a.com/x&lt;y` is one link, since `&lt;` is not a
`<` byte; `&#42;www.a.com` is text, since the byte before `www.` is `;`, not the `*` it decodes
to. The same holds for an entity that decodes to whitespace or to `<`: `&#32;` or `&lt;` in a
path does not end the link, since the rules see `&`, not the character, and the destination
then holds that character (`www.a.com/x&#32;y` is one link whose destination is
`http://www.a.com/x y`). The link's **text and destination are decoded output**: the link in `www.a.com/x&lt;y` has
the text `www.a.com/x<y` and the destination `http://www.a.com/x<y`. The destination is
decoded, not the raw source, for the reason the specification's example 626 shows: the
destination is the address the author wrote, character for character, and an entity in
Markdown is a way of writing a character. (The HTML writer escapes it on output, as it
escapes `&` to `&amp;` in example 626.) The link's source range covers the entity's bytes it
spans.

## The algorithm

The scan works on the source bytes of one run, with a cursor `pos` and a `window`, both
starting at 0, and a flag from the caller saying whether a candidate at the run's first byte
is delimited (rule 1). It yields the links found as `(start, end, kind)` in order, in the
run's coordinates; the adapter maps them to the decoded text through the run's pieces.

**Dispatch.** While `pos` is inside the run, look at the byte there. A `w` that starts `www.`
and is delimited is a `www` candidate whose body starts four bytes on; an `h`/`H` that starts
`http://` or `https://`, or an `f`/`F` that starts `ftp://` (case-insensitively), delimited,
is a URL candidate whose body starts after the scheme; an `@` is an email candidate. Any other
byte moves `pos` on by one. A candidate is checked once; on success the link is recorded and
both `pos` and `window` move to its end; on failure `pos` moves to the end of what the check
consumed (at least one byte on), and `window` follows for an email candidate but stays for a
`www`/URL one, so a failed `www.`'s bytes may still be an email's local part (`www.afoo@b.c`
is the email link `mailto:www.afoo@b.c`, as in cmark-gfm).

**The `www` and URL check.** The forward scan takes the maximal run of domain bytes
(alphanumeric, `_`, `-`, `.`) from the body and remembers the end of its last non-period byte:
trailing periods are not part of the domain for the validity check (the path takes them and
the trim drops them). The domain must be non-empty, must not begin with `.`, must begin with
an alphanumeric when the candidate is a URL, must contain a period when it is a `www.` one,
must have no empty segment, and must have no underscore in its last two segments (for `www.`,
the segments of `www.` plus the domain; `www` holds no underscore and its period only starts a
new segment, so the pass over the body alone decides the same). An invalid domain fails the
candidate and consumes the run up to the domain's end. A valid one continues with the path:
every byte that is not whitespace and not `<`. The trim (rule 3) then shortens the link from
its end, and the candidate consumes up to the untrimmed path end. If the trim leaves nothing
after the prefix the candidate fails.

**The email check.** The local part runs backward from the `@` over local bytes
(alphanumeric, `.`, `-`, `_`, `+`) but never past `window`; an empty local part fails the
candidate, consuming one byte. The domain runs forward over alphanumerics, `-`, `_`, and a `.`
only when an alphanumeric follows it; the domain must be non-empty, contain a period, not
begin with `.`, and end in an alphanumeric. An invalid domain fails the candidate and
consumes up to the domain's end; a valid one is a link from the local part's start to the
domain's end.

**The trim.** The parentheses of the whole link are counted once, before the loop. Then,
until nothing changes: a trailing byte in `?!.,:*_~` is dropped; a trailing `)` is dropped,
and the count of closing parentheses with it, when there are more closing than opening ones;
a trailing `;` drops the entity it ends when `&` or `&#` and one or more alphanumerics precede
it, and is dropped alone otherwise; anything else ends the trim.

## Invariants and linearity

The scan keeps three invariants, stated in the module documentation of `autolink.rs`:

- **Progress**: every iteration of the dispatch loop increases `pos`.
- **No rescan behind a span**: after a candidate (success or failure) `pos` is at or past the
  end of what the check consumed, so no byte is read by a forward scan after `pos` has passed
  it; the backward local-part loop reads only bytes between `window` and the `@`, and
  `window` moves past them when the candidate ends, so it reads a byte at most once in the
  whole scan.
- **Bounded steps**: the scan of a run of `n` bytes takes at most `7·n + 16` steps.

**A step** is one execution of a loop body in: the dispatch loop (`while pos < n`); every
forward scan (the domain scan, the email domain scan, the path scan); the one validity pass
over a scanned domain (segments, underscores); the backward local-part loop; and in the trim
the paren pre-count pass, the removal loop and the entity walk-back loop. The
case-insensitive prefix compares are **not** steps (each is at most 8 bytes and happens once
per dispatch), nor is the email domain scan's one-byte look-ahead after a `.`. Nothing else
counts. `scan_counting` returns the count with the links; it is the function the linearity
tests call.

**The bound.** The trim is first made linear: the `(` and `)` are counted once before the
loop (the pre-count pass) and the counts are updated as bytes are dropped; a re-count per
dropped `)` would be quadratic on a link that ends in many `)`. Then every counted iteration
is charged to one byte of the run, the byte it reads or drops, and the charges per byte are
bounded:

- The dispatch loop runs once per distinct `pos`, and `pos` strictly increases: at most 1
  per byte.
- Candidate spans, from a candidate's start to what its check consumed, are pairwise disjoint:
  after a failure `pos` is at or past the consumed end; after a success `pos` is the trimmed
  end, and no candidate can start in the trimmed tail, which holds only `?!.,:*_~);` and
  `&name;` bytes, so a `w`, `h` or `f` there is preceded by `&` or an alphanumeric, never a
  delimiter, and it holds no `@`. So the forward scan reads a byte at most once, and the
  validity pass at most once.
- The backward local-part loop reads a byte at most once (the second invariant). A byte of a
  *successful* `www`/URL span is never read by it: `window` moved to the span's end.
- The trim charges a *successful* `www`/URL span's bytes at most 4 times: the pre-count; one
  removal-loop iteration per dropped byte; one entity walk-back read (a successful walk drops
  the bytes it read, and after a failed walk the loop ends at the alphanumeric run it read,
  so no byte is walked twice); and the loop's final iteration, the one that ends it, charged
  to the byte it looked at.

A byte of a successful `www`/URL span is therefore in at most 1 (dispatch) + 1 (forward scan)
+ 1 (validity) + 0 (backward loop) + 4 (trim) = 7 counted loop bodies; a byte of a failed
`www`/URL span in at most 4 (dispatch, forward scan, validity, backward loop); an email span's
bytes in at most 3 (dispatch, the backward or forward scan, validity); a byte outside every
span in at most 2 (dispatch, backward loop). Since every iteration is charged to a byte and
no byte is charged more than 7, the total is at most `7·n`. The `+ 16` is slack: it is not
needed by this argument today; it keeps the bound stable, and the fuzz target's constant
unchanged, if a loop later gains an iteration that reads no byte (a terminating iteration on
an empty run, say). The claim is a bound, not a tight count; the adversarial runs in the tests
take between `n` and about `4·n` steps.

## Nested candidates

Where one candidate's bytes could hold another, the algorithm decides; these rows are
Rubrica's rule and are pinned by `nested_candidates_follow_rubricas_rule`.

| input | result | why |
|---|---|---|
| `www.foo/http://bar.com` | all text | `www.` + `foo` has no period, so it fails and consumes through `foo`; `h` is then preceded by `/`, not a delimiter |
| `http://@b.com` | all text | an empty domain fails and consumes `http://`; the `@` has an empty local part, since `/` is not a local byte |
| `mailto:www.a.com` | all text | `m` is no candidate; `w` is preceded by `:` |
| `foo@bar.baz@qux.com` | one link `mailto:foo@bar.baz`, then the text `@qux.com` | the first `@` succeeds through `bar.baz`; the second `@`'s local part is empty at `window` |
| `(www.a.com)` | `(` text, link, `)` text | the paren rule |
| `foowww.a.com` | all text | not delimited |
| `www.a_b.c` | all text | an underscore in the last two segments |
| `www.a_b.c.d` | link | the underscore is outside the last two |
| `http://a` | link `http://a` | a URL's domain may be short |
| `www.a` | text | a `www.` domain needs a period |
| `a@b.c1` | link | the spec allows a final digit (a documented deviation from cmark-gfm) |
| `www.a.com'` and `www.a.com"` | links whose text ends in `'` / `"` | the spec's trailing set has no quotes (a documented deviation from cmark-gfm) |
| `[www.a.com]` | all text | `[` is not in the spec's delimiter list (`*`, `_`, `~`, `(`); cmark-gfm accepts it (a documented deviation) |
| `1http://x` | all text | `h` is preceded by `1`: not delimited; cmark-gfm needs no delimiter before a scheme (a documented deviation) |
| `www.a..` | all text | an empty domain segment, anywhere, fails the domain |
| `www.a.com;` | link `www.a.com`, then `;` | a trailing `;` that ends no entity is dropped alone |
| `www.afoo@b.c` | link `mailto:www.afoo@b.c` | the `www.` candidate fails at `@` and leaves `window`; the `@`'s local part reaches back over its bytes |
| `WWW.a.com` | all text | `www.` is lower case |
| `http://-` | all text | a URL's first host byte is alphanumeric |
| `-@o.-h` | all text | the `.` is followed by `-`, so the domain is `o`: no period |
| `+@c.f@` | link `mailto:+@c.f`, then `@` | the domain ends at the second `@`; that `@` has an empty local part |
| `a@b@c.com` | all text | the first `@`'s domain `b` has no period, consuming through `b`; the second `@`'s local part is empty at `window` |
| `www.a.` | all text | the trailing period is not part of the domain, and `a` has no period |
| `www.a_b.c.` | all text | without the trailing period the last two segments are `a_b` and `c` |
| `x@.p` | all text | an empty first segment |
| `_www.a.com_` | a link inside emphasis | the parser makes `_…_` emphasis; the run `www.a.com` begins after an emphasis delimiter |
| `[ www.a.com]` | `[ ` text, link `www.a.com]` | `w` is preceded by a space; `]` is a path byte and not in the trailing set, so the link keeps it (as in cmark-gfm) |
| `[x www.a.com` | `[x ` text, link | `w` is preceded by a space |
| `x [a@b.c]` | `x [` text, link `mailto:a@b.c`, `]` text | brackets do not suppress an email candidate here (cmark-gfm's do: a documented deviation) |
| `mailto:a@b.c` | `mailto:` text, link `mailto:a@b.c` with the text `a@b.c` | `:` is not a local byte (cmark-gfm takes the `mailto:` into the link text: a documented deviation) |
| `a&#64;b.c` | all text | the scan sees `&#64;`, not the `@` it decodes to (a documented deviation) |
| `http://ünïcode.example` | all text | domain bytes are ASCII (a documented deviation) |
| `a\@b.c` | all text | the escape's backslash is in the source view and is not a local-part byte (a documented deviation) |
| `www\.a.com` | all text | `www\.` is not `www.` |

The runs that are not delimited at their start are pinned by
`a_run_start_is_delimited_only_after_a_block_start_a_break_or_emphasis`: `<b>www.a.com</b>`,
`` `x`www.a.com ``, `[a](b)www.a.com`, `![i](u)www.a.com`, `foo<br>www.a.com` and
`a[^1]www.a.com` (with a definition) make no link; `*x*www.a.com`, `~~x~~www.a.com`,
`**x**www.a.com` and `www.a.com` after a hard break make one. The source-bytes rule is pinned
by `the_scan_sees_source_bytes_not_decoded_text`.

## Deviations from cmark-gfm

Rubrica follows the specification's text. cmark-gfm, the reference implementation, differs
in the places below. The reference is cmark-gfm's `autolink.c` (its `www_match`, `url_match`,
`email_match`, `check_domain` and `autolink_delim`); each row was settled on 2026-11-01 by
reading it, not by running cmark-gfm; a run of cmark-gfm on the inputs would confirm them, and
a row a run refutes is corrected together with its test.

**Kept, and documented.** Rubrica keeps the specification's reading where cmark-gfm departs
from the text, or where matching cmark-gfm would overturn a decided rule:

| input | Rubrica | cmark-gfm | why it differs |
|---|---|---|---|
| `a@b.c1` | link | text | the spec says the last character must not be `-` or `_`; cmark-gfm requires a letter |
| `www.a.com'`, `www.a.com"` | links ending in the quote | links without the quote | the spec's trailing-punctuation set has no quotes; cmark-gfm strips them |
| `[www.a.com]` | text | link | `[` is not among the spec's delimiters `*`, `_`, `~`, `(`; cmark-gfm's `www_match` accepts it |
| `1http://x` | text | link | the spec requires a delimiter; cmark-gfm's `url_match` requires only that no letter precede the scheme |
| `www.foo/http://bar.com` | text | `www.foo/` text, then the link `http://bar.com` | the same: cmark-gfm needs no delimiter before `http://`, and the failed `www.` does not stop it |
| `x [a@b.c]` | link `mailto:a@b.c` | text | cmark-gfm skips an `@` inside `[`…`]` of the same text node (its bracket context); Rubrica has no bracket rule yet |
| `+@c.f@` | link `mailto:+@c.f`, then `@` | text | in cmark-gfm an `@` met while scanning the domain fails the whole candidate |
| `a@b@c.com` | text | `a` text, then the link `mailto:b@c.com` | the same rule, and cmark-gfm's next `@` then reaches back over `b` |
| `www.a` | text | link | Rubrica follows the spec text: the domain after `www.` needs a period; cmark-gfm's `check_domain` runs over the whole `www.a` and the period of `www.` satisfies it |
| `www.a.` | text | link `www.a` | the same: the period of `www.` makes it a link there, and the trailing period is trimmed |
| `www.a_b.c.` | text | link `www.a_b.c` | the same: with the trailing period the last two segments there are `c` and the empty one, so the underscore is out of them |
| `x@.p` | text | link `mailto:x@.p` | cmark-gfm accepts an empty first segment in an email domain (`.` followed by an alphanumeric); Rubrica rejects empty segments everywhere |
| `mailto:a@b.c` | `mailto:` text, link with the text `a@b.c` | one link with the text `mailto:a@b.c` | cmark-gfm's local-part scan recognises a `mailto:` or `xmpp:` scheme and takes it in |
| `a\@b.c` | text | link `mailto:a@b.c` | cmark-gfm's email scan runs on the decoded text, where the escape is gone; Rubrica's source view keeps the backslash, which is not a local-part byte |
| `a&#64;b.c` | text | link `mailto:a@b.c` | cmark-gfm's email scan runs on the decoded text node (its `www`/URL scan on the source); Rubrica's scan reads the source for all three |
| `http://ünïcode.example` | text | link | cmark-gfm's host characters are any that are not whitespace, punctuation or symbols, Unicode included; Rubrica's are ASCII alphanumerics, `-`, `_` and `.` |

**Matched to cmark-gfm.** Where cmark-gfm's behaviour was matchable without breaking
linearity or a decided rule, Rubrica matches it; these are the departures from the package's
first reading, each pinned in `nested_candidates_follow_rubricas_rule`:

| input | both | what was matched |
|---|---|---|
| `WWW.a.com` | text | `www.` is matched in lower case only |
| `http://-` | text | a URL's first host byte must be alphanumeric |
| `-@o.-h` | text | an email domain's `.` counts only when an alphanumeric follows it |
| `www.a.com;` | link `www.a.com`, then `;` | a trailing `;` that ends no entity is dropped alone |
| `www.afoo@b.c` | link `mailto:www.afoo@b.c` | a failed `www.` candidate's bytes may be an email's local part |

**Same in both** (listed because the reviews raised them): `www\.a.com` is text (neither sees
`www.` in it); `_www.a.com_` is a link inside
emphasis; `[ www.a.com]` (the link `www.a.com]`, `]` being a path byte) and `[x www.a.com` are
links, since a space precedes `www.`.

## What is pinned

In `crates/rubrica-markdown/tests/autolink.rs`:

- `the_eleven_gfm_examples_produce_exactly_these_links`: the specification's examples
  621–631, each link's destination and text, and that every byte is emitted exactly once.
- `the_parsers_own_email_autolinks_carry_mailto`: `<a@b.c>` comes out with `mailto:a@b.c`.
- `nested_candidates_follow_rubricas_rule`: the thirty-four rows above (thirty-five inputs:
  the quote row is two).
- `a_run_start_is_delimited_only_after_a_block_start_a_break_or_emphasis`: rule 1 at a run's
  start.
- `the_scan_sees_source_bytes_not_decoded_text`: the seven entity cases (four named, two
  numeric references at a link's end, one that decodes to a space inside a path), with
  ranges.
- `text_inside_links_images_and_code_is_not_scanned`: links, images, code spans, fenced and
  indented code blocks and HTML blocks are never scanned.
- `delimiters_before_a_candidate`: each delimiter in rule 1, five non-delimiters, the run
  start and a soft break.
- `ranges_are_exact_in_verbatim_pieces_and_stay_on_one_line`: two escape cases with exact
  source ranges, and a property test over 1,000 documents. The property (a link's source bytes
  equal its text wherever no `&` is in them) is shown for the test's alphabet only (`a . w W :
  / @ ( ) < & ; + - _ h t p f`, space, tab, line feed, `*`), where only an entity makes a piece
  non-verbatim.
- `every_byte_is_emitted_once_and_ranges_are_monotone`: a property test over 2,000 documents
  against the vendored parser's own text events; ranges well-formed and monotone within a run;
  no empty link text; no whitespace or `<` in a destination. Shown for the same alphabet only.
- `scan_is_linear_on_adversarial_runs`: eight adversarial runs at 0.5, 1 and 2 MB (re-run at
  4 and 8 MB when 2 MB is under the floor); the best of three timings grows by at most 2.5×
  per doubling (above a 50 ms floor, 100 ms for the re-runs), and `steps ≤ 7·n + 16` in every
  build.

In `crates/rubrica-markdown/tests/linear.rs`, `every_shape_is_linear_with_autolinks_on`: the
whole parser with the scanner on 37 pathological shapes at 0.5, 1 and 2 MB (re-run at 4 and
8 MB when 2 MB is under the floor), the same ratio rule and a 2 s ceiling at 2 MB.

In the private suite, `tests/linear_private.rs`,
`the_two_lt_shapes_are_linear_at_parser_level`: the two shapes that hold `<`, at parser level
and on the scan alone, under the same rules and the same step bound. Their strings do not
appear in this repository.
