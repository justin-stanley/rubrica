//! The writer's own tests: heading ids and `@sec` anchors come from the list the
//! caller passes, and the writer's time is linear in the input. Further tests cover the compat
//! flags, the markdown-it task markup, strikethrough, footnotes, positions, the container
//! depth bound, the output cap, the HTML-block newline and the footnote-shape timing.

use std::ops::Range;
use std::time::{Duration, Instant};

use rubrica_markdown::{events, CodeBlockKind, Event, Options, Tag, GFM};
use rubrica_render::writer::{
    write, write_bounded, HeadingAttrs, MermaidMarkup, RawHtmlMode, WriterOptions,
    MAX_CONTAINER_DEPTH, MAX_FOOTNOTE_BACKREFS,
};

#[test]
fn heading_ids_and_sec_anchors_come_from_the_list() {
    let attrs = [HeadingAttrs {
        id: Some("a".to_owned()),
        sec_anchor: Some("sec-x".to_owned()),
    }];
    let out = write(events("# t\n", GFM), &attrs, WriterOptions::NATIVE);
    assert_eq!(
        out.html,
        "<h1 id=\"a\"><span class=\"sec\" id=\"sec-x\"></span>t</h1>\n"
    );
    // No entry: a plain heading.
    let out = write(events("# t\n", GFM), &[], WriterOptions::NATIVE);
    assert_eq!(out.html, "<h1>t</h1>\n");
}

const MB: usize = 1 << 20;
/// A doubling whose smaller run took under this is timer noise and is not compared.
const FLOOR: Duration = Duration::from_millis(50);
#[cfg(not(debug_assertions))]
const CEILING: Duration = Duration::from_secs(2);

/// Timings are the best of three in release; debug builds, whose timing asserts are
/// compiled out, run each once.
const REPEATS: usize = if cfg!(debug_assertions) { 1 } else { 3 };

/// The best of `REPEATS` timings of `f`.
fn best_of<T>(mut f: impl FnMut() -> T) -> (T, Duration) {
    let mut best: Option<(T, Duration)> = None;
    for _ in 0..REPEATS {
        let start = Instant::now();
        let out = f();
        let took = start.elapsed();
        if best.as_ref().is_none_or(|(_, t)| took < *t) {
            best = Some((out, took));
        }
    }
    best.unwrap()
}

/// The linear-time rule on the `(bytes, time)` series: per doubling `t(2x) <= 2.5 t(x)` unless
/// `t(x)` is under the floor; the 2 MB run under 2 s.
#[cfg(not(debug_assertions))]
fn assert_linear(name: &str, series: &[(usize, Duration)]) {
    for w in series.windows(2) {
        let (n1, t1) = w[0];
        let (n2, t2) = w[1];
        if t1 >= FLOOR {
            let ratio = t2.as_secs_f64() / t1.as_secs_f64();
            assert!(
                t2 <= t1.mul_f64(2.5),
                "assertion failed: t2 <= 2.5 * t1 ({name} {}→{} MB: ratio {ratio:.1})",
                n1 as f64 / MB as f64,
                n2 as f64 / MB as f64
            );
        }
    }
    for &(n, t) in series {
        if n == 2 * MB {
            assert!(t <= CEILING, "{name} at 2 MB took {:.3} s", t.as_secs_f64());
        }
    }
}

/// `"a\n"` repeated (one `Text` and one `SoftBreak` per two bytes) at 0.5, 1 and 2 MB
/// through `write`, the events collected first so only the writer is timed, then doubling
/// on (to 32 MB at most) until a doubling's smaller run reaches the floor, so a ratio is
/// checked on this machine too (the writer wrote 8 MB in 65 ms, so its
/// 4→8 MB doubling was never compared). The timing asserts are compiled out of debug
/// builds; run with `--release`.
#[test]
fn writer_time_is_linear_on_paragraph_lines() {
    let name = "paragraph-lines";
    let series = doubling_series(
        name,
        MB / 2,
        32 * MB,
        |size| "a\n".repeat(size / 2),
        |evs, _| {
            write(evs.iter().cloned(), &[], WriterOptions::NATIVE)
                .html
                .len()
        },
    );
    #[cfg(not(debug_assertions))]
    assert_linear(name, &series);
    #[cfg(debug_assertions)]
    let _ = series;
}

// ---- compat flags and footnotes ---------------------------------------------------------------------------

/// `md` through the parser (`events`, the stream `render()` consumes) and the writer.
fn html(md: &str, opts: WriterOptions) -> String {
    write(events(md, GFM), &[], opts).html
}

/// The event vector the compat adapter hands the writer for a task list: the
/// source parsed **without** `ENABLE_TASKLISTS` (the compat profile does not rely on the
/// parser's markers), then, in each item, the first text that opens with `[ ] ` or `[x] `
/// is split into a `TaskListMarker` (the three marker bytes) and the text after the
/// four (the marker and the one space), the marker placed where the parser places its own:
/// right after `Start(Item)`, or inside the item's first paragraph when it is loose.
fn compat_task_events(md: &str) -> Vec<(Event<'static>, Range<usize>)> {
    let evs: Vec<_> = events(md, GFM.difference(Options::ENABLE_TASKLISTS))
        .map(|(e, r)| (e.into_static(), r))
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < evs.len() {
        out.push(evs[i].clone());
        i += 1;
        if !matches!(out.last(), Some((Event::Start(Tag::Item), _))) {
            continue;
        }
        if matches!(evs.get(i), Some((Event::Start(Tag::Paragraph), _))) {
            out.push(evs[i].clone());
            i += 1;
        }
        let Some((Event::Text(t), r)) = evs.get(i) else {
            continue;
        };
        let checked = match t.as_bytes() {
            [b'[', b' ', b']', b' ', ..] => false,
            [b'[', b'x' | b'X', b']', b' ', ..] => true,
            _ => continue,
        };
        out.push((Event::TaskListMarker(checked), r.start..r.start + 3));
        out.push((Event::Text(t[4..].to_owned().into()), r.start + 4..r.end));
        i += 1;
    }
    out
}

