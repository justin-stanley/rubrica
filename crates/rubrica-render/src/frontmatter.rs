//! Wiki.js front matter: the YAML block between a first-line `---` and the
//! next `---` (or `...`) line.
//!
//! Wiki.js 2.5.314's git import (`server/modules/storage/disk/common.js`
//! `processPage`, via `server/models/pages.js` `parseMetadata`) reads it with
//! the regex `^(-{3}(?:\n|\r)([\w\W]+?)(?:\n|\r)-{3})?…` and js-yaml 3.14.0's
//! `safeLoad`, then takes `title`, `description`, `isPublished` and `tags`
//! (`tags.split(', ')`). Where that reading differs from Rubrica's, the
//! `wikijs_limits` check reports it ([`FrontMatterFinding`]).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::{Scanner, TScalarStyle, Token, TokenType};
use yaml_rust2::{ScanError, Yaml, YamlLoader};

/// A page's front matter, as Wiki.js 2's git storage reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct FrontMatter {
    /// `title:`, required.
    pub title: String,
    /// `description:`.
    pub description: Option<String>,
    /// `published:`, default true. `false` keeps the page off the site: a
    /// Rubrica behaviour. Wiki.js's git import reads `isPublished`, never
    /// `published`, so Wiki.js publishes such a page anyway
    /// ([`FrontMatterFinding::PublishedIgnoredByWikijs`]).
    pub published: bool,
    /// `date:` (last update, as Wiki.js writes it).
    pub date: Option<DateTime<Utc>>,
    /// `dateCreated:`.
    pub date_created: Option<DateTime<Utc>>,
    /// `tags:`, a comma-separated string (all of the corpus, as `a, b, c`)
    /// or a YAML list, trimmed, empty pieces dropped. Wiki.js's import splits
    /// on `", "` exactly (then trims and lowercases), and fails on a list
    /// ([`FrontMatterFinding::TagListBreaksWikijsSync`],
    /// [`FrontMatterFinding::TagsSplitDifferentlyByWikijs`]).
    pub tags: Vec<String>,
    /// `editor:`.
    pub editor: Option<String>,
    /// `redirect_to:`: a redirect stub left by a move.
    pub redirect_to: Option<String>,
    /// Every other key, kept.
    pub extra: BTreeMap<String, Yaml>,
    /// The 1-based source line of each top-level key, from the YAML parser.
    pub key_lines: BTreeMap<String, usize>,
    /// How Wiki.js's import would read this differently, found while
    /// splitting and parsing; reported by [`FrontMatter::findings`] under
    /// `wikijs_limits`.
    wikijs: Vec<FrontMatterFinding>,
}

/// Why a page's front matter could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatterError {
    /// 1-based source line; 0 when the file has no front matter at all.
    pub line: usize,
    /// What is wrong.
    pub kind: FrontMatterErrorKind,
}

/// The kinds of [`FrontMatterError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontMatterErrorKind {
    /// The first line is not `---`.
    Missing,
    /// A bare `\r` (one not before a `\n`) in the front matter, its fences
    /// included; at line 1 when the file's lines all end in one (classic Mac
    /// OS: `---` and a `\r`, and no `\n` follows). Rubrica's front matter
    /// reads `\n` and `\r\n` line endings only; Wiki.js's regex and YAML read
    /// `\r` too, and so does yaml-rust2, so its lines would not be Rubrica's.
    /// In the body a bare `\r` is a line end, as it is to the parser and
    /// markdown-it.
    BareCarriageReturns,
    /// A `---` with no closing `---`.
    Unterminated,
    /// Not YAML, or not a mapping.
    Yaml(String),
    /// No `title:`, or an empty one.
    NoTitle,
    /// A key with a value of the wrong type (`published: yes please`).
    BadValue {
        /// The key.
        key: String,
        /// What was expected.
        expected: &'static str,
    },
    /// The YAML between the fences is over [`MAX_FRONT_MATTER_BYTES`].
    TooLarge {
        /// Its size in bytes.
        bytes: usize,
    },
    /// A YAML anchor (`&a`). Refused, as are aliases: expanding aliases is
    /// unbounded (a few hundred bytes of "billion laughs" take seconds and
    /// hundreds of MB), and the corpus uses neither.
    Anchor,
    /// A YAML alias (`*a`); see [`FrontMatterErrorKind::Anchor`].
    Alias,
    /// An explicit YAML tag (`!foo`, `!!str`, `!<…>`). js-yaml 3.14.0's
    /// `safeLoad` throws on many (an unknown `!foo`, `!!js/regexp`, a value
    /// its `!!float`, `!!int` or `!!null` does not match), and Wiki.js then
    /// imports the page with no front matter (titled by its file name, the
    /// YAML as text). All are refused, one rule with no tag table; the corpus
    /// uses none.
    Tag,
    /// Collections nested deeper than [`MAX_FRONT_MATTER_DEPTH`].
    TooDeep,
}

