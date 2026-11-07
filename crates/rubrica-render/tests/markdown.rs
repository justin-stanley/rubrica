//! The renderer on synthetic input: the previous renderer configured as Wiki.js's
//! markdown-it, heading text and ids, links and the other collected facts.

use rubrica_render::markdown::{classify, render_strict};
use rubrica_render::{Heading, Link, LinkKind, RawHtml, RenderOptions, Rendered, TaskItem};

/// This file pins the compat profile's behaviour (it was the default when the tests were
/// written); the native default is pinned in `tests/code.rs` from the native default's introduction on. Strict: a
/// text whose position the adapters got wrong panics here.
fn r(md: &str) -> Rendered {
    render_strict(md, &RenderOptions::wikijs_compat())
}

fn heading(md: &str) -> Heading {
    let mut hs = r(md).headings;
    assert_eq!(hs.len(), 1, "{md:?}: {hs:?}");
    hs.remove(0)
}

fn hrefs(rendered: &Rendered) -> Vec<&str> {
    rendered.links.iter().map(|l| l.href.as_str()).collect()
}

#[test]
fn soft_breaks_become_br() {
    let out = r("Two views:\n[Second](/views/second)\n");
    assert!(
        out.html
            .contains("Two views:<br>\n<a href=\"/views/second\">"),
        "{}",
        out.html
    );
    // A hard break stays a single break.
    let out = r("one  \ntwo\\\nthree\n");
    assert_eq!(out.html, "<p>one<br>\ntwo<br>\nthree</p>\n");
}

#[test]
fn strikethrough_tables_tasklists_autolinks_render() {
    let out = r(include_str!("fixtures/md/gfm.md"));
    let html = &out.html;
    assert!(html.contains("<table>"), "{html}");
    assert!(html.contains("<td><del>compute</del> edge</td>"), "{html}");
    assert_eq!(html.matches("type=\"checkbox\"").count(), 2, "{html}");
    assert_eq!(html.matches("checked=\"\"").count(), 1, "{html}");
    assert_eq!(
        hrefs(&out),
        [
            "https://example.com/path",
            "https://example.org/x",
            "http://www.example.net"
        ]
    );
    assert!(out
        .links
        .iter()
        .all(|l| l.kind == LinkKind::External && l.line == 8));
    assert_eq!(
        out.task_items,
        [
            TaskItem {
                line: 5,
                checked: false,
                section: None
            },
            TaskItem {
                line: 6,
                checked: true,
                section: None
            },
        ]
    );
}

#[test]
fn a_heading_inside_a_blockquote_gets_an_id() {
    let out = r("Intro.\n\n> ## ⚠️ Open item\n> Still open.\n");
    assert_eq!(
        out.headings,
        [Heading {
            level: 2,
            text: "⚠️ Open item".into(),
            id: Some("\u{FE0F}-open-item".into()),
            line: 3,
            sec: None,
        }]
    );
    assert!(
        out.html
            .contains("<blockquote>\n<h2 id=\"\u{FE0F}-open-item\">⚠️ Open item</h2>"),
        "{}",
        out.html
    );
}

#[test]
fn heading_text_keeps_code_and_link_labels_and_drops_images() {
    let h = heading("## Use `gift.*` with [the *guide*](/x) ![logo](/l.png) **now** ~~not~~\n");
    assert_eq!(h.text, "Use gift.* with the guide  now not");
    assert_eq!(h.id.as_deref(), Some("use-gift-with-the-guide-now-not"));
    // Entities and backslash escapes are resolved by the parser: the text is
    // what the reader sees, and `_` inside a word is not emphasis.
    let h = heading("## Q&amp;A \\*not emphasis\\* FREE_POOL_SIZE &#x3A9;\n");
    assert_eq!(h.text, "Q&A *not emphasis* FREE_POOL_SIZE Ω");
    assert_eq!(h.id.as_deref(), Some("qa-not-emphasis-free_pool_size-ω"));
    // Inline HTML (a tag the sanitizer drops) contributes nothing, like a
    // tag in markdown-it's output; an autolink contributes its text.
    let h = heading("## a <b>b</b> <https://x.example>\n");
    assert_eq!(h.text, "a b https://x.example");
    // Trimmed: a trailing image leaves no trailing space.
    assert_eq!(heading("## Logo ![logo](/l.png)\n").text, "Logo");
}

#[test]
fn a_closing_hash_sequence_is_not_text() {
    assert_eq!(heading("## Title ##\n").text, "Title");
    assert_eq!(heading("## C# #\n").text, "C#");
    assert_eq!(heading("### foo ###b\n").text, "foo ###b");
    assert_eq!(heading("## a \\#\n").text, "a #");
}

