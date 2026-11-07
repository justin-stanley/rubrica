//! Input nested deeper than any stack: every walk over the AST is
//! iterative. Each case runs on a thread with a 2 MiB stack, the size of
//! the worker threads `serve` will render on; a recursive walk aborts the
//! whole test binary with a stack overflow.

use rubrica_render::{render, RenderOptions, Rendered};

const DEPTH: usize = 100_000;

fn render_on_small_stack(md: String) -> Rendered {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || render(&md, &RenderOptions::default()))
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn a_100k_deep_blockquote_renders_on_a_2_mib_stack() {
    let out = render_on_small_stack(format!("{}x\n", ">".repeat(DEPTH)));
    assert_eq!(out.plain_text, "x");
    assert_eq!(out.word_count, 1);
}

#[test]
fn a_heading_at_the_bottom_of_a_deep_blockquote_gets_its_id() {
    let out = render_on_small_stack(format!("{} # Deep `x`\n", ">".repeat(DEPTH)));
    assert_eq!(out.headings.len(), 1);
    assert_eq!(out.headings[0].text, "Deep x");
    assert_eq!(out.headings[0].id.as_deref(), Some("deep-x"));
}

#[test]
fn deeply_nested_lists_render_on_a_2_mib_stack() {
    // The previous renderer stops nesting lists at 100 (MAX_LIST_DEPTH, counting every
    // open container); the rest of the line is text. Lists inside
    // blockquotes are the deepest a list gets.
    let out = render_on_small_stack(format!("{}x\n", "> - [ ] ".repeat(DEPTH / 2)));
    assert!(out.plain_text.ends_with('x'), "{:?}", &out.plain_text[..80]);
    assert!(!out.task_items.is_empty());
    let out = render_on_small_stack(format!("{}x\n", "- ".repeat(DEPTH)));
    assert!(out.plain_text.ends_with('x'));
}

#[test]
fn deeply_nested_emphasis_in_a_heading_renders_on_a_2_mib_stack() {
    let stars = "*".repeat(DEPTH / 2);
    let out = render_on_small_stack(format!("# {stars}deep{stars}\n\n> {stars}body{stars}\n"));
    assert_eq!(out.headings.len(), 1);
    assert_eq!(out.headings[0].text, "deep");
    assert_eq!(out.plain_text, "deep\nbody");
}