/// The largest YAML block Rubrica reads, in bytes (the corpus's largest is
/// well under 1 KiB).
pub const MAX_FRONT_MATTER_BYTES: usize = 64 * 1024;

/// The deepest nesting of YAML collections Rubrica reads (the corpus nests
/// none).
pub const MAX_FRONT_MATTER_DEPTH: usize = 32;

/// Something worth reporting that does not stop the page from rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontMatterFinding {
    /// Wiki.js stores `title` and `description` in `varchar(255)` columns and
    /// silently drops a page whose value is longer. Counted in code points,
    /// as Postgres `char_length()` does.
    TooLongForWikijs {
        /// `title` or `description`.
        field: &'static str,
        /// Its length in code points.
        chars: usize,
        /// 1-based source line of the key.
        line: usize,
    },
    /// `published: false`. Rubrica does not publish the page; Wiki.js's git
    /// import reads `isPublished`, never `published`, so Wiki.js publishes it.
    /// Not reported when `isPublished` says otherwise: Wiki.js 2.5.314's
    /// `updatePage` stores `isPublished === true || isPublished === 1` (on
    /// js-yaml 3.14.0's reading), and `createPage`'s schema refuses all but a
    /// boolean, so only an `isPublished` js-yaml reads as `true` or 1, or none
    /// at all, can publish the page.
    PublishedIgnoredByWikijs {
        /// 1-based source line of the key.
        line: usize,
    },
    /// `isPublished:` with a value js-yaml 3.14.0 does not read as a boolean
    /// (`1`, null, a string such as `yes` or `"true"`, a collection). For a
    /// page Wiki.js does not have yet, its git import passes
    /// `_.get(pageData, 'isPublished', true)` to `createPage`, which inserts
    /// it as it is into a model whose schema says `{type: 'boolean'}`: the
    /// insert fails and the page is never imported. (An existing page's
    /// `updatePage` coerces it instead.)
    IsPublishedBreaksWikijsImport {
        /// 1-based source line of the key.
        line: usize,
    },
    /// `tags:` as a YAML list. Wiki.js's git import calls `.split(', ')` on
    /// it, which throws: the page never syncs to Wiki.js.
    TagListBreaksWikijsSync {
        /// 1-based source line of the key.
        line: usize,
    },
    /// A `tags:` string Wiki.js reads as other tags than Rubrica's, case
    /// aside: it splits on `", "` exactly, Rubrica on `,` (`a,b` is one tag
    /// to Wiki.js, two here), and it lowercases and drops duplicates, so
    /// `A, a` is one tag to Wiki.js, two here. Lowercasing alone (`Ops`) is
    /// not reported.
    TagsSplitDifferentlyByWikijs {
        /// 1-based source line of the key.
        line: usize,
        /// The tags as Wiki.js stores them: trimmed, lowercased, deduplicated.
        wikijs: Vec<String>,
    },
    /// Wiki.js's import sees no front matter at all (its regex needs the
    /// first line to be exactly `---`, and a later line starting `---`):
    /// it imports the YAML as page text, with the file name as the title.
    NoFrontMatterForWikijs {
        /// 1-based source line of the offending fence.
        line: usize,
        /// Why.
        reason: &'static str,
    },
    /// Wiki.js's import ends the front matter at a different line from
    /// Rubrica (it closes at the first line *starting* with `---`, so a
    /// `----` or `---x` line closes it, and `...` does not). A `----` line
    /// inside block YAML is usually a yaml-rust2 parse error first (a stray
    /// scalar after the mapping), so this is reported for `...` closers and
    /// for such a line inside flow YAML (`{title: T,⏎----: 2}`).
    WikijsClosesFrontMatterElsewhere {
        /// 1-based source line of Rubrica's closing fence.
        line: usize,
        /// 1-based source line Wiki.js closes at.
        wikijs_line: usize,
    },
}