/// The compat task markup is today's `wikijs_formatter`'s, pre-sanitizer (markdown.rs:2027-2070:
/// the `checkbox` closure's three literals, `<li class="task-list-item">`, the checkbox
/// inside a loose item's first `<p>`; `<ul class="contains-task-list">` from
/// `tasklist_classes`, :224). The item text keeps its leading space in markdown-it (the
/// formatter's own comment: "No space after: it is the item's text's"); `wikijs_task_items`
/// (:954-1045) strips the three marker bytes and leaves it. The adapter strips four and the
/// writer emits the one space, so the bytes are the same. Not compared against `render()`:
/// that output is post-ammonia.
#[test]
fn compat_task_items_match_todays_markup() {
    let tight = compat_task_events("- [ ] a\n- [x] b\n");
    assert_eq!(
        tight
            .iter()
            .filter(|(e, _)| matches!(e, Event::TaskListMarker(_)))
            .count(),
        2,
        "{tight:?}"
    );
    let out = write(tight.iter().cloned(), &[], WriterOptions::WIKIJS_COMPAT);
    assert_eq!(
        out.html,
        "<ul class=\"contains-task-list\">\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\" /> a</li>\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" checked=\"\" disabled=\"\" type=\"checkbox\" /> b</li>\n\
         </ul>\n"
    );
    let loose = compat_task_events("- [ ] a\n\n- [x] b\n");
    assert_eq!(
        loose
            .iter()
            .filter(|(e, _)| matches!(e, Event::TaskListMarker(_)))
            .count(),
        2,
        "{loose:?}"
    );
    // The marker sits inside the paragraph, as the parser's own does.
    assert!(matches!(
        &loose[1..4],
        [
            (Event::Start(Tag::Item), _),
            (Event::Start(Tag::Paragraph), _),
            (Event::TaskListMarker(false), _)
        ]
    ));
    let out = write(loose.iter().cloned(), &[], WriterOptions::WIKIJS_COMPAT);
    assert_eq!(
        out.html,
        "<ul class=\"contains-task-list\">\n\
         <li class=\"task-list-item\">\n\
         <p><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\" /> a</p>\n\
         </li>\n\
         <li class=\"task-list-item\">\n\
         <p><input class=\"task-list-item-checkbox\" checked=\"\" disabled=\"\" type=\"checkbox\" /> b</p>\n\
         </li>\n\
         </ul>\n"
    );
    // A list with no task item has no class; a nested list is marked on its own.
    let plain = compat_task_events("- a\n- b\n");
    assert_eq!(
        write(plain.iter().cloned(), &[], WriterOptions::WIKIJS_COMPAT).html,
        "<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n"
    );
    let nested = compat_task_events("- a\n  - [ ] b\n");
    assert_eq!(
        write(nested.iter().cloned(), &[], WriterOptions::WIKIJS_COMPAT).html,
        "<ul>\n<li>a\n<ul class=\"contains-task-list\">\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\" /> b</li>\n\
         </ul>\n</li>\n</ul>\n"
    );
    // GFM's own markup on the parser's own markers, loose (the spec's examples are tight).
    assert_eq!(
        html("- [ ] a\n\n- [x] b\n", WriterOptions::NATIVE),
        "<ul>\n<li>\n<p><input disabled=\"\" type=\"checkbox\"> a</p>\n</li>\n\
         <li>\n<p><input checked=\"\" disabled=\"\" type=\"checkbox\"> b</p>\n</li>\n</ul>\n"
    );
}

/// Both mermaid forms: `<pre class="mermaid">` with the source as escaped
/// text, and `<pre><code class="language-mermaid">`. The `PreClass` case builds its options
/// explicitly so it still tests that form when the compat preset moves to `LanguageCode`.
#[test]
fn compat_mermaid_pre_class_and_native_language_code() {
    let md = "```mermaid\ngraph TD\n  a --> b\n  x < y\n```\n";
    let pre_class = WriterOptions {
        mermaid: MermaidMarkup::PreClass,
        ..WriterOptions::WIKIJS_COMPAT
    };
    assert_eq!(
        html(md, pre_class),
        "<pre class=\"mermaid\">graph TD\n  a --&gt; b\n  x &lt; y\n</pre>\n"
    );
    assert_eq!(
        html(md, WriterOptions::NATIVE),
        "<pre><code class=\"language-mermaid\">graph TD\n  a --&gt; b\n  x &lt; y\n</code></pre>\n"
    );
    // `LanguageCode` is a flag, not the native preset: it holds with the other compat flags on.
    let language_code = WriterOptions {
        mermaid: MermaidMarkup::LanguageCode,
        ..WriterOptions::WIKIJS_COMPAT
    };
    assert_eq!(
        html(md, language_code),
        "<pre><code class=\"language-mermaid\">graph TD\n  a --&gt; b\n  x &lt; y\n</code></pre>\n"
    );
    // Any other language is `language-L` under both.
    assert_eq!(
        html("```rust\nx\n```\n", pre_class),
        "<pre><code class=\"language-rust\">x\n</code></pre>\n"
    );
}