#[test]
fn a_setext_heading_line_break_is_deleted_by_the_slugger() {
    // markdown-it renders the soft break inside a setext heading as
    // `<br>\n` (breaks: true); jQuery's `.text()` sees "\n", which uslug
    // deletes rather than treating as a space. The corpus has no setext
    // heading, so this pins the semantics the oracle cannot.
    let h = heading("foo\nbar\n===\n");
    assert_eq!((h.level, h.text.as_str()), (1, "foo\nbar"));
    assert_eq!(h.id.as_deref(), Some("foobar"));
}

#[test]
fn heading_ids_dedupe_across_the_page_and_clobbered_ids_are_omitted() {
    let out = r("# A\n\n## A\n\n> ## A\n\n- ### A\n\n## Open\n\n## 4. NAS config\n\n## a 1\n");
    let ids: Vec<_> = out.headings.iter().map(|h| h.id.as_deref()).collect();
    assert_eq!(
        ids,
        [
            Some("a"),
            Some("a-1"),
            Some("a-2"),
            Some("a-3"),
            None,
            Some("h-4-nas-config"),
            Some("a-1-1"),
        ]
    );
    assert!(out.html.contains("<h2>Open</h2>"), "{}", out.html);
    assert!(out.html.contains("<h3 id=\"a-3\">A</h3>"), "{}", out.html);
    assert_eq!(
        out.headings
            .iter()
            .map(|h| (h.level, h.line))
            .collect::<Vec<_>>(),
        [(1, 1), (2, 3), (2, 5), (3, 7), (2, 9), (2, 11), (2, 13)]
    );
    // Each render is its own page: no dedupe state leaks between them.
    assert_eq!(heading("# A\n").id.as_deref(), Some("a"));
}

#[test]
fn bare_domains_are_not_linkified() {
    // markdown-it's linkify turned `example.io`, `README.md` and
    // `FS.com` into http:// links; Rubrica does not. GFM's own autolinks
    // (www., a scheme, an email address) still work.
    let out = r("On example.io, see README.md and example-domain.com or github.com/a/b.\n");
    assert_eq!(hrefs(&out), Vec::<&str>::new());
    assert!(!out.html.contains("<a "), "{}", out.html);
    let out = r("www.example.com and someone@example.com\n");
    assert_eq!(
        hrefs(&out),
        ["http://www.example.com", "mailto:someone@example.com"]
    );
}

#[test]
fn raw_html_is_escaped_in_escape_mode() {
    // The earlier behaviour, now `RawHtml::Escape`; the default sanitizes
    // (tests/sanitize.rs).
    let r = |md: &str| {
        render_strict(
            md,
            &RenderOptions::wikijs_compat().raw_html(RawHtml::Escape),
        )
    };
    let out = r("a <b>bold</b> <script>alert(1)</script>\n\n<div>\nblock\n</div>\n");
    assert!(!out.html.contains("<script"), "{}", out.html);
    assert!(!out.html.contains("<b>"), "{}", out.html);
    assert!(!out.html.contains("<div>"), "{}", out.html);
    assert!(out.html.contains("&lt;b&gt;bold&lt;/b&gt;"), "{}", out.html);
    assert!(out.html.contains("&lt;div&gt;"), "{}", out.html);
    // Dangerous link schemes are neutralised too.
    let out = r("[x](javascript:alert(1))\n");
    assert!(!out.html.contains("javascript:"), "{}", out.html);
}

#[test]
fn links_are_classified_internal_anchor_external_relative() {
    let internal = |p: &str, f: Option<&str>| LinkKind::Internal {
        path: p.into(),
        fragment: f.map(Into::into),
    };
    assert_eq!(
        classify("/runbooks/backups"),
        internal("/runbooks/backups", None)
    );
    assert_eq!(classify("/a/b#c-d"), internal("/a/b", Some("c-d")));
    assert_eq!(classify("/a?x=1#f"), internal("/a", Some("f")));
    assert_eq!(classify("/"), internal("/", None));
    assert_eq!(classify("#open-item"), LinkKind::Anchor("open-item".into()));
    assert_eq!(classify("https://x.example/a"), LinkKind::External);
    assert_eq!(classify("mailto:a@b.example"), LinkKind::External);
    assert_eq!(classify("//cdn.example/x"), LinkKind::External);
    assert_eq!(classify("sibling"), LinkKind::Relative);
    assert_eq!(classify("../up#x"), LinkKind::Relative);
    assert_eq!(classify("finops/foo.md"), LinkKind::Relative);

    let out = r(
        "# T\n\n[a](/x) [b](#t)\n[c](https://e.example) ![i](/img.png)\n\n| [d](rel) |\n|---|\n",
    );
    assert_eq!(
        out.links,
        [
            Link {
                href: "/x".into(),
                line: 3,
                kind: internal("/x", None)
            },
            Link {
                href: "#t".into(),
                line: 3,
                kind: LinkKind::Anchor("t".into())
            },
            Link {
                href: "https://e.example".into(),
                line: 4,
                kind: LinkKind::External
            },
            Link {
                href: "rel".into(),
                line: 6,
                kind: LinkKind::Relative
            },
        ]
    );
}