/// How bad a [`FrontMatterFinding`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Wiki.js would lose or mangle the page.
    Error,
    /// Wiki.js would show the page differently from Rubrica.
    Warning,
}

impl FrontMatterFinding {
    /// How bad it is.
    pub fn severity(&self) -> Severity {
        match self {
            Self::TooLongForWikijs { .. }
            | Self::TagListBreaksWikijsSync { .. }
            | Self::IsPublishedBreaksWikijsImport { .. }
            | Self::NoFrontMatterForWikijs { .. }
            | Self::WikijsClosesFrontMatterElsewhere { .. } => Severity::Error,
            Self::PublishedIgnoredByWikijs { .. } | Self::TagsSplitDifferentlyByWikijs { .. } => {
                Severity::Warning
            }
        }
    }

    /// The 1-based source line it is about.
    pub fn line(&self) -> usize {
        match self {
            Self::TooLongForWikijs { line, .. }
            | Self::PublishedIgnoredByWikijs { line }
            | Self::TagListBreaksWikijsSync { line }
            | Self::IsPublishedBreaksWikijsImport { line }
            | Self::TagsSplitDifferentlyByWikijs { line, .. }
            | Self::NoFrontMatterForWikijs { line, .. }
            | Self::WikijsClosesFrontMatterElsewhere { line, .. } => *line,
        }
    }
}

/// Which optional front-matter checks to run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrontMatterChecks {
    /// The `varchar(255)` rule; only while Wiki.js still syncs from git.
    pub wikijs_limits: bool,
}

/// Wiki.js's `varchar(255)` columns, in code points.
const WIKIJS_MAX_CHARS: usize = 255;

impl FrontMatter {
    /// The findings `checks` asks for, in source-line order.
    pub fn findings(&self, checks: FrontMatterChecks) -> Vec<FrontMatterFinding> {
        let mut out = Vec::new();
        if checks.wikijs_limits {
            out.extend(self.wikijs.iter().cloned());
            let fields = [
                ("title", Some(self.title.as_str())),
                ("description", self.description.as_deref()),
            ];
            for (field, value) in fields {
                let chars = value.map_or(0, |v| v.chars().count());
                if chars > WIKIJS_MAX_CHARS {
                    let line = self.key_lines.get(field).copied().unwrap_or(1);
                    out.push(FrontMatterFinding::TooLongForWikijs { field, chars, line });
                }
            }
        }
        out.sort_by_key(FrontMatterFinding::line);
        out
    }
}

/// `---` (or `...` to close), optionally followed by spaces or tabs.
fn is_fence(line: &str, close: bool) -> bool {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let rest = line
        .strip_prefix("---")
        .or_else(|| line.strip_prefix("...").filter(|_| close));
    rest.is_some_and(|r| r.bytes().all(|b| b == b' ' || b == b'\t'))
}

fn err(line: usize, kind: FrontMatterErrorKind) -> FrontMatterError {
    FrontMatterError { line, kind }
}