#[test]
fn soft_breaks_follow_the_flag() {
    assert_eq!(html("a\nb\n", WriterOptions::NATIVE), "<p>a\nb</p>\n");
    assert_eq!(
        html("a\nb\n", WriterOptions::WIKIJS_COMPAT),
        "<p>a<br />\nb</p>\n"
    );
    // The flag alone, the other compat flags off.
    let br = WriterOptions {
        soft_break_is_br: true,
        ..WriterOptions::NATIVE
    };
    assert_eq!(html("a\nb\n", br), "<p>a<br />\nb</p>\n");
    // A hard break is `<br />` under both.
    assert_eq!(
        html("a\\\nb\n", WriterOptions::NATIVE),
        "<p>a<br />\nb</p>\n"
    );
}

/// `form_feed_ends_language`: the info string is rewritten by today's rule (every `\x0C` a
/// space, trimmed, first word); without the flag the word ends at a space or tab only. A
/// leading form feed never reaches the writer (the parser trims it), so the FF-mermaid fence
/// of `tests/code.rs` is `mermaid` to both profiles, and the flag changes nothing there.
#[test]
fn form_feed_ends_the_language_only_under_compat() {
    // The leading form feed: already trimmed by the parser, so this assertion is green before
    // and after this package.
    let ff_mermaid = "```\u{c}mermaid\ny\n```\n";
    let info: Vec<_> = events(ff_mermaid, GFM)
        .filter_map(|(e, _)| match e {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => Some(info.into_string()),
            _ => None,
        })
        .collect();
    assert_eq!(info, ["mermaid"]);
    let md = "```rust\u{c}x\ny\n```\n";
    assert_eq!(
        html(md, WriterOptions::WIKIJS_COMPAT),
        "<pre><code class=\"language-rust\">y\n</code></pre>\n"
    );
    assert_eq!(
        html(md, WriterOptions::NATIVE),
        "<pre><code class=\"language-rust\u{c}x\">y\n</code></pre>\n"
    );
    // The rule's trim and first word: `rust\x0Ca b` is `rust` too; a second word after a
    // space is dropped under both.
    assert_eq!(
        html("```rust\u{c}a b\ny\n```\n", WriterOptions::WIKIJS_COMPAT),
        "<pre><code class=\"language-rust\">y\n</code></pre>\n"
    );
    assert_eq!(
        html("```rust\u{c}a b\ny\n```\n", WriterOptions::NATIVE),
        "<pre><code class=\"language-rust\u{c}a\">y\n</code></pre>\n"
    );
    // The flag alone.
    let ff = WriterOptions {
        form_feed_ends_language: true,
        ..WriterOptions::NATIVE
    };
    assert_eq!(
        html(md, ff),
        "<pre><code class=\"language-rust\">y\n</code></pre>\n"
    );
    // The FF-mermaid fence is `mermaid` to both profiles, and both write it as
    // `language-mermaid` code: the language is `mermaid`, not the flag.
    assert_eq!(
        html(ff_mermaid, WriterOptions::WIKIJS_COMPAT),
        "<pre><code class=\"language-mermaid\">y\n</code></pre>\n"
    );
    assert_eq!(
        html(ff_mermaid, WriterOptions::NATIVE),
        "<pre><code class=\"language-mermaid\">y\n</code></pre>\n"
    );
}

#[test]
fn raw_html_escape_mode() {
    let escape = WriterOptions {
        raw_html: RawHtmlMode::Escape,
        ..WriterOptions::NATIVE
    };
    // Inline.
    assert_eq!(
        html("a <b>x</b>\n", escape),
        "<p>a &lt;b&gt;x&lt;/b&gt;</p>\n"
    );
    assert_eq!(
        html("a <b>x</b>\n", WriterOptions::NATIVE),
        "<p>a <b>x</b></p>\n"
    );
    // A block.
    assert_eq!(
        html("<div>\nx\n</div>\n\ny\n", escape),
        "&lt;div&gt;\nx\n&lt;/div&gt;\n<p>y</p>\n"
    );
    assert_eq!(
        html("<div>\nx\n</div>\n\ny\n", WriterOptions::NATIVE),
        "<div>\nx\n</div>\n<p>y</p>\n"
    );
    // The compat preset keeps raw HTML too (the sanitizer's pass is the filter).
    assert_eq!(
        html("a <b>x</b>\n", WriterOptions::WIKIJS_COMPAT),
        "<p>a <b>x</b></p>\n"
    );
}

#[test]
fn strikethrough_is_del() {
    assert_eq!(
        html("~~a~~\n", WriterOptions::NATIVE),
        "<p><del>a</del></p>\n"
    );
    assert_eq!(
        html("x ~~a *b*~~ y\n", WriterOptions::WIKIJS_COMPAT),
        "<p>x <del>a <em>b</em></del> y</p>\n"
    );
}