#[test]
fn task_items_know_their_section_and_text_skips_mermaid() {
    let out = r("- [ ] before\n\n# T\n\nOne **two** `three`.\n\n## S\n\n- [x] done\n\n```mermaid\ngraph LR\n```\n\n```bash\necho hi\n```\n");
    assert_eq!(
        out.task_items,
        [
            TaskItem {
                line: 1,
                checked: false,
                section: None
            },
            TaskItem {
                line: 9,
                checked: true,
                section: Some(1)
            },
        ]
    );
    assert_eq!(out.mermaid_blocks, ["graph LR\n"]);
    assert!(
        out.plain_text.contains("One two three."),
        "{:?}",
        out.plain_text
    );
    assert!(out.plain_text.contains("echo hi"), "{:?}", out.plain_text);
    assert!(!out.plain_text.contains("graph"), "{:?}", out.plain_text);
    assert_eq!(out.word_count, 9, "{:?}", out.plain_text);
}

#[test]
fn a_single_tilde_is_not_strikethrough() {
    // Found against the stored renders (architecture/multi-site,
    // architecture/office-backhaul): GFM lets `~x~` strike through, but
    // markdown-it needs `~~`, and this corpus writes `~` for "about":
    // "(+ fiber office-backhaul ~$420–580, separate page) · … ~" was struck
    // through by the previous renderer and is plain text in Wiki.js.
    for md in [
        // multi-site.md:112, the shape that struck through: the closer
        // follows "(" and precedes "$", so it is right-flanking.
        "(+ fiber office-backhaul ~$420–580, separate page) · mini PC (~$500)\n",
        "x ~a~ y and (~1)~ z\n",
        "> about ~$1 and (~$2)\n",
        "- about ~$1 and (~$2)\n",
        "1. *about ~$1 and (~$2)* GbE\n",
        "| cost | range |\n|---|---|\n| ~$449 and (~$500) | ~a~ |\n",
    ] {
        let out = r(md);
        assert!(!out.html.contains("<del>"), "{md:?}: {}", out.html);
        assert_eq!(
            out.html.matches('~').count(),
            md.matches('~').count(),
            "{md:?}: every tilde is text: {}",
            out.html
        );
    }
    let out = r("~~gone~~ and ~1 ~2, ~~also gone~~\n");
    assert_eq!(
        out.html,
        "<p><del>gone</del> and ~1 ~2, <del>also gone</del></p>\n"
    );
    let h = heading("## about ~80 and ~90 lines\n");
    assert_eq!(h.text, "about ~80 and ~90 lines");
    assert_eq!(h.id.as_deref(), Some("about-~80-and-~90-lines"));
    assert!(r("x ~a~ y\n").plain_text.contains("~a~"));
    assert!(heading("## x ~a~ y\n").text.contains("~a~"));
}

/// The `owner-tasks` shape: items separated by blank lines make a loose
/// list, and markdown-it-task-lists puts the checkbox inside the item's
/// first paragraph, where the previous renderer puts it before the paragraph. Wiki.js
/// (a stored render in the private conformance corpus):
/// `<li class="task-list-item">\n<p><input class="task-list-item-checkbox"
/// disabled="" type="checkbox"> <strong>…`.
#[test]
fn a_loose_task_item_has_its_checkbox_inside_the_first_paragraph() {
    let out =
        r("- [ ] **Add a key** *(High)*\n  Your DID has one.\n\n- [x] **Done** it\n\n  More.\n");
    assert_eq!(
        out.html,
        "<ul class=\"contains-task-list\">\n\
         <li class=\"task-list-item\">\n\
         <p><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\"> \
         <strong>Add a key</strong> <em>(High)</em><br>\nYour DID has one.</p>\n\
         </li>\n\
         <li class=\"task-list-item\">\n\
         <p><input class=\"task-list-item-checkbox\" checked=\"\" disabled=\"\" type=\"checkbox\"> \
         <strong>Done</strong> it</p>\n\
         <p>More.</p>\n\
         </li>\n\
         </ul>\n"
    );
    assert_eq!(out.task_items.len(), 2);
    assert_eq!(
        out.plain_text,
        "Add a key (High) Your DID has one.\nDone it\nMore."
    );
}

