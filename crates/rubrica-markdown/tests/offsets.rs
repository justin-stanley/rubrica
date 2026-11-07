//! Byte ranges from `parse_with_offsets` for blocks that start inside containers.

use rubrica_markdown::{parse_with_offsets, Event, GFM};

/// `" - >\t-"` (fuzz `offsets`, minimized): a list item holding a block quote holding a
/// list item. Bytes: 0 space, 1 `-`, 2 space, 3 `>`, 4 TAB, 5 `-`.
///
/// The TAB at byte 4 spans columns 4 to 8; the quote marker's optional space takes one of
/// its columns, and the remaining three are the inner item's indent. Those three columns
/// are no bytes of their own: they are the rest of byte 4, which the quote has already
/// consumed. Subtracting the indent's three *columns* from the marker's byte offset 5 gave
/// 2, before the quote (upstream 0.13.6 does the same). The inner list and item start at
/// the byte where their indent ends, 5, the marker: inside the quote (3..6), as the
/// offsets invariant requires.
#[test]
fn a_list_item_indented_by_a_partial_tab_starts_inside_its_quote() {
    let src = " - >\t-";
    let got: Vec<String> = parse_with_offsets(src, GFM)
        .map(|(e, r)| format!("{r:?} {e:?}"))
        .collect();
    let want = [
        "0..6 Start(List(None))",
        "0..6 Start(Item)",
        "3..6 Start(BlockQuote(None))",
        "5..6 Start(List(None))",
        "5..6 Start(Item)",
        "5..6 End(Item)",
        "5..6 End(List(false))",
        "3..6 End(BlockQuote(None))",
        "0..6 End(Item)",
        "0..6 End(List(false))",
    ];
    assert_eq!(got, want);
}

/// The invariant the `offsets` fuzz target checks: no `Start` begins before the previous
/// `Start`, on the minimized input and on variants with the TAB elsewhere.
#[test]
fn starts_never_go_backwards_after_a_partial_tab() {
    for src in [
        " - >\t-",
        "> >\t- a",
        "-\t>\t- b",
        " - >\t1. c",
        "> -\t-\tx",
    ] {
        let mut last = 0;
        for (e, r) in parse_with_offsets(src, GFM) {
            if matches!(e, Event::Start(_)) {
                assert!(r.start >= last, "{src:?}: {r:?} {e:?} before {last}");
                last = r.start;
            }
        }
    }
}