/// Split `src` into its front matter, the body after it, and the number of
/// source lines before the body (body line `n` is source line `n + offset`).
///
/// The block is a first line `---`, YAML, and a line `---` or `...`, as
/// `check-anchors.cjs` reads it; trailing spaces or tabs on a fence are
/// allowed. Every corpus page has one, so a file without one is an error
/// (line 0); `README.md` is not a page. Anchors, aliases and explicit tags
/// are refused, the YAML is capped at [`MAX_FRONT_MATTER_BYTES`] and its
/// nesting at [`MAX_FRONT_MATTER_DEPTH`].
pub fn split_front_matter(src: &str) -> Result<(FrontMatter, &str, usize), FrontMatterError> {
    let mut lines = src.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if !is_fence(first.strip_suffix('\n').unwrap_or(first), false) {
        // `---\r` then more on the same `\n`-line: bare `\r` line ends.
        if first.starts_with("---\r") {
            return Err(err(1, FrontMatterErrorKind::BareCarriageReturns));
        }
        return Err(err(0, FrontMatterErrorKind::Missing));
    }
    let yaml_start = first.len();
    let mut pos = yaml_start;
    let mut close = None;
    for (i, line) in lines.enumerate() {
        if is_fence(line.strip_suffix('\n').unwrap_or(line), true) {
            close = Some((pos, pos + line.len(), i + 2));
            break;
        }
        pos += line.len();
        if pos - yaml_start > MAX_FRONT_MATTER_BYTES {
            break;
        }
    }
    if pos - yaml_start > MAX_FRONT_MATTER_BYTES {
        return Err(err(
            2,
            FrontMatterErrorKind::TooLarge {
                bytes: pos - yaml_start,
            },
        ));
    }
    // Through the closing fence, or as far as the search went.
    let mut block = &src[..close.map_or(pos, |(_, body_start, _)| body_start)];
    // A `\r` ending the file on the closing fence ends Wiki.js's block too
    // (`(?:\n|\r)-{3}`, then `(?:\n|\r)*`), and no line follows it.
    if close.is_some_and(|(_, body_start, _)| body_start == src.len()) {
        block = block.strip_suffix('\r').unwrap_or(block);
    }
    if let Some(at) = bare_carriage_return(block) {
        let line = block[..at].matches('\n').count() + 1;
        return Err(err(line, FrontMatterErrorKind::BareCarriageReturns));
    }
    let Some((yaml_end, body_start, close_line)) = close else {
        return Err(err(1, FrontMatterErrorKind::Unterminated));
    };
    let yaml = &src[yaml_start..yaml_end];
    let mut front = parse(yaml)?;
    front
        .wikijs
        .extend(wikijs_fence_finding(src, yaml_end, close_line));
    Ok((front, &src[body_start..], close_line))
}

/// The byte offset of the first `\r` not followed by `\n`.
fn bare_carriage_return(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    (0..b.len()).find(|&i| b[i] == b'\r' && b.get(i + 1) != Some(&b'\n'))
}

/// Where Wiki.js's front-matter regex ends the YAML: the byte offset of the
/// newline before the first line that starts with `---`, at least one byte
/// into the block. `None` when it sees no front matter: the first line is
/// not exactly `---`, or no later line starts with `---`.
///
/// `/^(-{3}(?:\n|\r)([\w\W]+?)(?:\n|\r)-{3})?(?:\n|\r)*([\w\W]*)*/`
/// (`server/models/pages.js`, `frontmatterRegex.markdown`).
fn wikijs_yaml_end(src: &str) -> Option<usize> {
    let b = src.as_bytes();
    if !(b.starts_with(b"---") && matches!(b.get(3), Some(b'\n' | b'\r'))) {
        return None;
    }
    // `[\w\W]+?` takes at least one byte after the opener's newline.
    (5..b.len()).find(|&j| matches!(b[j], b'\n' | b'\r') && b[j + 1..].starts_with(b"---"))
}