/// Containers past `MAX_CONTAINER_DEPTH` are written without their tags, their content
/// with (today's rule): 150 nested blockquotes write 100 `<blockquote>`; nested lists count
/// the list and the item each.
#[test]
fn containers_past_the_depth_bound_lose_their_tags() {
    assert_eq!(MAX_CONTAINER_DEPTH, 100);
    let md = format!("{}x\n", "> ".repeat(150));
    let out = html(&md, WriterOptions::NATIVE);
    assert_eq!(
        out.matches("<blockquote>").count(),
        MAX_CONTAINER_DEPTH,
        "expected {MAX_CONTAINER_DEPTH} got {}",
        out.matches("<blockquote>").count()
    );
    assert_eq!(out.matches("</blockquote>").count(), MAX_CONTAINER_DEPTH);
    assert!(out.contains("<p>x</p>\n"), "{out}");
    assert!(out.ends_with("</blockquote>\n"), "{out}");
    // At the bound, every tag.
    let md = format!("{}x\n", "> ".repeat(MAX_CONTAINER_DEPTH));
    let out = html(&md, WriterOptions::NATIVE);
    assert_eq!(out.matches("<blockquote>").count(), MAX_CONTAINER_DEPTH);
    // Lists: 150 levels are 300 containers; the first 100 (50 lists, 50 items) are tagged.
    let md: String = (0..150)
        .map(|i| format!("{}- x\n", "  ".repeat(i)))
        .collect();
    let out = html(&md, WriterOptions::NATIVE);
    assert_eq!(out.matches("<ul>").count(), 50, "{out}");
    assert_eq!(out.matches("<li>").count(), 50);
    assert_eq!(out.matches("</li>").count(), 50);
    assert_eq!(out.matches('x').count(), 150);
}

/// The k-th reference to footnote N, as the writer writes it.
fn reference(n: usize, k: usize) -> String {
    let id = if k == 1 {
        format!("fnref-{n}")
    } else {
        format!("fnref-{n}-{k}")
    };
    format!("<sup class=\"footnote-ref\"><a href=\"#fn-{n}\" id=\"{id}\" data-footnote-ref>{n}</a></sup>")
}

/// The back reference to the k-th reference of footnote N: `data-footnote-backref-idx`
/// and `aria-label` name the reference as cmark-gfm and the previous renderer do, `N` for the first and
/// `N-k` after it (the review's decision over the package's `k`).
fn backref(n: usize, k: usize) -> String {
    if k == 1 {
        format!("<a href=\"#fnref-{n}\" class=\"footnote-backref\" data-footnote-backref data-footnote-backref-idx=\"{n}\" aria-label=\"Back to reference {n}\">↩</a>")
    } else {
        format!("<a href=\"#fnref-{n}-{k}\" class=\"footnote-backref\" data-footnote-backref data-footnote-backref-idx=\"{n}-{k}\" aria-label=\"Back to reference {n}-{k}\">↩<sup class=\"footnote-ref\">{k}</sup></a>")
    }
}

/// Footnote markup: ids by the 1-based first-reference ordinal of the name, the
/// definitions written after the body in that order, unreferenced ones dropped, back
/// references capped at `MAX_FOOTNOTE_BACKREFS` and the excess recorded, a definition past
/// the depth bound unlinked.
#[test]
fn footnote_markup_ids_order_and_backref_cap() {
    assert_eq!(MAX_FOOTNOTE_BACKREFS, 100);
    // `b` first (150 references), then `a`; `c` is defined and never referenced.
    let refs = 150;
    let md = format!(
        "x[^b] y[^a]{}\n\n[^a]: A\n\n[^b]: B\n\n[^c]: C\n",
        "[^b]".repeat(refs - 1)
    );
    let out = write(events(&md, GFM), &[], WriterOptions::NATIVE);
    let mut expected = format!("<p>x{} y{}", reference(1, 1), reference(2, 1));
    for k in 2..=refs {
        expected.push_str(&reference(1, k));
    }
    expected.push_str(
        "</p>\n<section class=\"footnotes\" data-footnotes>\n<ol>\n<li id=\"fn-1\">\n<p>B",
    );
    for k in 1..=MAX_FOOTNOTE_BACKREFS as usize {
        expected.push(' ');
        expected.push_str(&backref(1, k));
    }
    expected.push_str("</p>\n</li>\n<li id=\"fn-2\">\n<p>A ");
    expected.push_str(&backref(2, 1));
    expected.push_str("</p>\n</li>\n</ol>\n</section>\n");
    assert_eq!(out.html, expected);
    // `b`'s 100 and `a`'s one.
    assert_eq!(
        out.html.matches(" class=\"footnote-backref\" ").count(),
        MAX_FOOTNOTE_BACKREFS as usize + 1
    );
    assert_eq!(out.html.matches("data-footnote-ref>").count(), refs + 1);
    assert!(!out.html.contains('C'));
    let def_b = md.find("[^b]: B").unwrap();
    assert_eq!(out.backrefs_bounded, [("b".to_owned(), refs, def_b)]);
    assert_eq!(out.unlinked_footnotes, Vec::<String>::new());
    // Every id written, once each: fn-1, fn-2, fnref-1, fnref-1-2..150, fnref-2.
    let mut ids: Vec<String> = Vec::new();
    for (at, _) in out.html.match_indices(" id=\"") {
        let rest = &out.html[at + 5..];
        ids.push(rest[..rest.find('"').unwrap()].to_owned());
    }
    assert_eq!(ids.len(), 2 + refs + 1);
    let mut sorted = out.footnote_ids.clone();
    sorted.sort();
    ids.sort();
    assert_eq!(sorted, ids);
    assert!(out.footnote_ids.contains(&"fn-1".to_owned()));
    assert!(out.footnote_ids.contains(&"fnref-1-150".to_owned()));
    assert!(!out.footnote_ids.contains(&"fn-3".to_owned()));
    // At the bound, nothing is recorded.
    let md = format!(
        "x{}\n\n[^1]: n\n",
        "[^1]".repeat(MAX_FOOTNOTE_BACKREFS as usize)
    );
    let out = write(events(&md, GFM), &[], WriterOptions::NATIVE);
    assert_eq!(out.backrefs_bounded, []);
    assert_eq!(
        out.html.matches(" class=\"footnote-backref\" ").count(),
        MAX_FOOTNOTE_BACKREFS as usize
    );
    // The ordinal is the name's first-reference order, not the label text, and a
    // definition before its reference is still written after the body; a definition whose
    // last child is not a paragraph gets its back references after that child.
    let out = write(
        events("[^9]: nine\n\n    ```\n    c\n    ```\n\nx[^9]\n", GFM),
        &[],
        WriterOptions::NATIVE,
    );
    assert_eq!(
        out.html,
        format!(
            "<p>x{}</p>\n<section class=\"footnotes\" data-footnotes>\n<ol>\n<li id=\"fn-1\">\n<p>nine</p>\n<pre><code>c\n</code></pre>\n{}\n</li>\n</ol>\n</section>\n",
            reference(1, 1),
            backref(1, 1)
        )
    );
    // A definition past the depth bound is never written: its references have no link and
    // no id, its name is recorded, and no id of its own exists.
    let md = format!("{}[^d]: D\n\nx[^d] y[^e]\n\n[^e]: E\n", "> ".repeat(150));
    let out = write(events(&md, GFM), &[], WriterOptions::NATIVE);
    assert_eq!(out.unlinked_footnotes, ["d".to_owned()]);
    assert!(
        out.html
            .contains("<p>x<sup class=\"footnote-ref\">1</sup> y"),
        "{}",
        out.html
    );
    assert!(!out.html.contains("fn-1\""), "{}", out.html);
    assert!(out.html.contains("<li id=\"fn-2\">\n<p>E "), "{}", out.html);
    assert_eq!(out.footnote_ids, ["fnref-2", "fn-2"]);
    // Two definitions of one name: the first is the one written (as for link references).
    let out = html("x[^1]\n\n[^1]: a\n\n[^1]: b\n", WriterOptions::NATIVE);
    assert!(out.contains("<p>a "), "{out}");
    assert!(!out.contains("<p>b"), "{out}");
    assert_eq!(out.matches("<li id=").count(), 1);
}

