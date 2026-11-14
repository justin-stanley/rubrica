//! The shared preamble: the size cap, directives, front matter and the header.

use rubrica_diagram::{preamble, Kind, MAX_SOURCE_BYTES};

fn err(src: &str) -> (usize, String) {
    let u = preamble(src).unwrap_err();
    (u.line, u.why.to_string())
}

const FRONT: &str = "front matter other than a single `title:` line is not supported";
const INIT: &str = "`%%{init}%%` and other `%%{…}%%` directives are not supported";

#[test]
fn the_header_names_the_kind_and_the_lines_are_numbered() {
    let p = preamble("%% c\n\n  flowchart LR;  A-->B\n  C\n").unwrap();
    assert_eq!(p.kind, Kind::Flowchart);
    assert_eq!(p.title, "flowchart LR;  A-->B");
    assert_eq!(p.header_line, 3);
    assert_eq!(p.header_rest, " LR;  A-->B");
    assert_eq!(p.body, vec![(4, "  C")]);
}

#[test]
fn the_three_kinds_are_told_apart() {
    assert_eq!(preamble("graph TD\n").unwrap().kind, Kind::Flowchart);
    assert_eq!(preamble("graph;A\n").unwrap().kind, Kind::Flowchart);
    assert_eq!(preamble("sequenceDiagram\n").unwrap().kind, Kind::Sequence);
    assert_eq!(preamble("stateDiagram-v2\n").unwrap().kind, Kind::State);
    assert_eq!(
        err("stateDiagram\n"),
        (1, "`stateDiagram` diagrams are not supported".into())
    );
    assert_eq!(
        err("\n\nGraph TD\n"),
        (3, "`Graph` diagrams are not supported".into())
    );
}

#[test]
fn line_ends_are_not_part_of_the_text() {
    let p = preamble("graph TD\r\n  A --> B\r\n").unwrap();
    assert_eq!(p.title, "graph TD");
    assert_eq!(p.body, vec![(2, "  A --> B")]);
    let p = preamble("graph TD\n\n\n").unwrap();
    assert_eq!(p.body, vec![(2, ""), (3, "")]);
}

#[test]
fn front_matter_is_a_single_title() {
    let p = preamble("---\n\n  title:  Hello  \n---\n\ngraph LR\n  A\n").unwrap();
    assert_eq!(p.title, "Hello");
    assert_eq!(p.header_line, 6);
    assert_eq!(p.body, vec![(7, "  A")]);
    let p = preamble(" --- \r\ntitle: T\r\n---\r\ngraph TD\r\n").unwrap();
    assert_eq!(p.title, "T");
}

#[test]
fn front_matter_breaks_at_its_first_bad_line() {
    assert_eq!(err("---\ntitle: x\ngraph TD\n"), (1, FRONT.into()));
    assert_eq!(err("---\n"), (1, FRONT.into()));
    assert_eq!(err("---\n\n---\ngraph TD\n  A\n"), (1, FRONT.into()));
    assert_eq!(err("---\ntitle:\n---\ngraph TD\n"), (2, FRONT.into()));
    assert_eq!(err("---\ntitle:   \n---\ngraph TD\n"), (2, FRONT.into()));
    assert_eq!(err("---\nname: x\n---\ngraph TD\n"), (2, FRONT.into()));
    assert_eq!(
        err("---\ntitle: a\ntitle: b\n---\ngraph TD\n"),
        (3, FRONT.into())
    );
    assert_eq!(
        err("---\n\ntitle: a\n  theme: x\n---\ngraph TD\n"),
        (4, FRONT.into())
    );
}

#[test]
fn a_directive_anywhere_is_refused_first() {
    assert_eq!(err("---\ntitle: %%{x\n---\ngraph TD\n"), (2, INIT.into()));
    assert_eq!(err("graph TD\n  A --> B\n  %%{wrap}%%\n"), (3, INIT.into()));
    // The size cap comes before the directive.
    let big = format!("%%{{x}}%%\n{}", "a".repeat(MAX_SOURCE_BYTES));
    assert_eq!(
        err(&big),
        (
            0,
            format!(
                "the diagram source is {} bytes, over the limit of 32768",
                big.len()
            )
        )
    );
}

#[test]
fn nothing_but_blanks_and_comments_is_empty() {
    let empty = (0, "the diagram is empty".to_string());
    assert_eq!(err(""), empty);
    assert_eq!(err("\n\n"), empty);
    assert_eq!(err("  \n%% hi\n   %% there\n"), empty);
    assert_eq!(err("---\ntitle: x\n---\n\n%% c\n"), empty);
}