/// A finding when Wiki.js's import would not read the block Rubrica read.
fn wikijs_fence_finding(
    src: &str,
    yaml_end: usize,
    close_line: usize,
) -> Option<FrontMatterFinding> {
    let opener = src.split('\n').next().unwrap_or("");
    match wikijs_yaml_end(src) {
        // Rubrica's block ends at `yaml_end`, the start of the closing line;
        // Wiki.js's at the newline just before it.
        Some(end) if end + 1 == yaml_end => None,
        Some(end) => Some(FrontMatterFinding::WikijsClosesFrontMatterElsewhere {
            line: close_line,
            wikijs_line: src[..=end].matches('\n').count() + 1,
        }),
        None if opener.trim_end_matches('\r') != "---" => {
            Some(FrontMatterFinding::NoFrontMatterForWikijs {
                line: 1,
                reason: "the opening `---` has trailing whitespace; Wiki.js needs the line to be exactly `---`",
            })
        }
        None => Some(FrontMatterFinding::NoFrontMatterForWikijs {
            line: close_line,
            reason: "Wiki.js closes front matter only at a line starting `---`, never `...`, and finds none",
        }),
    }
}

/// What [`scan`] finds in the YAML's parse events.
struct Scanned {
    /// The 1-based source line of every top-level key.
    key_lines: BTreeMap<String, usize>,
    /// The text of every top-level value that is a plain scalar, as written
    /// (js-yaml resolves its type from it: `True`, `0x1`).
    plain_values: BTreeMap<String, String>,
}

/// The 1-based source line of the first anchor or tag in `yaml` (it starts
/// on line 2). The parser marks a node with an empty value where the next
/// token starts (`- !!str⏎---`: the fence), so a refused node is reported
/// where its first property is; it is the first in the YAML, since every
/// node before it had none.
fn first_property_line(yaml: &str) -> Option<usize> {
    Scanner::new(yaml.chars())
        .find(|Token(_, t)| matches!(t, TokenType::Anchor(_) | TokenType::Tag(..)))
        .map(|Token(mark, _)| mark.line() + 1)
}

/// Walks the YAML's parse events before anything is loaded: refuses anchors,
/// aliases, explicit tags and deep nesting, and returns the 1-based source
/// line of every top-level key (the YAML starts on line 2) and the text of
/// every top-level plain scalar value.
fn scan(yaml: &str) -> Result<Scanned, FrontMatterError> {
    // One frame per open collection; a mapping's frame says whether its
    // next node is a key.
    enum Frame {
        Map { next_is_key: bool },
        Seq,
    }
    let mut parser = Parser::new_from_str(yaml);
    let mut stack: Vec<Frame> = Vec::new();
    let mut keys = BTreeMap::new();
    let mut plain_values = BTreeMap::new();
    let mut last_key: Option<String> = None;
    loop {
        let (event, mark) = parser.next_token().map_err(yaml_error)?;
        let mut line = mark.line() + 1;
        // A node starts: a scalar, an alias or a collection.
        let (anchor, tagged, opens) = match &event {
            Event::StreamEnd => break,
            Event::Alias(_) => return Err(err(line, FrontMatterErrorKind::Alias)),
            Event::Scalar(_, _, anchor, tag) => (*anchor, tag.is_some(), None),
            Event::SequenceStart(anchor, tag) => (*anchor, tag.is_some(), Some(Frame::Seq)),
            Event::MappingStart(anchor, tag) => (
                *anchor,
                tag.is_some(),
                Some(Frame::Map { next_is_key: true }),
            ),
            Event::SequenceEnd | Event::MappingEnd => {
                stack.pop();
                continue;
            }
            _ => continue,
        };
        if anchor != 0 || tagged {
            line = first_property_line(yaml).unwrap_or(line);
        }
        if anchor != 0 {
            return Err(err(line, FrontMatterErrorKind::Anchor));
        }
        if tagged {
            return Err(err(line, FrontMatterErrorKind::Tag));
        }
        let top_level = stack.len() == 1;
        if let Some(Frame::Map { next_is_key }) = stack.last_mut() {
            if top_level {
                match (&event, *next_is_key) {
                    (Event::Scalar(key, ..), true) => {
                        keys.entry(key.clone()).or_insert(line);
                        last_key = Some(key.clone());
                    }
                    (_, true) => last_key = None,
                    (Event::Scalar(v, TScalarStyle::Plain, ..), false) => {
                        if let Some(key) = last_key.take() {
                            plain_values.insert(key, v.clone());
                        }
                    }
                    (_, false) => last_key = None,
                }
            }
            *next_is_key = !*next_is_key;
        }
        if let Some(frame) = opens {
            stack.push(frame);
            if stack.len() > MAX_FRONT_MATTER_DEPTH {
                return Err(err(line, FrontMatterErrorKind::TooDeep));
            }
        }
    }
    Ok(Scanned {
        key_lines: keys,
        plain_values,
    })
}