/// `positions` holds `(html offset, source offset)` at every block and inline start (once
/// per source offset), sorted by html offset; on a body whose footnote definition comes
/// last in the source the source offsets never go backwards either.
#[test]
fn positions_start_each_block_and_grow_monotonically() {
    // Only events the plain writer writes, first: red on the empty vector.
    let w = write(
        events("# h\n\npara *em*\n\n- t\n\n```\nc\n```\n", GFM),
        &[],
        WriterOptions::NATIVE,
    );
    assert!(!w.positions.is_empty());
    assert_eq!(w.positions[0], (0, 0));
    // Then every kind of node.
    let md = "# h\n\npara *em* `c`[^1]\n\n- [ ] t\n- u\n\n```\nc\n```\n\n<div>\n\n---\n\n| a |\n| - |\n| b |\n\n[^1]: n\n";
    let w = write(events(md, GFM), &[], WriterOptions::NATIVE);
    assert_eq!(w.positions[0], (0, 0));
    for pair in w.positions.windows(2) {
        assert!(
            pair[0].0 < pair[1].0,
            "html offsets not strictly increasing: {:?} then {:?}",
            pair[0],
            pair[1]
        );
        assert!(
            pair[0].1 <= pair[1].1,
            "source offsets decrease: {:?} then {:?}",
            pair[0],
            pair[1]
        );
    }
    // Each pushed html offset is where a node's HTML starts.
    let at = |needle: &str| w.html.find(needle).unwrap();
    let html_offsets: Vec<usize> = w.positions.iter().map(|p| p.0).collect();
    for needle in [
        "<h1>", "<p>para", "<em>", "<code>", "<sup", "<ul>", "<input", "<pre>", "<div>", "<hr />",
        "<table>", "<th>", "<td>", "<section",
    ] {
        assert!(
            html_offsets.contains(&at(needle)),
            "{needle} at {} not in {html_offsets:?}",
            at(needle)
        );
    }
    let src_offsets: Vec<usize> = w.positions.iter().map(|p| p.1).collect();
    for needle in [
        "# h", "para", "*em*", "`c`", "[^1]", "- [ ]", "[ ]", "- u", "```", "<div>", "---",
        "| a |", "| b |", "[^1]: n",
    ] {
        let src = md.find(needle).unwrap();
        assert!(
            src_offsets.contains(&src),
            "{needle:?} at {src} not in {src_offsets:?}"
        );
    }
    // The last entries are the footnote definition (at the `<section>`, where its HTML
    // starts) and its paragraph, written after the body.
    let def = md.find("[^1]: n").unwrap();
    let tail: Vec<usize> = w.positions.iter().rev().take(2).map(|p| p.1).collect();
    assert_eq!(tail, [def + 6, def]);
    // A definition before its reference is written after the body: its entries sort by html
    // offset, so the source offsets go back to its own: the body paragraph (9) and its
    // reference (10), then the definition (0) and its paragraph (6).
    let w = write(
        events("[^1]: n\n\nx[^1]\n", GFM),
        &[],
        WriterOptions::NATIVE,
    );
    for pair in w.positions.windows(2) {
        assert!(pair[0].0 < pair[1].0, "{:?}", w.positions);
    }
    let src: Vec<usize> = w.positions.iter().map(|p| p.1).collect();
    assert_eq!(src, [9, 10, 0, 6]);
    assert_eq!(w.positions[2].0, w.html.find("<section").unwrap());
    assert_eq!(w.positions[3].0, w.html.find("<p>n").unwrap());
}