#[test]
fn a_tight_task_item_keeps_its_checkbox_first_with_wikijs_classes() {
    let out = r("- [ ] a\n- [x] b\n- c\n");
    assert_eq!(
        out.html,
        "<ul class=\"contains-task-list\">\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\"> a</li>\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" checked=\"\" disabled=\"\" type=\"checkbox\"> b</li>\n\
         <li>c</li>\n\
         </ul>\n"
    );
    // A list without task items has no class.
    assert_eq!(r("- c\n").html, "<ul>\n<li>c</li>\n</ul>\n");
}

#[test]
fn fragments_are_percent_decoded_before_classification() {
    // A browser decodes the fragment before matching it to an id, so
    // `#caf%C3%A9` targets `id="café"`.
    assert_eq!(classify("#caf%C3%A9"), LinkKind::Anchor("café".into()));
    assert_eq!(classify("#café"), LinkKind::Anchor("café".into()));
    assert_eq!(classify("#a%20b"), LinkKind::Anchor("a b".into()));
    assert_eq!(
        classify("/p#caf%C3%A9"),
        LinkKind::Internal {
            path: "/p".into(),
            fragment: Some("café".into())
        }
    );
    // Not UTF-8 once decoded, or not an escape: kept as written.
    assert_eq!(classify("#%FF"), LinkKind::Anchor("%FF".into()));
    assert_eq!(classify("#100%"), LinkKind::Anchor("100%".into()));
    assert_eq!(classify("#%zz"), LinkKind::Anchor("%zz".into()));
    // The href itself is never changed.
    let out = r("[x](#caf%C3%A9)\n");
    assert_eq!(out.links[0].href, "#caf%C3%A9");
    assert_eq!(out.links[0].kind, LinkKind::Anchor("café".into()));
}

/// markdown-it-task-lists 2.1.1 (Wiki.js 2.5.314's) makes an item a task
/// only when its first block is a paragraph whose content (markdown-it trims
/// it) starts with `[ ] `, `[x] ` or `[X] `: the space is a literal space and
/// something must follow it. The previous renderer also takes a marker followed by a tab, by
/// the end of the paragraph or by the end of its first text run; Wiki.js
/// writes those as text. Wiki.js's HTML for each case (markdown-it 11.0.1 in
/// the prod container).
#[test]
fn task_markers_markdown_it_task_lists_does_not_take_stay_text() {
    let cases = [
        ("- [ ]\n", "<ul>\n<li>[ ]</li>\n</ul>\n"),
        ("- [x] \n", "<ul>\n<li>[x]</li>\n</ul>\n"),
        ("- [X]\n", "<ul>\n<li>[X]</li>\n</ul>\n"),
        ("- [ ]\ttab\n", "<ul>\n<li>[ ]\ttab</li>\n</ul>\n"),
        (
            "- [ ]**st**\n",
            "<ul>\n<li>[ ]<strong>st</strong></li>\n</ul>\n",
        ),
        (
            "- [ ]\n  next line\n",
            "<ul>\n<li>[ ]<br>\nnext line</li>\n</ul>\n",
        ),
        (
            "- [x]\n\n  text after\n",
            "<ul>\n<li>\n<p>[x]</p>\n<p>text after</p>\n</li>\n</ul>\n",
        ),
        // The content as written, not as decoded: an entity is not a space.
        ("- [ ]&#32;x\n", "<ul>\n<li>[ ] x</li>\n</ul>\n"),
        ("- [ ]\\\n  b\n", "<ul>\n<li>[ ]<br>\nb</li>\n</ul>\n"),
    ];
    for (md, html) in cases {
        let out = r(md);
        assert_eq!(out.html, html, "{md:?}");
        assert_eq!(out.task_items, [], "{md:?}");
        assert!(
            out.plain_text.starts_with(&md[2..5]),
            "{md:?}: the marker is text: {:?}",
            out.plain_text
        );
    }
    // One real item keeps the list's class; the other item is plain.
    let out = r("- [x] a\n- [ ]\n");
    assert_eq!(
        out.html,
        "<ul class=\"contains-task-list\">\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" checked=\"\" disabled=\"\" type=\"checkbox\"> a</li>\n\
         <li>[ ]</li>\n\
         </ul>\n"
    );
    assert_eq!(out.task_items.len(), 1);
}

/// What markdown-it-task-lists does take, the previous renderer too: a space then text, even
/// when the text is only a line break away (`[ ]  ⏎` and `[ ] ⏎` keep the
/// space in markdown-it's trimmed content).
#[test]
fn task_markers_both_take_stay_task_items() {
    for md in [
        "- [ ] ok\n",
        "- [X] up\n",
        "1. [ ] num\n",
        "- [ ] `c`\n",
        "- [ ]   spaces\n",
        "- [ ]  \n  hard\n",
        "- [ ] \n  soft\n",
        "- [ ] **st**\n",
    ] {
        let out = r(md);
        assert_eq!(out.task_items.len(), 1, "{md:?}: {}", out.html);
        assert!(out.html.contains("contains-task-list"), "{md:?}");
        assert!(!out.html.contains('['), "{md:?}: {}", out.html);
    }
    // As todoify writes it: the space after the marker is the text's, so a
    // break right after it follows the checkbox directly.
    assert_eq!(
        r("- [ ]  \n  hard\n").html,
        "<ul class=\"contains-task-list\">\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\"><br>\nhard</li>\n\
         </ul>\n"
    );
}