/// A yaml-rust2 error at its source line: its lines are 1-based within the
/// block, which starts on source line 2.
fn yaml_error(e: ScanError) -> FrontMatterError {
    err(
        e.marker().line() + 1,
        FrontMatterErrorKind::Yaml(e.to_string()),
    )
}

/// The tags Wiki.js 2.5.314 makes of a `tags:` string: `tags.split(', ')`
/// (`server/modules/storage/disk/common.js`), then each piece trimmed and
/// lowercased, and duplicates dropped, first kept (`server/models/tags.js`
/// `associateTags`: `_.uniq(tags.map(t => _.trim(t).toLowerCase()))`).
fn wikijs_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split(", ").map(|t| t.trim().to_lowercase()) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Whether js-yaml 3.14.0's `safeLoad` (the default safe schema: null,
/// bool, int, float, in that order) reads a plain scalar as `true` or as a
/// number `=== 1`: `true`, `True`, `TRUE`; an int `1`, `+1`, `01` (octal),
/// `0x1`, `0b1`, `0_1`; a float `1.0`, `1.`, `1e0`, `10e-1`, `1_0e-1`.
/// `0o1`, `1_` and `0:1` are strings to it, and `1:0` (base 60) is 60.
fn js_yaml_true_or_one(s: &str) -> bool {
    if matches!(s, "true" | "True" | "TRUE") {
        return true;
    }
    match js_yaml_int(s) {
        Some(v) => v == 1.0,
        None => js_yaml_float(s) == Some(1.0),
    }
}

/// A leading `-` or `+`, as a sign, and the rest.
fn js_yaml_sign(s: &str) -> (f64, &str) {
    match s.as_bytes().first() {
        Some(b'-') => (-1.0, &s[1..]),
        Some(b'+') => (1.0, &s[1..]),
        _ => (1.0, s),
    }
}

/// Digits and underscores, at least one digit.
fn js_yaml_digits(t: &str) -> bool {
    t.bytes().any(|b| b.is_ascii_digit()) && t.bytes().all(|b| b.is_ascii_digit() || b == b'_')
}

/// A base-60 part after a `:`, `[0-5]?[0-9]`.
fn js_yaml_sexagesimal(p: &str) -> bool {
    match p.as_bytes() {
        [d] => d.is_ascii_digit(),
        [t, d] => (b'0'..=b'5').contains(t) && d.is_ascii_digit(),
        _ => false,
    }
}

/// `t` without its underscores, as a number.
fn js_yaml_number(t: &str) -> Option<f64> {
    t.replace('_', "").parse().ok()
}

/// js-yaml 3.14.0's `int` type (`resolveYamlInteger`, `constructYamlInteger`).
fn js_yaml_int(s: &str) -> Option<f64> {
    let (sign, body) = js_yaml_sign(s);
    // Underscores anywhere but last; at least one digit.
    let radix = |t: &str, radix: u32| -> Option<f64> {
        if t.ends_with('_') || !t.bytes().any(|b| b != b'_') {
            return None;
        }
        t.chars().filter(|&c| c != '_').try_fold(0.0, |v, c| {
            c.to_digit(radix)
                .map(|d| v * f64::from(radix) + f64::from(d))
        })
    };
    let value = if body == "0" {
        0.0
    } else if let Some(rest) = body.strip_prefix("0b") {
        radix(rest, 2)?
    } else if let Some(rest) = body.strip_prefix("0x") {
        radix(rest, 16)?
    } else if let Some(rest) = body.strip_prefix('0') {
        radix(rest, 8)?
    } else if body.starts_with('_') {
        return None;
    } else if let Some((head, tail)) = body.split_once(':') {
        // Base 60: `1:30` is 90.
        if !js_yaml_digits(head) || !tail.split(':').all(js_yaml_sexagesimal) {
            return None;
        }
        tail.split(':').try_fold(js_yaml_number(head)?, |v, p| {
            Some(v * 60.0 + js_yaml_number(p)?)
        })?
    } else {
        radix(body, 10)?
    };
    Some(sign * value)
}