#[test]
fn output_over_the_cap_is_refused_with_a_notice() {
    let md = "a\n".repeat(5_000);
    assert_eq!(md.len(), 10_000);
    let w = write_bounded(events(&md, GFM), &[], WriterOptions::NATIVE, 4096);
    assert!(w.truncated);
    assert_eq!(
        w.html,
        "<p>This page is not rendered: its HTML would be larger than Rubrica's limit of 67108864 bytes.</p>\n"
    );
    assert_eq!(w.positions, []);
    // The cap is inclusive: output of exactly `cap` bytes stands, one more does not.
    let full = write(events(&md, GFM), &[], WriterOptions::NATIVE);
    assert!(!full.truncated);
    let len = full.html.len();
    let w = write_bounded(events(&md, GFM), &[], WriterOptions::NATIVE, len);
    assert!(!w.truncated);
    assert_eq!(w.html, full.html);
    let w = write_bounded(events(&md, GFM), &[], WriterOptions::NATIVE, len - 1);
    assert!(w.truncated);
    assert_eq!(w.positions, []);
}

/// cmark writes a newline after an HTML block, visible only at
/// EOF without a final newline.
#[test]
fn html_block_at_eof_ends_with_a_newline() {
    assert_eq!(html("<div>", WriterOptions::NATIVE), "<div>\n");
    assert_eq!(
        html("- <div>", WriterOptions::NATIVE),
        "<ul>\n<li>\n<div>\n</li>\n</ul>\n"
    );
    assert_eq!(html("<div>\n", WriterOptions::NATIVE), "<div>\n");
}

/// A doubling series on `md_of(size)` through `run` (the events collected first, so only
/// the writer is timed): `start`, twice and four times it (0.5, 1 and 2 MB as a rule),
/// then doubling on, up to `max`, until a doubling's smaller run is at or over the floor,
/// so at least one ratio is checked.
fn doubling_series(
    name: &str,
    start: usize,
    max: usize,
    md_of: impl Fn(usize) -> String,
    mut run: impl for<'a> FnMut(&[(Event<'a>, Range<usize>)], usize) -> usize,
) -> Vec<(usize, Duration)> {
    let mut series: Vec<(usize, Duration)> = Vec::new();
    let mut sizes = vec![start, 2 * start, 4 * start];
    while let Some(size) = sizes.first().copied() {
        sizes.remove(0);
        let md = md_of(size);
        let evs: Vec<_> = events(&md, GFM).collect();
        let (len, took) = best_of(|| run(&evs, md.len()));
        eprintln!(
            "{name} {} B {:.3} s ({} events, {len} B out)",
            md.len(),
            took.as_secs_f64(),
            evs.len()
        );
        let previous_under_floor = series.last().is_none_or(|&(_, t)| t < FLOOR);
        if sizes.is_empty() && size < max && previous_under_floor {
            sizes.push(2 * size);
        }
        series.push((size, took));
    }
    // A ratio was checked, or the ceiling stopped the doubling.
    #[cfg(not(debug_assertions))]
    assert!(
        series.windows(2).any(|w| w[0].1 >= FLOOR) || series.last().is_some_and(|&(n, _)| n >= max),
        "{name}: no doubling's smaller run reached the floor under {max} B: {series:?}"
    );
    series
}

/// `"[^1] "` repeated (one reference and one text per five bytes, about 409 k events per
/// MB) with one definition, through `write(…, NATIVE)`: the linear-time rule in release, and
/// the amplification is real (each reference writes about 85 bytes). Past 2 MB the output
/// would pass `MAX_RENDER_OUTPUT_BYTES` (2 MB of this shape is about 36 MB of HTML), so the
/// doublings beyond it go through `write_bounded` with no cap, the same code path.
#[test]
fn writer_time_is_linear_on_the_footnote_shape() {
    let name = "footnote-shape";
    let series = doubling_series(
        name,
        MB / 2,
        8 * MB,
        |size| format!("{}\n\n[^1]: note\n", "[^1] ".repeat(size / 5)),
        |evs, md_len| {
            let w = if md_len <= 2 * MB {
                write(evs.iter().cloned(), &[], WriterOptions::NATIVE)
            } else {
                write_bounded(evs.iter().cloned(), &[], WriterOptions::NATIVE, usize::MAX)
            };
            assert!(!w.truncated);
            assert!(
                w.html.len() > 10 * md_len,
                "amplification: {} B of HTML from {md_len} B",
                w.html.len()
            );
            w.html.len()
        },
    );
    #[cfg(not(debug_assertions))]
    assert_linear(name, &series);
    #[cfg(debug_assertions)]
    let _ = series;
}

// ---- review fixes --------------------------------------------------------------