/// Table cells are separated by a space in the plain text, rows by a line
/// (the old recursive walk ended every cell with a newline).
#[test]
fn a_table_rows_plain_text_is_its_cells_on_one_line() {
    let out = r("| a | b |\n|---|---|\n| c | d |\n");
    assert_eq!(out.plain_text, "a b\nc d");
}

/// markdown-it reads a reference definition opening an item as a block of
/// its own, so the item's paragraph, and the marker test, start on the line
/// after it; the task item is on that line, the checkbox's (the previous renderer
/// reported the definition's line, `docs/spec/deviations.md`). Wiki.js's HTML
/// (markdown-it 11.0.1, task-lists 2.1.1).
#[test]
fn a_reference_definition_opening_an_item_does_not_hide_its_task_marker() {
    for (md, checked) in [
        ("- [y]: /w\n  [ ] x\n", false),
        ("- [y]: /w\n  [x] x\n", true),
    ] {
        let out = r(md);
        assert_eq!(
            out.task_items,
            [TaskItem {
                line: 2,
                checked,
                section: None
            }],
            "{md:?}"
        );
        let checked = if checked { " checked=\"\"" } else { "" };
        assert_eq!(
            out.html,
            format!(
                "<ul class=\"contains-task-list\">\n\
                 <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\"{checked} disabled=\"\" type=\"checkbox\"> x</li>\n\
                 </ul>\n"
            ),
            "{md:?}"
        );
    }
    // The marker alone after the definition is text, as in Wiki.js; a break
    // after the marker's space still leaves something after it.
    let out = r("- [y]: /w\n  [ ]\n");
    assert_eq!(
        (out.task_items.len(), out.html.as_str()),
        (0, "<ul>\n<li>[ ]</li>\n</ul>\n")
    );
    assert_eq!(
        r("- [y]: /w\n  [ ]  \n  z\n").html,
        "<ul class=\"contains-task-list\">\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\"><br>\nz</li>\n\
         </ul>\n"
    );
    // Still the allowed difference: a marker that is itself a reference link
    // stays a plain item (markdown-it: a task, then the link).
    let out = r("[x]: /u\n\n- [x] foo\n");
    assert_eq!(out.task_items, []);
    assert_eq!(out.html, "<ul>\n<li><a href=\"/u\">x</a> foo</li>\n</ul>\n");
}

/// The previous renderer, like markdown-it, ends a line at `\r\n`, `\r` or `\n`; every line
/// Rubrica reports is a line in that sense, and the marker and tilde tests
/// read the same lines.
#[test]
fn a_bare_carriage_return_ends_a_line_for_task_items_and_line_numbers() {
    let item = |line, checked| TaskItem {
        line,
        checked,
        section: None,
    };
    let out = r("- a\r- [x] b\n- [ ] c\n");
    assert_eq!(out.task_items, [item(2, true), item(3, false)]);
    assert_eq!(
        out.html,
        "<ul class=\"contains-task-list\">\n\
         <li>a</li>\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" checked=\"\" disabled=\"\" type=\"checkbox\"> b</li>\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\"> c</li>\n\
         </ul>\n"
    );
    // Headings, links and task items on lines ended every way.
    let out = r("# A\r[l](/u)\r\n## B\n- [ ] t\r");
    assert_eq!(
        out.headings.iter().map(|h| h.line).collect::<Vec<_>>(),
        [1, 3]
    );
    assert_eq!(out.links.iter().map(|l| l.line).collect::<Vec<_>>(), [2]);
    assert_eq!(
        out.task_items,
        [TaskItem {
            line: 4,
            checked: false,
            section: Some(1)
        }]
    );
    // The single-tilde test reads the tilde's own line.
    assert_eq!(r("a\r~b~ c\n").html, "<p>a<br>\n~b~ c</p>\n");
    assert_eq!(r("x\r~b~\n").html, "<p>x<br>\n~b~</p>\n");
}