/// js-yaml 3.14.0's `float` type, finite values only (`.inf` and `.nan` are
/// never 1): `[-+]?(0|[1-9][0-9_]*)(\.[0-9_]*)?([eE][-+]?[0-9]+)?`,
/// `\.[0-9_]+([eE][-+]?[0-9]+)?` and base 60
/// `[-+]?[0-9][0-9_]*(:[0-5]?[0-9])+\.[0-9_]*`, not ending in `_`.
fn js_yaml_float(s: &str) -> Option<f64> {
    if s.ends_with('_') {
        return None;
    }
    let (sign, body) = js_yaml_sign(s);
    let digits_or_none = |t: &str| t.bytes().all(|b| b.is_ascii_digit() || b == b'_');
    if let Some((head, rest)) = body.split_once(':') {
        let (parts, frac) = rest.rsplit_once('.')?;
        let head_ok =
            head.as_bytes().first().is_some_and(u8::is_ascii_digit) && js_yaml_digits(head);
        if !head_ok || !parts.split(':').all(js_yaml_sexagesimal) || !digits_or_none(frac) {
            return None;
        }
        let mut parts: Vec<&str> = parts.split(':').collect();
        let last = parts.pop()?;
        let whole = parts.into_iter().try_fold(js_yaml_number(head)?, |v, p| {
            Some(v * 60.0 + js_yaml_number(p)?)
        })?;
        return Some(sign * (whole * 60.0 + js_yaml_number(&format!("{last}.{frac}0"))?));
    }
    let (mantissa, exp) = match body.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (body, None),
    };
    if let Some(e) = exp {
        let d = e.strip_prefix(['-', '+']).unwrap_or(e);
        if d.is_empty() || !d.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let (int, frac) = match mantissa.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mantissa, None),
    };
    let ok = match (int, frac) {
        // `.5`: no sign, and a digit after the dot.
        ("", Some(f)) => body.len() == s.len() && js_yaml_digits(f),
        ("", None) => false,
        (i, f) => {
            (i == "0" || (!i.starts_with(['0', '_']) && js_yaml_digits(i)))
                && f.is_none_or(digits_or_none)
        }
    };
    if !ok {
        return None;
    }
    let mut text = format!("0{}", int.replace('_', ""));
    if let Some(f) = frac {
        text.push('.');
        text.push_str(&f.replace('_', ""));
        text.push('0');
    }
    if let Some(e) = exp {
        text.push('e');
        text.push_str(e);
    }
    text.parse::<f64>().ok().map(|v| sign * v)
}