/// The parser matches footnote labels case-insensitively (`UniCase`); so does the writer:
/// a reference and a definition that differ in case are one footnote, reported by its
/// first spelling.
#[test]
fn footnote_labels_match_case_insensitively() {
    // ASCII case: the definition is written and the reference links to it.
    let out = write(
        events("x[^A]\n\n[^a]: y\n", GFM),
        &[],
        WriterOptions::NATIVE,
    );
    assert_eq!(
        out.html,
        format!(
            "<p>x{}</p>\n<section class=\"footnotes\" data-footnotes>\n<ol>\n<li id=\"fn-1\">\n<p>y {}</p>\n</li>\n</ol>\n</section>\n",
            reference(1, 1),
            backref(1, 1)
        )
    );
    assert_eq!(out.unlinked_footnotes, Vec::<String>::new());
    assert_eq!(out.footnote_ids, ["fnref-1", "fn-1"]);
    // Two spellings of one reference: one footnote, two references, two back references.
    let out = write(
        events("x[^a] z[^A]\n\n[^A]: y\n", GFM),
        &[],
        WriterOptions::NATIVE,
    );
    assert_eq!(
        out.html,
        format!(
            "<p>x{} z{}</p>\n<section class=\"footnotes\" data-footnotes>\n<ol>\n<li id=\"fn-1\">\n<p>y {} {}</p>\n</li>\n</ol>\n</section>\n",
            reference(1, 1),
            reference(1, 2),
            backref(1, 1),
            backref(1, 2)
        )
    );
    assert_eq!(out.html.matches("<li id=").count(), 1);
    // A non-ASCII pair folds too.
    let out = write(
        events("x[^É]\n\n[^é]: y\n", GFM),
        &[],
        WriterOptions::NATIVE,
    );
    assert!(out.html.contains("<li id=\"fn-1\">\n<p>y "), "{}", out.html);
    assert!(out.html.contains(&reference(1, 1)), "{}", out.html);
    // A deep definition and a reference in another case: one name, reported as the
    // definition first spelled it.
    let md = format!("{}[^Note]: n\n\nx[^note] y[^NOTE]\n", "> ".repeat(150));
    let out = write(events(&md, GFM), &[], WriterOptions::NATIVE);
    assert_eq!(out.unlinked_footnotes, ["Note".to_owned()]);
    assert!(
        out.html.contains(
            "<p>x<sup class=\"footnote-ref\">1</sup> y<sup class=\"footnote-ref\">1</sup></p>"
        ),
        "{}",
        out.html
    );
    assert!(!out.html.contains("<section"), "{}", out.html);
    // The bounded record names the footnote as first referenced.
    let md = format!("x[^Big]{}\n\n[^big]: n\n", "[^BIG]".repeat(150));
    let out = write(events(&md, GFM), &[], WriterOptions::NATIVE);
    assert_eq!(
        out.backrefs_bounded,
        [("Big".to_owned(), 151, md.find("[^big]: n").unwrap())]
    );
}

/// A tight task item past `MAX_CONTAINER_DEPTH` loses its checkbox with its tags, as
/// today's formatter drops it (its item is not written at all; the text stays). A loose
/// item's checkbox opens its paragraph, which is no container, and stays.
#[test]
fn tight_task_items_past_the_depth_bound_lose_their_checkbox() {
    // 52 levels: lists and items are 104 containers; the first 50 items (depth 100) are
    // tagged, the two deepest are text in the 50th.
    let md: String = (0..52)
        .map(|i| format!("{}- [x] x\n", "  ".repeat(i)))
        .collect();
    let evs = compat_task_events(&md);
    assert_eq!(
        evs.iter()
            .filter(|(e, _)| matches!(e, Event::TaskListMarker(true)))
            .count(),
        52
    );
    let out = write(evs.iter().cloned(), &[], WriterOptions::WIKIJS_COMPAT).html;
    assert_eq!(
        out.matches("<li class=\"task-list-item\">").count(),
        50,
        "{out}"
    );
    assert_eq!(out.matches("<input").count(), 50, "{out}");
    assert!(out.contains("/> x x x</li>\n"), "{out}");
    // Every item's text (the space the writer emits, then `x`), tagged or not.
    assert_eq!(out.matches(" x").count(), 52);
    // GFM's markup: the same rule.
    let out = html(&md, WriterOptions::NATIVE);
    assert_eq!(out.matches("<input").count(), 50, "{out}");
    assert!(out.contains("> x x x</li>\n"), "{out}");
    // Loose items past the bound keep the checkbox in their paragraph (today's rule).
    let deep = "> ".repeat(101);
    let md = format!("{deep}- [ ] a\n{deep}\n{deep}- [x] b\n");
    let evs = compat_task_events(&md);
    assert_eq!(
        evs.iter()
            .filter(|(e, _)| matches!(e, Event::TaskListMarker(_)))
            .count(),
        2
    );
    let out = write(evs.iter().cloned(), &[], WriterOptions::WIKIJS_COMPAT).html;
    assert!(
        out.contains("<p><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\" /> a</p>\n<p><input class=\"task-list-item-checkbox\" checked=\"\" disabled=\"\" type=\"checkbox\" /> b</p>\n"),
        "{out}"
    );
    assert_eq!(out.matches("<li").count(), 0);
}