/// The previous renderer numbers the inlines of a paragraph or setext heading that
/// reference definitions opened from the definitions' first line, and takes
/// their columns from it. markdown-it starts the block after the
/// definitions: links, headings and the single-tilde test are on the lines
/// they are written on, and the block's text starts at its first non-blank.
#[test]
fn lines_and_columns_after_reference_definitions_are_where_the_text_is() {
    let out = r("[y]: /w\n[l](/u) ~b~\n");
    assert_eq!(out.links.iter().map(|l| l.line).collect::<Vec<_>>(), [2]);
    assert_eq!(out.html, "<p><a href=\"/u\">l</a> ~b~</p>\n");
    let out = r("[y]: /w\nTitle [l](/u)\n===\n");
    assert_eq!(out.headings.iter().map(|h| h.line).collect::<Vec<_>>(), [2]);
    assert_eq!(out.links.iter().map(|l| l.line).collect::<Vec<_>>(), [2]);
    // Two lines of text after the definition, in a blockquote, each with its
    // own indentation: the tilde test reads each line's own bytes.
    let out = r("> [y]: /w\n>    b [l](/u)\n>  ~a~ [m](/v)\n");
    assert_eq!(out.links.iter().map(|l| l.line).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(
        out.html,
        "<blockquote>\n<p>b <a href=\"/u\">l</a><br>\n~a~ <a href=\"/v\">m</a></p>\n</blockquote>\n"
    );
    // A task item's text after the definition starts at its first non-blank.
    let out = r("- [y]: /w\n  [ ] x [l](/u)\n");
    assert_eq!(out.links.iter().map(|l| l.line).collect::<Vec<_>>(), [2]);
    assert_eq!(out.task_items.len(), 1);
    // A line that opens with an escape, or with blanks the previous renderer keeps: the
    // columns are still the line's own.
    assert_eq!(r("[y]: /w\n\\* ~b~\n").html, "<p>* ~b~</p>\n");
    assert_eq!(r("[y]: /w\n   b ~c~\n").html, "<p>b ~c~</p>\n");
    // Real strikethrough after a definition still strikes.
    assert_eq!(r("[y]: /w\na ~~b~~\n").html, "<p>a <del>b</del></p>\n");
}

/// markdown-it reads `\r\n` and `\r` as `\n` before anything else; the previous renderer
/// ends lines at both but keeps the `\r` in code it copies.
#[test]
fn carriage_returns_never_reach_the_html() {
    for md in [
        "```\r\na\r\nb\r\n```\r\n",
        "```\ra\rb\r```\r",
        "    a\r\n    b\r\n",
    ] {
        assert_eq!(r(md).html, "<pre><code>a\nb\n</code></pre>\n", "{md:?}");
    }
    assert_eq!(r("`a\r\nb`\r\n").html, "<p><code>a b</code></p>\n");
    let out = r("```mermaid\r\ngraph\r\nA\r\n```\r\n");
    assert_eq!(out.mermaid_blocks, ["graph\nA\n"]);
    assert_eq!(r("```\ra\rb\r```\r").plain_text, "a\nb");
}

fn link_lines(out: &Rendered) -> Vec<usize> {
    out.links.iter().map(|l| l.line).collect()
}

fn task_lines(out: &Rendered) -> Vec<(usize, bool)> {
    out.task_items.iter().map(|t| (t.line, t.checked)).collect()
}

/// The previous renderer counts a line end inside a link's or image's destination,
/// title or reference label as no line end at all: the link, and what
/// follows it on the line, is numbered from the line it starts on. A block
/// that opens with `[` then has fewer lines of inlines than of source with
/// no reference definition in it. Only a definition the previous renderer took out of the
/// block moves the block's start. Each case as Wiki.js 2.5.314 has it
/// (markdown-it 11.0.1, task-lists 2.1.1, on the replica).
#[test]
fn a_block_that_opens_with_a_bracket_moves_only_for_a_definition() {
    // A task on line 1, its link on line 1.
    let out = r("- [ ] see [a](/u\n  \"title\")\n");
    assert_eq!(task_lines(&out), [(1, false)]);
    assert_eq!(link_lines(&out), [1]);
    assert_eq!(
        out.html,
        "<ul class=\"contains-task-list\">\n\
         <li class=\"task-list-item\"><input class=\"task-list-item-checkbox\" disabled=\"\" type=\"checkbox\"> see <a href=\"/u\" title=\"title\">a</a></li>\n\
         </ul>\n"
    );
    let out = r("- [ ] x\n  [a](/u \"t\nu\")\n");
    assert_eq!(
        (task_lines(&out), link_lines(&out)),
        (vec![(1, false)], vec![2])
    );
    let out = r("- [ ] see [a](\n  /u)\n");
    assert_eq!(
        (task_lines(&out), link_lines(&out)),
        (vec![(1, false)], vec![1])
    );
    let out = r("- [ ] see ![i](/p.png\n  \"t\")\n");
    assert_eq!(
        (task_lines(&out), link_lines(&out)),
        (vec![(1, false)], vec![])
    );
    let out = r("- [ ] see [a][b\n  c]\n\n[b c]: /u\n");
    assert_eq!(
        (task_lines(&out), link_lines(&out)),
        (vec![(1, false)], vec![1])
    );
    // `~b~` stays text.
    let out = r("[a] ~b~ [c](/u \"t\nu\")\n");
    assert_eq!(link_lines(&out), [1]);
    assert_eq!(
        out.html,
        "<p>[a] ~b~ <a href=\"/u\" title=\"t\nu\">c</a></p>\n"
    );
    // A setext heading on line 1.
    let out = r("[a](/u \"t\nu\")\n===\n");
    assert_eq!(out.headings.iter().map(|h| h.line).collect::<Vec<_>>(), [1]);
    assert_eq!(link_lines(&out), [1]);
}

/// After a link, image or reference link whose syntax goes on to another
/// line, what follows is on that other line: the previous renderer numbers it from
/// the line the link starts on, with that line's columns, so a link after it
/// was reported a line early and the single-tilde test read other bytes
/// (`~b~` was struck through). With and without definitions; Wiki.js's
/// lines (the replica).
#[test]
fn what_follows_a_link_that_spans_lines_is_on_the_line_it_ends_on() {
    let out = r("[a](/u\n\"t\") ~b~ [c](/v)\n");
    assert_eq!(link_lines(&out), [1, 2]);
    assert_eq!(
        out.html,
        "<p><a href=\"/u\" title=\"t\">a</a> ~b~ <a href=\"/v\">c</a></p>\n"
    );
    let out = r("> [a](/u\n>  \"t\") [c](/v) ~d~\n");
    assert_eq!(link_lines(&out), [1, 2]);
    assert!(!out.html.contains("<del>"), "{}", out.html);
    let out = r("[a][b\nc] [d](/v)\n\n[b c]: /u\n");
    assert_eq!(link_lines(&out), [1, 2]);
    let out = r("x ![i](/p.png\n\"t\") [c](/v) ~e~\n");
    assert_eq!(link_lines(&out), [2]);
    assert!(!out.html.contains("<del>"), "{}", out.html);
    // A lazy line, its blanks kept by the previous renderer, inside the title.
    let out = r("> [a](/u\n   \"t\") [c](/v) ~d~\n");
    assert_eq!(link_lines(&out), [1, 2]);
    assert!(!out.html.contains("<del>"), "{}", out.html);
    // Two of them, then a line break: each line its own.
    let out = r("[a](/u\n\"t\") [b](/v\n\"s\") [c](/w)\n[d](/x) ~e~\n");
    assert_eq!(link_lines(&out), [1, 2, 3, 4]);
    assert!(!out.html.contains("<del>"), "{}", out.html);
    // After a definition as well.
    let out = r("[y]: /w\n[a](/u\n\"t\") [c](/v) ~b~\n");
    assert_eq!(link_lines(&out), [2, 3]);
    assert!(!out.html.contains("<del>"), "{}", out.html);
    let out = r("- [y]: /w\n  [ ] see [a](/u\n  \"t\") [c](/v)\n");
    assert_eq!(
        (task_lines(&out), link_lines(&out)),
        (vec![(2, false)], vec![2, 3])
    );
    // A definition whose label and title span lines.
    let out = r("[y\nz]: /w \"t\nu\"\n[c](/v) ~b~\n");
    assert_eq!(link_lines(&out), [4]);
    assert_eq!(out.html, "<p><a href=\"/v\">c</a> ~b~</p>\n");
    // Real strikethrough after it still strikes.
    assert_eq!(
        r("[a](/u\n\"t\") ~~b~~\n").html,
        "<p><a href=\"/u\" title=\"t\">a</a> <del>b</del></p>\n"
    );
}

/// A paragraph just before a table (its preface, which the previous renderer builds from
/// the lines before the header row) is corrected too; the previous renderer takes no
/// definitions from it.
#[test]
fn what_follows_a_link_that_spans_lines_in_a_tables_preface_is_on_its_line() {
    let out = r("[l](/u\n\"t\") ~a~ [m](/v)\n|b|\n|-|\n");
    assert_eq!(link_lines(&out), [1, 2]);
    assert!(!out.html.contains("<del>"), "{}", out.html);
    let out = r("x\n[y]: /w\n[m](/v\n\"t\") [n](/u) ~a~\n| b |\n| - |\n");
    assert_eq!(link_lines(&out), [3, 4]);
    assert!(!out.html.contains("<del>"), "{}", out.html);
}

/// A lazy line whose containers stop halfway through a tab: the previous renderer writes
/// spaces for the rest of the tab, and the text after a definition starts
/// at its own byte, so the marker test reads the marker (CommonMark's
/// structure: commonmark.js has the item's text `[ ] x`; markdown-it ends
/// the item: allowed differences). The item is on the checkbox's line (see
/// `docs/spec/deviations.md`).
#[test]
fn text_after_a_half_consumed_tab_is_read_at_its_own_byte() {
    let out = r("> 1.  [y]: /w\n>\t[ ] x\n");
    assert_eq!(task_lines(&out), [(2, false)]);
}

fn heading_lines(out: &Rendered) -> Vec<usize> {
    out.headings.iter().map(|h| h.line).collect()
}

/// The previous renderer moves every referenced footnote definition to the end of the
/// document, so the blockquotes and list items it was written in are no
/// longer its ancestors; their prefixes still begin its continuation lines.
/// A link after one spanning lines there, and a block that definitions open
/// there, are on the lines Wiki.js 2.5.314 has them on (markdown-it 11.0.1
/// with markdown-it-footnote 3.0.3, on the replica).
#[test]
fn a_footnote_definition_in_a_container_keeps_its_lines() {
    let out = r(
        "- Item[^1]\n\n  [^1]: See [the docs](https://example.com/a\n  \"Docs\") and [this](/x).\n",
    );
    assert_eq!(link_lines(&out), [3, 4]);
    let out = r("x[^1]\n\n> [^1]: [y]: /w\n>     h\n>     ===\n");
    assert_eq!(heading_lines(&out), [4]);
    let out = r("> - a[^n]\n>\n>   [^n]: [b](/u\n>   \"t\") [c](/v)\n");
    assert_eq!(link_lines(&out), [3, 4]);
}

/// Reference definitions, then a line that could underline them as a setext heading,
/// then a pipe row and a delimiter row: the `=` line is a paragraph and the pipe row is
/// the header of a table, as in markdown-it, commonmark.js and cmark-gfm. Each link is on
/// its own line.
#[test]
fn definitions_an_underline_and_a_pipe_row_give_a_paragraph_and_a_table() {
    let out = r("[r]: /d\n=\n[a](/u) | b\n-|-\n");
    assert_eq!(link_lines(&out), [3]);
    assert!(out.html.starts_with("<p>=</p>\n<table>\n"), "{}", out.html);
    assert!(out.html.contains("<a href=\"/u\">a</a>"), "{}", out.html);
    assert!(!out.html.contains("<br>"), "{}", out.html);
    let out = r("> [r]: /d\n> =\n> [a](/u) | b\n> -|-\n");
    assert_eq!(link_lines(&out), [3]);
    assert!(out.html.contains("<p>=</p>\n<table>"), "{}", out.html);
    // A link spanning lines in the preface, and a body row.
    let out = r("[r]: /d\n=\n[l](/v\n\"t\") [m](/w)\n[a](/u) | b\n-|-\n[c](/x) | d\n");
    assert_eq!(link_lines(&out), [3, 4, 5, 7]);
    assert!(out.html.contains("</p>\n<table>"), "{}", out.html);
}

/// A NUL is one byte to the previous renderer's positions as to its text: what follows a
/// link spanning lines is on its own line with one in the paragraph.
#[test]
fn a_nul_does_not_stop_the_positions_being_corrected() {
    let out = r("[a](/u\n\"t\") \0 [b](/v)\n");
    assert_eq!(link_lines(&out), [1, 2]);
}

/// The same, with the header row on a lazy line whose containers stop
/// halfway through a tab: the row is on the line it is
/// written on in the previous renderer's own structure (the third), not left a line early.
#[test]
fn a_table_after_an_underline_on_a_half_tab_lazy_line_is_on_its_own_line() {
    for md in [
        "> > [r]: /d\n> > =\n>\t[a](/u) | b\n> > -|-\n",
        "> 1. [y]: /d\n>    =\n>\t[a](/u) | b\n>    -|-\n",
    ] {
        let out = r(md);
        assert_eq!(link_lines(&out), [3], "{md:?}");
    }
}

/// Definitions, then an underline line, then a one-cell delimiter row: the `=` line is the
/// table's header row, as it is in markdown-it, commonmark.js and cmark-gfm.
#[test]
fn an_underline_before_a_delimiter_row_is_the_table_header() {
    let out = r("[q]: /w\n=\n|-\n");
    assert!(out.html.starts_with("<table>\n"), "{}", out.html);
    assert!(out.html.contains("<th>=</th>"), "{}", out.html);
    assert!(!out.html.contains("<p>"), "{}", out.html);
    let out = r("> [q]: /w\n> =\n> |-\n> [c](/u) | d\n");
    assert_eq!(link_lines(&out), [4]);
    assert!(out.html.contains("<th>=</th>"), "{}", out.html);
}
