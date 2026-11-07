//! Front matter: the corpus's shape (CSV tags, ISO dates, `published: true`)
//! and the YAML forms a hand edit can produce.

use std::time::{Duration, Instant};

use rubrica_render::frontmatter::{
    split_front_matter, FrontMatterChecks, FrontMatterErrorKind, FrontMatterFinding, Severity,
    MAX_FRONT_MATTER_BYTES,
};

const WIKIJS: FrontMatterChecks = FrontMatterChecks {
    wikijs_limits: true,
};

/// The findings `--wikijs-limits` reports for `src`, which must parse.
fn wikijs_findings(src: &str) -> Vec<FrontMatterFinding> {
    let (fm, _, _) = split_front_matter(src).unwrap_or_else(|e| panic!("{src:?}: {e:?}"));
    assert_eq!(
        fm.findings(FrontMatterChecks::default()),
        [],
        "only --wikijs-limits reports Wiki.js's reading"
    );
    fm.findings(WIKIJS)
}

const PAGE: &str = "---
title: Architecture
description: \"System architecture overview — roles\"
published: true
date: 2026-10-25T23:30:00.000Z
tags: services, wiki, identity
editor: markdown
dateCreated: 2026-06-27T02:27:30.756Z
owner: someone
---

# Architecture
";

#[test]
fn front_matter_parses_csv_tags_and_list_tags() {
    let (fm, body, offset) = split_front_matter(PAGE).unwrap();
    assert_eq!(fm.title, "Architecture");
    assert_eq!(
        fm.description.as_deref(),
        Some("System architecture overview — roles")
    );
    assert!(fm.published);
    assert_eq!(fm.tags, ["services", "wiki", "identity"]);
    assert_eq!(fm.editor.as_deref(), Some("markdown"));
    assert_eq!(fm.date.unwrap().to_rfc3339(), "2026-10-25T23:30:00+00:00");
    assert_eq!(
        fm.date_created.unwrap().to_rfc3339(),
        "2026-06-27T02:27:30.756+00:00"
    );
    assert_eq!(fm.redirect_to, None);
    assert_eq!(fm.extra.len(), 1, "unknown keys are kept: {:?}", fm.extra);
    assert!(fm.extra.contains_key("owner"));
    assert_eq!(fm.key_lines["tags"], 6);
    // The body starts on the line after the closing `---`, and the offset
    // maps body lines back to source lines.
    assert_eq!(body, "\n# Architecture\n");
    assert_eq!(offset, 10);

    let list = "---\ntitle: T\ntags:\n  - a\n  - b c\n---\n";
    let (fm, body, offset) = split_front_matter(list).unwrap();
    assert_eq!(fm.tags, ["a", "b c"]);
    assert_eq!((body, offset), ("", 6));

    // Empty pieces of a CSV are not tags; no `tags:` is no tags.
    let (fm, _, _) = split_front_matter("---\ntitle: T\ntags: a,, b ,\n---\n").unwrap();
    assert_eq!(fm.tags, ["a", "b"]);
    let (fm, _, _) = split_front_matter("---\ntitle: T\n---\n").unwrap();
    assert!(fm.tags.is_empty());
    assert!(fm.published, "published defaults to true");
}

#[test]
fn crlf_front_matter_splits_the_same() {
    let src = PAGE.replace('\n', "\r\n");
    let (fm, body, offset) = split_front_matter(&src).unwrap();
    assert_eq!(fm.title, "Architecture");
    assert_eq!(fm.tags, ["services", "wiki", "identity"]);
    assert_eq!(body, "\r\n# Architecture\r\n");
    assert_eq!(offset, 10);
}

#[test]
fn published_false_is_honoured() {
    let (fm, _, _) = split_front_matter("---\ntitle: Draft\npublished: false\n---\nx\n").unwrap();
    assert!(!fm.published);
    let err = split_front_matter("---\ntitle: Draft\npublished: maybe\n---\nx\n").unwrap_err();
    assert_eq!(err.line, 3);
    assert_eq!(
        err.kind,
        FrontMatterErrorKind::BadValue {
            key: "published".into(),
            expected: "true or false"
        }
    );
}