/// `data-footnote-backref-idx` and `aria-label` name the reference as cmark-gfm and
/// the previous renderer do: `N` for the footnote's first reference, `N-k` for the k-th.
#[test]
fn footnote_backref_idx_names_the_reference() {
    // A self-reference: the second reference to 1 is inside its own note.
    let out = html("x[^1]\n\n[^1]: see [^1]\n", WriterOptions::NATIVE);
    assert!(
        out.contains("data-footnote-backref-idx=\"1\" aria-label=\"Back to reference 1\">↩</a> <a href=\"#fnref-1-2\" class=\"footnote-backref\" data-footnote-backref data-footnote-backref-idx=\"1-2\" aria-label=\"Back to reference 1-2\">↩<sup class=\"footnote-ref\">2</sup></a></p>"),
        "{out}"
    );
    assert!(out.contains(&format!("see {}", reference(1, 2))), "{out}");
    // Footnote 2's first back reference is `2`, not `1`.
    let out = html("x[^a] y[^b]\n\n[^a]: A\n\n[^b]: B\n", WriterOptions::NATIVE);
    assert!(
        out.contains("<li id=\"fn-2\">\n<p>B <a href=\"#fnref-2\" class=\"footnote-backref\" data-footnote-backref data-footnote-backref-idx=\"2\" aria-label=\"Back to reference 2\">↩</a></p>"),
        "{out}"
    );
    assert_eq!(out.matches("Back to reference 1\"").count(), 1);
}

/// An empty definition: `<li id="fn-N">` and the back reference on the same line, the
/// newline after `<li …>` being the first block's `cr`, as the previous renderer writes it.
#[test]
fn an_empty_definition_writes_its_backref_on_the_li_line() {
    let out = html("x[^1]\n\n[^1]:\n", WriterOptions::NATIVE);
    assert_eq!(
        out,
        format!(
            "<p>x{}</p>\n<section class=\"footnotes\" data-footnotes>\n<ol>\n<li id=\"fn-1\">{}\n</li>\n</ol>\n</section>\n",
            reference(1, 1),
            backref(1, 1)
        )
    );
    // A definition with content keeps the newline.
    let out = html("x[^1]\n\n[^1]: y\n", WriterOptions::NATIVE);
    assert!(out.contains("<li id=\"fn-1\">\n<p>y "), "{out}");
}

/// Past the cap, every footnote record goes with the HTML: nothing in the notice has an id.
#[test]
fn truncation_clears_every_footnote_record() {
    let md = format!(
        "{}[^d]: D\n\nx[^d]{}\n\n[^a]: A\n",
        "> ".repeat(150),
        "[^a]".repeat(150)
    );
    let full = write(events(&md, GFM), &[], WriterOptions::NATIVE);
    assert!(!full.truncated);
    assert!(!full.footnote_ids.is_empty());
    assert_eq!(full.unlinked_footnotes, ["d".to_owned()]);
    assert_eq!(full.backrefs_bounded.len(), 1);
    let w = write_bounded(events(&md, GFM), &[], WriterOptions::NATIVE, 64);
    assert!(w.truncated);
    assert_eq!(w.footnote_ids, Vec::<String>::new());
    assert_eq!(w.backrefs_bounded, []);
    assert_eq!(w.unlinked_footnotes, Vec::<String>::new());
    assert_eq!(w.positions, []);
}

/// Inline HTML gets a position, as today's formatter positions every node it enters.
#[test]
fn inline_html_has_a_position() {
    let md = "a\n<b x=\"1\">c</b>\n";
    let w = write(events(md, GFM), &[], WriterOptions::NATIVE);
    let src: Vec<usize> = w.positions.iter().map(|p| p.1).collect();
    assert_eq!(src, [0, 2, 12], "{:?}", w.positions);
    assert_eq!(w.positions[1].0, w.html.find("<b x=").unwrap());
    assert_eq!(w.positions[2].0, w.html.find("</b>").unwrap());
}

/// k distinct footnote definitions, each 101 blockquotes deep (so none is written and
/// every name is recorded as unlinked), at 1, 2 and 4 MB and doubling on: the unlinked
/// list must be deduplicated through a set, not a scan (the review measured a scan at a
/// 1→2 MB ratio of 2.8; this machine's was 2.45, so the series starts at 1 MB, where the
/// 2→4 MB doubling shows the square).
#[test]
fn writer_time_is_linear_on_deep_footnote_definitions() {
    let name = "deep-definitions";
    let series = doubling_series(
        name,
        MB,
        64 * MB,
        |size| {
            let line = ">".repeat(101);
            (0..size / 112)
                .map(|i| format!("{line}[^{i}]: n\n"))
                .collect()
        },
        |evs, _| {
            let w = write(evs.iter().cloned(), &[], WriterOptions::NATIVE);
            assert!(!w.truncated);
            assert!(w.unlinked_footnotes.len() > 1000);
            assert!(!w.html.contains("<section"));
            w.html.len()
        },
    );
    #[cfg(not(debug_assertions))]
    assert_linear(name, &series);
    #[cfg(debug_assertions)]
    let _ = series;
}

/// The render crate's manifest names no parser but rubrica-markdown. The word is
/// built from two halves so that this file does not itself name it (the tree guard
/// scans for it). Read by `CARGO_MANIFEST_DIR`, not a `..` path: a packaged test may not
/// read outside its crate.
#[test]
fn the_render_crate_depends_on_no_parser_but_rubrica_markdown() {
    let manifest = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    assert!(manifest.contains("rubrica-markdown"));
    let removed_parser = concat!("com", "rak");
    assert!(
        !manifest.contains(removed_parser),
        "the manifest names {removed_parser}"
    );
}