fn parse(yaml: &str) -> Result<FrontMatter, FrontMatterError> {
    // Before loading: YamlLoader expands aliases without bound.
    let Scanned {
        key_lines,
        plain_values,
    } = scan(yaml)?;
    let mut docs = YamlLoader::load_from_str(yaml).map_err(yaml_error)?;
    if docs.len() > 1 {
        return Err(err(
            2,
            FrontMatterErrorKind::Yaml("more than one YAML document".into()),
        ));
    }
    let map = match docs.pop().unwrap_or(Yaml::Null) {
        Yaml::Hash(m) => m,
        Yaml::Null => Default::default(),
        _ => {
            return Err(err(
                2,
                FrontMatterErrorKind::Yaml("front matter is not a mapping".into()),
            ))
        }
    };
    let line_of = |key: &str| key_lines.get(key).copied().unwrap_or(1);
    let bad = |key: &str, expected: &'static str| {
        err(
            line_of(key),
            FrontMatterErrorKind::BadValue {
                key: key.to_owned(),
                expected,
            },
        )
    };

    let mut title = None;
    let mut front = FrontMatter {
        title: String::new(),
        description: None,
        published: true,
        date: None,
        date_created: None,
        tags: Vec::new(),
        editor: None,
        redirect_to: None,
        extra: BTreeMap::new(),
        key_lines: BTreeMap::new(),
        wikijs: Vec::new(),
    };
    let string = |key: &str, v: &Yaml| match v {
        Yaml::Null => Ok(None),
        Yaml::String(s) => Ok(Some(s.clone())),
        _ => Err(bad(key, "a string")),
    };
    let date = |key: &str, v: &Yaml| match v {
        Yaml::Null => Ok(None),
        Yaml::String(s) => DateTime::parse_from_rfc3339(s.trim())
            .map(|d| Some(d.with_timezone(&Utc)))
            .map_err(|_| bad(key, "an ISO 8601 date-time")),
        _ => Err(bad(key, "an ISO 8601 date-time")),
    };
    for (k, v) in map {
        let Yaml::String(key) = k else {
            return Err(err(
                1,
                FrontMatterErrorKind::Yaml("a key is not a string".into()),
            ));
        };
        match key.as_str() {
            "title" => title = string(&key, &v)?,
            "description" => front.description = string(&key, &v)?,
            "published" => {
                front.published = match v {
                    Yaml::Null => true,
                    Yaml::Boolean(b) => b,
                    _ => return Err(bad(&key, "true or false")),
                };
            }
            "date" => front.date = date(&key, &v)?,
            "dateCreated" => front.date_created = date(&key, &v)?,
            "tags" => {
                front.tags = match &v {
                    Yaml::Null => Vec::new(),
                    Yaml::String(s) => s.split(',').map(str::to_owned).collect(),
                    Yaml::Array(items) => {
                        front
                            .wikijs
                            .push(FrontMatterFinding::TagListBreaksWikijsSync {
                                line: line_of(&key),
                            });
                        items
                            .iter()
                            .map(|t| match t {
                                Yaml::String(s) => Ok(s.clone()),
                                _ => Err(bad(&key, "a list of strings")),
                            })
                            .collect::<Result<_, _>>()?
                    }
                    _ => return Err(bad(&key, "a comma-separated string or a list")),
                };
                front.tags = front
                    .tags
                    .iter()
                    .map(|t| t.trim())
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned)
                    .collect();
                if let Yaml::String(s) = &v {
                    let wikijs = wikijs_tags(s);
                    let lower: Vec<_> = front.tags.iter().map(|t| t.to_lowercase()).collect();
                    if wikijs != lower {
                        front
                            .wikijs
                            .push(FrontMatterFinding::TagsSplitDifferentlyByWikijs {
                                line: line_of(&key),
                                wikijs,
                            });
                    }
                }
            }
            "editor" => front.editor = string(&key, &v)?,
            "redirect_to" => front.redirect_to = string(&key, &v)?,
            _ => {
                front.extra.insert(key, v);
            }
        }
    }
    // Wiki.js publishes only an `isPublished` js-yaml reads as `true` or 1;
    // with none, a new page is published and an existing one keeps its state.
    let wikijs_may_publish = !front.extra.contains_key("isPublished")
        || plain_values
            .get("isPublished")
            .is_some_and(|v| js_yaml_true_or_one(v));
    if !front.published && wikijs_may_publish {
        front
            .wikijs
            .push(FrontMatterFinding::PublishedIgnoredByWikijs {
                line: line_of("published"),
            });
    }
    // A new page's `createPage` inserts `isPublished` as it is, into a
    // `{type: 'boolean'}` column: only js-yaml's booleans pass.
    if front.extra.contains_key("isPublished")
        && !plain_values.get("isPublished").is_some_and(|v| {
            matches!(
                v.as_str(),
                "true" | "True" | "TRUE" | "false" | "False" | "FALSE"
            )
        })
    {
        front
            .wikijs
            .push(FrontMatterFinding::IsPublishedBreaksWikijsImport {
                line: line_of("isPublished"),
            });
    }
    match title {
        Some(t) if !t.trim().is_empty() => front.title = t,
        _ => return Err(err(1, FrontMatterErrorKind::NoTitle)),
    }
    front.key_lines = key_lines;
    Ok(front)
}