#[test]
fn missing_front_matter_is_an_error_with_line_zero() {
    let err = split_front_matter("# a-wiki\n\nWiki.js content\n").unwrap_err();
    assert_eq!((err.line, err.kind), (0, FrontMatterErrorKind::Missing));
    // `---` must be the very first line.
    let err = split_front_matter("\n---\ntitle: T\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (0, FrontMatterErrorKind::Missing));

    let err = split_front_matter("---\ntitle: T\n\nno closing fence\n").unwrap_err();
    assert_eq!(
        (err.line, err.kind),
        (1, FrontMatterErrorKind::Unterminated)
    );

    let err = split_front_matter("---\ndescription: d\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (1, FrontMatterErrorKind::NoTitle));

    let err = split_front_matter("---\ntitle: T\ndate: last tuesday\n---\n").unwrap_err();
    assert_eq!(err.line, 3);
    assert!(matches!(err.kind, FrontMatterErrorKind::BadValue { ref key, .. } if key == "date"));

    let err = split_front_matter("---\ntitle: T\nkey: a: b\n---\n").unwrap_err();
    assert!(matches!(err.kind, FrontMatterErrorKind::Yaml(_)), "{err:?}");
    assert_eq!(
        err.line, 3,
        "a YAML error points at its source line: {err:?}"
    );
}

#[test]
fn a_description_over_255_is_reported_only_with_wikijs_limits() {
    // Code points, not bytes: 255 em-dashes are 765 bytes and still fit.
    let ok = format!("---\ntitle: T\ndescription: {}\n---\n", "—".repeat(255));
    let (fm, _, _) = split_front_matter(&ok).unwrap();
    assert_eq!(
        fm.findings(FrontMatterChecks {
            wikijs_limits: true
        }),
        []
    );

    let long = format!("---\ntitle: T\ndescription: {}\n---\n", "—".repeat(256));
    let (fm, _, _) = split_front_matter(&long).unwrap();
    assert_eq!(fm.findings(FrontMatterChecks::default()), []);
    assert_eq!(
        fm.findings(FrontMatterChecks {
            wikijs_limits: true
        }),
        [FrontMatterFinding::TooLongForWikijs {
            field: "description",
            chars: 256,
            line: 3
        }]
    );

    let title = format!("---\ntitle: {}\n---\n", "t".repeat(300));
    let (fm, _, _) = split_front_matter(&title).unwrap();
    assert_eq!(
        fm.findings(FrontMatterChecks {
            wikijs_limits: true
        }),
        [FrontMatterFinding::TooLongForWikijs {
            field: "title",
            chars: 300,
            line: 2
        }]
    );
}

#[test]
fn block_scalar_description_folds() {
    // The four forms check-frontmatter.cjs:74-129 handles, read by a real
    // YAML parser: folded, literal, multi-line quoted, multi-line plain.
    let src = include_str!("fixtures/md/block-scalars.md");
    let (fm, body, offset) = split_front_matter(src).unwrap();
    assert_eq!(
        fm.description.as_deref(),
        Some("folded onto one line\nand a second paragraph\n")
    );
    let s = |k: &str| fm.extra[k].as_str().unwrap().to_owned();
    assert_eq!(s("summary"), "literal\nkeeps its newlines");
    assert_eq!(s("quoted"), "a quoted value that spans lines");
    assert_eq!(s("plain"), "a plain value that spans lines");
    assert_eq!(body, "\nBody.\n");
    assert_eq!(offset, 16);
}

/// "Billion laughs": every level of aliases multiplies the expansion. Seven
/// levels of nine are 9^7 = 4.8 million strings, which yaml-rust2's
/// `YamlLoader` builds in seconds; the anchor is refused before loading.
#[test]
fn a_yaml_alias_bomb_is_refused_fast() {
    let mut src = String::from("---\ntitle: T\na: &a [\"lol\",\"lol\",\"lol\",\"lol\",\"lol\",\"lol\",\"lol\",\"lol\",\"lol\"]\n");
    let names = ["a", "b", "c", "d", "e", "f", "g"];
    for w in names.windows(2) {
        let (prev, next) = (w[0], w[1]);
        let refs = vec![format!("*{prev}"); 9].join(",");
        src.push_str(&format!("{next}: &{next} [{refs}]\n"));
    }
    src.push_str("---\n");
    assert!(src.len() < 400, "a small file: {} bytes", src.len());
    let start = Instant::now();
    let err = split_front_matter(&src).unwrap_err();
    let took = start.elapsed();
    eprintln!("anchor bomb: refused in {took:?}");
    assert_eq!((err.line, err.kind), (3, FrontMatterErrorKind::Anchor));
    assert!(took < Duration::from_millis(500), "took {took:?}");
}

#[test]
fn anchors_and_aliases_are_refused_even_when_harmless() {
    let err = split_front_matter("---\ntitle: &t T\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (2, FrontMatterErrorKind::Anchor));
    let err = split_front_matter("---\ntitle: T\nk:\n  - &x 1\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (4, FrontMatterErrorKind::Anchor));
    // An alias with no anchor is a YAML error to yaml-rust2 or an Alias,
    // never a loaded page.
    let err = split_front_matter("---\ntitle: T\nk: *nope\n---\n").unwrap_err();
    assert!(
        matches!(
            err.kind,
            FrontMatterErrorKind::Alias | FrontMatterErrorKind::Yaml(_)
        ),
        "{err:?}"
    );
}

#[test]
fn front_matter_over_64_kib_is_refused() {
    let pad = "x".repeat(MAX_FRONT_MATTER_BYTES);
    let src = format!("---\ntitle: T\ndescription: {pad}\n---\n");
    let err = split_front_matter(&src).unwrap_err();
    assert!(
        matches!(err.kind, FrontMatterErrorKind::TooLarge { bytes } if bytes > MAX_FRONT_MATTER_BYTES),
        "{err:?}"
    );
    // Just under the cap still parses.
    let pad = "x".repeat(MAX_FRONT_MATTER_BYTES - 64);
    let src = format!("---\ntitle: T\ndescription: {pad}\n---\n");
    assert!(split_front_matter(&src).is_ok());
}

#[test]
fn deeply_nested_yaml_is_refused_without_overflowing() {
    // 30,000 levels of `[` fit in 64 KiB; dropping a tree that deep recurses
    // once per level.
    let deep = format!(
        "---\ntitle: T\nk: {}{}\n---\n",
        "[".repeat(30_000),
        "]".repeat(30_000)
    );
    let err = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || split_front_matter(&deep).unwrap_err())
        .unwrap()
        .join()
        .unwrap();
    // yaml-rust2's scanner has its own limit (256 flow levels) and may stop
    // first; either way it is an error, not an overflow.
    assert!(
        match &err.kind {
            FrontMatterErrorKind::TooDeep => true,
            FrontMatterErrorKind::Yaml(m) => m.contains("recursion limit"),
            _ => false,
        },
        "{err:?}"
    );
    let deep = format!(
        "---\ntitle: T\nk: {}{}\n---\n",
        "[".repeat(33),
        "]".repeat(33)
    );
    let err = split_front_matter(&deep).unwrap_err();
    assert_eq!((err.line, err.kind), (3, FrontMatterErrorKind::TooDeep));
    let deep = format!(
        "---\ntitle: T\nk: {}{}\n---\n",
        "[".repeat(31),
        "]".repeat(31)
    );
    assert!(
        split_front_matter(&deep).is_ok(),
        "32 levels with the top mapping"
    );
    // The corpus's nesting (a list under a key) is fine.
    let ok = "---\ntitle: T\nk:\n  - [a, {b: [c]}]\n---\n";
    assert!(split_front_matter(ok).is_ok());
}

#[test]
fn key_lines_come_from_the_yaml_parser_not_the_text() {
    // A flow mapping is one line with two keys; a quoted value's
    // continuation is not a key.
    let (fm, _, _) =
        split_front_matter("---\n{title: T,\n description: d,\n q: \"a\n  b: c\"}\n---\n").unwrap();
    assert_eq!(
        fm.key_lines
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect::<Vec<_>>(),
        [("description", 3), ("q", 4), ("title", 2)]
    );
}

#[test]
fn published_false_is_reported_as_ignored_by_wikijs() {
    // Rubrica's own semantics: `published: false` hides the page. Wiki.js's
    // git import reads `isPublished`, so it would publish this page.
    assert_eq!(
        wikijs_findings("---\ntitle: Draft\npublished: false\n---\n"),
        [FrontMatterFinding::PublishedIgnoredByWikijs { line: 3 }]
    );
    assert_eq!(
        FrontMatterFinding::PublishedIgnoredByWikijs { line: 3 }.severity(),
        Severity::Warning
    );
    assert_eq!(wikijs_findings("---\ntitle: T\npublished: true\n---\n"), []);
}

#[test]
fn list_tags_are_an_error_for_wikijs() {
    // `pageData.tags.split(', ')` on an array throws: the page never syncs.
    let found = wikijs_findings("---\ntitle: T\ntags:\n  - a\n  - b\n---\n");
    assert_eq!(
        found,
        [FrontMatterFinding::TagListBreaksWikijsSync { line: 3 }]
    );
    assert_eq!(found[0].severity(), Severity::Error);
    assert_eq!(wikijs_findings("---\ntitle: T\ntags: []\n---\n").len(), 1);
}

#[test]
fn tags_wikijs_splits_differently_are_reported() {
    // Wiki.js splits on ", " exactly, then trims.
    let found = wikijs_findings("---\ntitle: T\ntags: a,b, c\n---\n");
    assert_eq!(
        found,
        [FrontMatterFinding::TagsSplitDifferentlyByWikijs {
            line: 3,
            wikijs: vec!["a,b".into(), "c".into()]
        }]
    );
    assert_eq!(found[0].severity(), Severity::Warning);
    let found = wikijs_findings("---\ntitle: T\ntags: a ,b\n---\n");
    assert!(
        matches!(&found[..], [FrontMatterFinding::TagsSplitDifferentlyByWikijs { wikijs, .. }] if wikijs == &["a ,b"]),
        "{found:?}"
    );
    // The corpus's form, and extra spaces, split the same.
    assert_eq!(
        wikijs_findings("---\ntitle: T\ntags: services, wiki, identity\n---\n"),
        []
    );
    assert_eq!(
        wikijs_findings("---\ntitle: T\ntags: a,  b ,  c\n---\n"),
        []
    );
    assert_eq!(wikijs_findings("---\ntitle: T\ntags: a,  b\n---\n"), []);
    assert_eq!(wikijs_findings("---\ntitle: T\ntags:\n---\n"), []);
    assert_eq!(wikijs_findings("---\ntitle: T\ntags: solo\n---\n"), []);
}

#[test]
fn fences_wikijs_reads_differently_are_reported() {
    // Wiki.js 2.5.314 server/models/pages.js:
    // /^(-{3}(?:\n|\r)([\w\W]+?)(?:\n|\r)-{3})?(?:\n|\r)*([\w\W]*)*/
    let no_fm = |line| move |f: &FrontMatterFinding| matches!(f, FrontMatterFinding::NoFrontMatterForWikijs { line: l, .. } if *l == line);
    // An opener with trailing spaces: Wiki.js sees no front matter.
    let found = wikijs_findings("--- \ntitle: T\n---\nx\n");
    assert!(found.len() == 1 && no_fm(1)(&found[0]), "{found:?}");
    assert_eq!(found[0].severity(), Severity::Error);
    let found = wikijs_findings("---\t\ntitle: T\n---\nx\n");
    assert!(found.len() == 1 && no_fm(1)(&found[0]), "{found:?}");

    // `...` closes YAML, not Wiki.js's front matter: with no later `---`
    // line Wiki.js has none; with one it reads on to it.
    let found = wikijs_findings("---\ntitle: T\n...\nx\n");
    assert!(found.len() == 1 && no_fm(3)(&found[0]), "{found:?}");
    assert_eq!(
        wikijs_findings("---\ntitle: T\n...\nx\n\n---\n\ny\n"),
        [FrontMatterFinding::WikijsClosesFrontMatterElsewhere {
            line: 3,
            wikijs_line: 6
        }]
    );
    assert_eq!(
        FrontMatterFinding::WikijsClosesFrontMatterElsewhere {
            line: 3,
            wikijs_line: 6
        }
        .severity(),
        Severity::Error
    );

    // A line starting `---` inside the YAML closes it for Wiki.js.
    for dashes in ["----", "---x"] {
        let src = format!("---\n{{title: T,\n{dashes}: 2}}\n---\n");
        assert_eq!(
            wikijs_findings(&src),
            [FrontMatterFinding::WikijsClosesFrontMatterElsewhere {
                line: 4,
                wikijs_line: 3
            }],
            "{src:?}"
        );
        // As a closing fence, Rubrica refuses it too (it is not `---`).
        let src = format!("---\ntitle: T\n{dashes}\n");
        assert!(split_front_matter(&src).is_err(), "{src:?}");
    }

    // A closer with trailing spaces, and CRLF, read the same.
    assert_eq!(wikijs_findings("---\ntitle: T\n---  \nx\n"), []);
    assert_eq!(wikijs_findings("---\r\ntitle: T\r\n---\r\nx\r\n"), []);
}

#[test]
fn finding_lines_and_severities() {
    let f = FrontMatterFinding::TooLongForWikijs {
        field: "title",
        chars: 256,
        line: 2,
    };
    assert_eq!((f.line(), f.severity()), (2, Severity::Error));
    let f = FrontMatterFinding::TagsSplitDifferentlyByWikijs {
        line: 7,
        wikijs: vec![],
    };
    assert_eq!(f.line(), 7);
}

/// js-yaml 3.14.0's `safeLoad` throws on these tags, and Wiki.js's
/// `parseMetadata` then imports the page with no front matter (titled by its
/// file name, the YAML as body). Rubrica refuses every explicit tag, as it
/// refuses anchors: the corpus has none.
#[test]
fn explicit_yaml_tags_are_refused() {
    for (yaml, line) in [
        ("title: T\nx: !foo bar\n", 3),
        ("title: T\nx: !!js/regexp /a/\n", 3),
        ("title: T\nx: !!float abc\n", 3),
        ("title: T\nx: !!int 1.5\n", 3),
        ("title: T\ny: !!null \"\"\n", 3),
        // Tags js-yaml accepts are refused too: one rule, no tag table.
        ("title: !!str T\n", 2),
        ("title: T\ntags: !!seq [a]\n", 3),
        ("title: T\nx: !!binary aGk=\n", 3),
    ] {
        let src = format!("---\n{yaml}---\n");
        let err = split_front_matter(&src).unwrap_err();
        assert_eq!(
            (err.line, err.kind),
            (line, FrontMatterErrorKind::Tag),
            "{src:?}"
        );
    }
    // A tag on the top mapping itself.
    let err = split_front_matter("---\n!foo\ntitle: T\n---\n").unwrap_err();
    assert_eq!(err.kind, FrontMatterErrorKind::Tag, "{err:?}");
}

/// An anchor or tag on an empty value is reported on its key's line, not on
/// the line after (the closing fence, when it is the last value).
#[test]
fn an_anchor_or_tag_on_an_empty_last_value_is_reported_on_its_key() {
    let err = split_front_matter("---\ntitle: T\nx: &a\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (3, FrontMatterErrorKind::Anchor));
    let err = split_front_matter("---\ntitle: T\nx: !foo\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (3, FrontMatterErrorKind::Tag));
    let err = split_front_matter("---\ntitle: T\nx: &a\ny: 1\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (3, FrontMatterErrorKind::Anchor));
}

#[test]
fn published_false_with_is_published_false_is_not_reported() {
    // Wiki.js obeys `isPublished: false`: it hides the page too.
    assert_eq!(
        wikijs_findings("---\ntitle: T\npublished: false\nisPublished: false\n---\n"),
        []
    );
    assert_eq!(
        wikijs_findings("---\ntitle: T\npublished: false\nisPublished: true\n---\n"),
        [FrontMatterFinding::PublishedIgnoredByWikijs { line: 3 }]
    );
}

#[test]
fn tags_wikijs_merges_by_case_are_reported() {
    // Wiki.js trims, lowercases and dedupes (`_.uniq`): `A, a` is one tag.
    assert_eq!(
        wikijs_findings("---\ntitle: T\ntags: A, a\n---\n"),
        [FrontMatterFinding::TagsSplitDifferentlyByWikijs {
            line: 3,
            wikijs: vec!["a".into()]
        }]
    );
    assert_eq!(
        wikijs_findings("---\ntitle: T\ntags: Ops, ops, b\n---\n"),
        [FrontMatterFinding::TagsSplitDifferentlyByWikijs {
            line: 3,
            wikijs: vec!["ops".into(), "b".into()]
        }]
    );
    // Lowercasing alone merges nothing.
    assert_eq!(wikijs_findings("---\ntitle: T\ntags: Ops, B\n---\n"), []);
}

#[test]
fn a_file_with_bare_cr_line_endings_says_so() {
    // Wiki.js's regex takes `\r` as a line end; Rubrica reads `\n` and
    // `\r\n` only, and says that is why it sees no front matter.
    let err = split_front_matter("---\rtitle: T\r---\rx\r").unwrap_err();
    assert_eq!(
        (err.line, err.kind),
        (1, FrontMatterErrorKind::BareCarriageReturns)
    );
    // Anything else on the first line is still `Missing`.
    let err = split_front_matter("--- x\ntitle: T\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (0, FrontMatterErrorKind::Missing));
}

#[test]
fn a_dash_line_inside_block_yaml_is_a_yaml_error() {
    // `WikijsClosesFrontMatterElsewhere` for a `----` line needs flow YAML
    // (above); in block YAML the line is a stray scalar, a parse error.
    for dashes in ["----", "---x"] {
        let src = format!("---\ntitle: T\n{dashes}\n---\n");
        let err = split_front_matter(&src).unwrap_err();
        assert!(
            matches!(err.kind, FrontMatterErrorKind::Yaml(_)),
            "{src:?}: {err:?}"
        );
    }
}

/// A page whose front matter ends its lines in `\r\n` and whose body mixes in
/// bare `\r`s: the front matter splits, and the body's lines are the previous renderer's.
#[test]
fn a_bare_carriage_return_in_the_body_is_a_line_end_there() {
    let src = "---\r\ntitle: x\r\n---\r\n- a\r- [ ] b\r\n";
    let (fm, body, offset) = split_front_matter(src).unwrap();
    assert_eq!(
        (fm.title.as_str(), body, offset),
        ("x", "- a\r- [ ] b\r\n", 3)
    );
    let out = rubrica_render::render(body, &rubrica_render::RenderOptions::default());
    assert_eq!(
        out.task_items
            .iter()
            .map(|t| t.line + offset)
            .collect::<Vec<_>>(),
        [5]
    );
}

/// In the front matter Rubrica reads `\n` and `\r\n` line ends only (its
/// fences, its line numbers), so a bare `\r` anywhere in the block, fences
/// included, is refused at its line, as a file of bare `\r`s is.
#[test]
fn a_bare_carriage_return_in_the_front_matter_is_refused_at_its_line() {
    for (src, line) in [
        ("---\ntitle: T\rdescription: d\n---\n", 2),
        ("---\r\ntitle: T\r\nx: 1\r---\r\nbody\n", 3),
        ("---\ntitle: T\n---\rbody\n---\n", 3),
        ("---\ntitle: T\n---\r\r\nbody\n", 3),
        // Unterminated but for the `\r`: still the `\r` that is reported.
        ("---\ntitle: T\r---\rbody\n", 2),
    ] {
        let err = split_front_matter(src).unwrap_err();
        assert_eq!(
            (err.line, err.kind),
            (line, FrontMatterErrorKind::BareCarriageReturns),
            "{src:?}"
        );
    }
}

/// Wiki.js 2.5.314's `updatePage` stores `isPublished === true ||
/// isPublished === 1` (js-yaml 3.14.0's reading of the value), and
/// `createPage`'s schema takes only a boolean: any other value leaves the
/// page unpublished (or unimported), so `published: false` hides it in both.
/// The finding stays when Wiki.js may publish: no `isPublished` (a new page
/// is published, an existing one keeps its state), or one js-yaml reads as
/// `true` or 1.
#[test]
fn published_false_is_reported_unless_wikijs_also_unpublishes() {
    let reported = |v: &str| {
        let src = format!("---\ntitle: T\npublished: false\nisPublished: {v}\n---\n");
        // A value that is no boolean also breaks a new page's import (below).
        let mut found = wikijs_findings(&src);
        found.retain(|f| !matches!(f, FrontMatterFinding::IsPublishedBreaksWikijsImport { .. }));
        match found.as_slice() {
            [] => false,
            [FrontMatterFinding::PublishedIgnoredByWikijs { line: 3 }] => true,
            other => panic!("{v:?}: {other:?}"),
        }
    };
    // Each value as js-yaml 3.14.0 `safeLoad` reads it (run on the replica):
    // these are `true` or `=== 1`, so Wiki.js publishes.
    for v in [
        "true", "True", "TRUE", "1", "+1", "01", "0x1", "0b1", "1.0", "1.", "1e0", "+1.0", "10e-1",
        " 1 ", "1_0e-1",
    ] {
        assert!(
            reported(v),
            "{v:?}: Wiki.js publishes, so Rubrica's hiding is a difference"
        );
    }
    // These are not: strings (`no`, quoted `"false"`, `"true"`, `"1"`, `1_`,
    // `0o1`, `0:1`), other numbers (`0`, base 60 `1:0` is 60), null.
    for v in [
        "false",
        "no",
        "yes",
        "on",
        "0",
        "\"false\"",
        "\"true\"",
        "'1'",
        "\"1\"",
        "1_",
        "0o1",
        "0:1",
        "1:0",
        "~",
        "null",
        "",
        "tRue",
        "[true]",
        "{a: 1}",
        "2",
    ] {
        assert!(!reported(v), "{v:?}: Wiki.js does not publish either");
    }
    // No `isPublished` at all: reported.
    assert_eq!(
        wikijs_findings("---\ntitle: T\npublished: false\n---\n"),
        [FrontMatterFinding::PublishedIgnoredByWikijs { line: 3 }]
    );
}

/// An anchor or tag on an empty sequence item is reported on the item's line,
/// not where the next token starts.
#[test]
fn an_anchor_or_tag_on_an_empty_sequence_item_is_reported_on_its_line() {
    let err = split_front_matter("---\ntitle: T\nd:\n  - a\n  - !!str\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (5, FrontMatterErrorKind::Tag));
    let err = split_front_matter("---\ntitle: T\nd:\n  - &a\n  - b\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (4, FrontMatterErrorKind::Anchor));
    let err = split_front_matter("---\ntitle: T\nd:\n- &a\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (4, FrontMatterErrorKind::Anchor));
    // On a value on the line after its key: the anchor's own line.
    let err = split_front_matter("---\ntitle: T\nx:\n  &a\ny: 1\n---\n").unwrap_err();
    assert_eq!((err.line, err.kind), (4, FrontMatterErrorKind::Anchor));
}

/// A closing fence that ends the file with a `\r` (`---\r` and no `\n`):
/// Wiki.js's regex closes there (`(?:\n|\r)-{3}`, then `(?:\n|\r)*` takes
/// the `\r`), and there is no line after it for Rubrica to number, so it is
/// not a bare `\r` to refuse. The body is empty.
#[test]
fn a_carriage_return_ending_the_file_on_the_closing_fence_is_accepted() {
    for src in ["---\ntitle: T\n---\r", "---\r\ntitle: T\r\n---\r"] {
        let (fm, body, offset) =
            split_front_matter(src).unwrap_or_else(|e| panic!("{src:?}: {e:?}"));
        assert_eq!((fm.title.as_str(), body, offset), ("T", "", 3), "{src:?}");
        assert_eq!(fm.findings(WIKIJS), [], "{src:?}");
    }
    // Anywhere else it is still refused: before more text, on the opening
    // fence, inside the YAML.
    for (src, line) in [
        ("---\ntitle: T\n---\rx", 3),
        ("---\ntitle: T\n---\r\r", 3),
        ("---\rtitle: T\n---\r", 1),
        ("---\ntitle: T\r", 2),
    ] {
        let err = split_front_matter(src).unwrap_err();
        assert_eq!(
            (err.line, err.kind),
            (line, FrontMatterErrorKind::BareCarriageReturns),
            "{src:?}"
        );
    }
}

/// For a page Wiki.js does not have yet, its git import calls `createPage`
/// with `isPublished: _.get(pageData, 'isPublished', true)`
/// (`server/modules/storage/disk/common.js`, 2.5.314), and `createPage`
/// inserts it as it is into a model whose schema says
/// `isPublished: {type: 'boolean'}`: any value js-yaml 3.14.0 does not read
/// as a boolean fails the insert, and the page is never imported. An
/// existing page's `updatePage` coerces instead, so this is the new page's
/// failure, reported as an error as `TagListBreaksWikijsSync` is.
#[test]
fn an_is_published_wikijs_cannot_store_breaks_a_new_pages_import() {
    let found = |v: &str| wikijs_findings(&format!("---\ntitle: T\nisPublished: {v}\n---\n"));
    // js-yaml's booleans (on the replica: `true|True|TRUE|false|False|FALSE`).
    for v in ["true", "True", "TRUE", "false", "False", "FALSE"] {
        assert_eq!(found(v), [], "{v:?}");
    }
    // Numbers, null, strings (YAML 1.1's `yes`, quoted booleans, `tRue`),
    // collections.
    for v in [
        "1", "0", "0x1", "1.0", "~", "null", "", "yes", "no", "on", "tRue", "\"true\"", "'false'",
        "[true]", "{a: 1}",
    ] {
        let f = found(v);
        assert_eq!(
            f,
            [FrontMatterFinding::IsPublishedBreaksWikijsImport { line: 3 }],
            "{v:?}"
        );
        assert_eq!(f[0].severity(), Severity::Error);
    }
    // Beside `published: false`, both are reported (an existing page keeps
    // `1` as published).
    assert_eq!(
        wikijs_findings("---\ntitle: T\npublished: false\nisPublished: 1\n---\n"),
        [
            FrontMatterFinding::PublishedIgnoredByWikijs { line: 3 },
            FrontMatterFinding::IsPublishedBreaksWikijsImport { line: 4 },
        ]
    );
}
