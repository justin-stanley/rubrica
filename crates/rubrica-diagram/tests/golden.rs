//! Determinism: the flowchart corpus renders to its blessed SVG byte for byte, rendering is a
//! pure function of the source, the output keeps to the SVG vocabulary, and the crate's source
//! holds no map type, float, clock or environment read.
//!
//! `RUBRICA_DIAGRAM_BLESS=1` rewrites every `.svg` from the current code and then fails, so a
//! blessing is never mistaken for a pass: run again without it.

use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use rubrica_diagram::render;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/flowchart"
    ))
}

/// The corpus: (file name, source), sorted by name.
fn corpus() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "mmd"))
        .map(|p| {
            (
                p.file_name().unwrap().to_str().unwrap().to_string(),
                fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    assert_eq!(out.len(), 20, "the flowchart corpus has 20 files");
    out
}

fn draw(file: &str, src: &str) -> String {
    render(src).unwrap_or_else(|e| panic!("{file}: render failed: {e:?}"))
}

/// Where `a` and `b` first differ, with up to 60 bytes of each from there.
fn first_difference(a: &[u8], b: &[u8]) -> String {
    let at = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let ctx = |s: &[u8]| {
        String::from_utf8_lossy(&s[at.min(s.len())..(at + 60).min(s.len())]).into_owned()
    };
    format!(
        "first difference at byte {at}:\n  rendered: {:?}\n  blessed:  {:?}",
        ctx(a),
        ctx(b)
    )
}

#[test]
fn the_flowchart_corpus_is_byte_identical() {
    let bless = std::env::var("RUBRICA_DIAGRAM_BLESS").is_ok_and(|v| v == "1");
    let mut failures = Vec::new();
    let files = corpus();
    for (file, src) in &files {
        let svg = draw(file, src);
        let path = dir().join(Path::new(file).with_extension("svg"));
        if bless {
            fs::write(&path, &svg).unwrap();
            continue;
        }
        match fs::read(&path) {
            Ok(want) if want == svg.as_bytes() => {}
            Ok(want) => failures.push(format!(
                "{file}: does not match {}: {}",
                path.file_name().unwrap().to_string_lossy(),
                first_difference(svg.as_bytes(), &want)
            )),
            Err(e) => failures.push(format!("{file}: no blessed SVG ({e})")),
        }
    }
    if bless {
        panic!(
            "blessed {} files; run again without RUBRICA_DIAGRAM_BLESS",
            files.len()
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn rendering_is_a_pure_function() {
    let files = corpus();
    let mut seen: Vec<Vec<String>> = vec![Vec::new(); files.len()];
    for _ in 0..2 {
        for (i, (file, src)) in files.iter().enumerate() {
            seen[i].push(draw(file, src));
        }
    }
    for _ in 0..2 {
        for (i, (file, src)) in files.iter().enumerate().rev() {
            seen[i].push(draw(file, src));
        }
    }
    for ((file, _), outs) in files.iter().zip(&seen) {
        assert_eq!(outs.len(), 4);
        assert!(outs.iter().all(|o| o == &outs[0]), "{file}: renders differ");
    }
}

// ---- the vocabulary -----------------------------------------------------------------------------

#[derive(Debug)]
enum Tok {
    Start(String, Vec<(String, String)>, bool),
    End(String),
    Text(String),
    Other(String),
}

#[derive(Default)]
struct Sink {
    toks: RefCell<Vec<Tok>>,
}

impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut t = self.toks.borrow_mut();
        match token {
            Token::TagToken(tag) => match tag.kind {
                TagKind::StartTag => t.push(Tok::Start(
                    tag.name.to_string(),
                    tag.attrs
                        .iter()
                        .map(|a| (a.name.local.to_string(), a.value.to_string()))
                        .collect(),
                    tag.self_closing,
                )),
                TagKind::EndTag => t.push(Tok::End(tag.name.to_string())),
            },
            Token::CharacterTokens(s) => match t.last_mut() {
                Some(Tok::Text(x)) => x.push_str(&s),
                _ => t.push(Tok::Text(s.to_string())),
            },
            Token::EOFToken => {}
            other => t.push(Tok::Other(format!("{other:?}"))),
        }
        TokenSinkResult::Continue
    }
}

fn tokens(svg: &str) -> Vec<Tok> {
    let tokenizer = Tokenizer::new(Sink::default(), TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(svg.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.toks.into_inner()
}

/// `[0-9]{1,6}\.[0-9]{2}`.
fn is_number(s: &str) -> bool {
    let Some((a, b)) = s.split_once('.') else {
        return false;
    };
    (1..=6).contains(&a.len())
        && b.len() == 2
        && a.bytes().all(|c| c.is_ascii_digit())
        && b.bytes().all(|c| c.is_ascii_digit())
}

/// `M N N` then any of ` L N N`, ` M N N`, ` H N`, ` V N`, ` Z`.
fn is_path(s: &str) -> bool {
    let words: Vec<&str> = s.split(' ').collect();
    let mut i = 0;
    let mut first = true;
    while i < words.len() {
        let w = words[i];
        let (letter, rest) = w.split_at(w.len().min(1));
        let ok = match letter {
            "M" | "L" if letter == "M" || !first => {
                i += 1;
                is_number(rest) && words.get(i).is_some_and(|n| is_number(n))
            }
            "H" | "V" if !first => is_number(rest),
            "Z" if !first => rest.is_empty(),
            _ => false,
        };
        if !ok {
            return false;
        }
        first = false;
        i += 1;
    }
    !first
}

fn classes_ok(element: &str, value: &str) -> bool {
    let allowed: &[&str] = match element {
        "g" => &[
            "rd-clusters",
            "rd-edges",
            "rd-nodes",
            "rd-labels",
            "rd-node",
            "rd-cluster",
            "rd-edge",
            "rd-edge-solid",
            "rd-edge-dotted",
            "rd-edge-thick",
            "rd-edge-label",
            "rd-actors",
            "rd-actor",
            "rd-lifelines",
            "rd-activations",
            "rd-messages",
            "rd-notes",
            "rd-note",
            "rd-frames",
            "rd-frame",
        ],
        "rect" => &[
            "rd-shape",
            "rd-label-bg",
            "rd-cluster-box",
            "rd-actor-box",
            "rd-activation",
            "rd-note-box",
            "rd-frame-box",
            "rd-frame-tag",
            "rd-start",
            "rd-end-ring",
            "rd-end-dot",
        ],
        "path" => &[
            "rd-shape",
            "rd-shape-line",
            "rd-line",
            "rd-arrow",
            "rd-lifeline",
            "rd-frame-divider",
        ],
        "text" => &[
            "rd-text",
            "rd-cluster-title",
            "rd-frame-label",
            "rd-frame-text",
        ],
        _ => &[],
    };
    let names: Vec<&str> = value.split(' ').collect();
    if element == "svg" {
        return names.len() == 2
            && names[0] == "rd"
            && ["rd-flowchart", "rd-sequence", "rd-state"].contains(&names[1]);
    }
    (1..=2).contains(&names.len()) && names.iter().all(|n| allowed.contains(n))
}

/// The attribute names of `element`, in order: the table's, and `rect`'s with or without its
/// corner radii.
fn attribute_lists(element: &str) -> Vec<Vec<&'static str>> {
    match element {
        "svg" => vec![vec![
            "role",
            "viewbox",
            "width",
            "height",
            "class",
            "font-family",
            "font-size",
        ]],
        "title" | "desc" => vec![vec![]],
        "g" => vec![vec!["class"]],
        "rect" => vec![
            vec!["class", "x", "y", "width", "height"],
            vec!["class", "x", "y", "width", "height", "rx", "ry"],
        ],
        "path" => vec![vec!["class", "d"]],
        "text" => vec![vec!["class", "x", "y", "text-anchor"]],
        _ => vec![],
    }
}

fn value_ok(element: &str, name: &str, value: &str) -> bool {
    match (element, name) {
        (_, "class") => classes_ok(element, value),
        ("svg", "role") => value == "img",
        ("svg", "viewbox") => {
            let v: Vec<&str> = value.split(' ').collect();
            v.len() == 4 && v[0] == "0" && v[1] == "0" && is_number(v[2]) && is_number(v[3])
        }
        ("svg", "font-family") => value == "Inter, sans-serif",
        ("svg", "font-size") => value == "14",
        ("path", "d") => is_path(value),
        ("text", "text-anchor") => value == "middle" || value == "start",
        _ => is_number(value),
    }
}

/// Every vocabulary failure of one SVG.
fn vocabulary_failures(svg: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let mut open: Vec<String> = Vec::new();
    let mut svg_children = Vec::new();
    for tok in tokens(svg) {
        match tok {
            Tok::Start(name, attrs, self_closing) => {
                let lists = attribute_lists(&name);
                if lists.is_empty() {
                    bad.push(format!("element {name}"));
                }
                if self_closing {
                    bad.push(format!("self-closing {name}"));
                }
                let names: Vec<&str> = attrs.iter().map(|(k, _)| k.as_str()).collect();
                if !lists.is_empty() && !lists.contains(&names) {
                    bad.push(format!("{name} attributes {names:?}"));
                }
                for (k, v) in &attrs {
                    if !value_ok(&name, k, v) {
                        bad.push(format!("{name} {k}={v:?}"));
                    }
                }
                let parent = open.last().map(String::as_str);
                let parent_ok = match name.as_str() {
                    "svg" => parent.is_none(),
                    "title" | "desc" => parent == Some("svg"),
                    "g" => matches!(parent, Some("svg" | "g")),
                    _ => parent == Some("g"),
                };
                if !parent_ok {
                    bad.push(format!("{name} inside {parent:?}"));
                }
                if parent == Some("svg") {
                    svg_children.push(name.clone());
                }
                open.push(name);
            }
            Tok::End(name) => {
                if open.pop().as_deref() != Some(name.as_str()) {
                    bad.push(format!("end tag {name} closes nothing open"));
                }
            }
            Tok::Text(text) => {
                let parent = open.last().map(String::as_str);
                if !matches!(parent, Some("title" | "desc" | "text")) {
                    bad.push(format!("text {text:?} inside {parent:?}"));
                }
            }
            Tok::Other(t) => bad.push(format!("token {t}")),
        }
    }
    if !open.is_empty() {
        bad.push(format!("unclosed {open:?}"));
    }
    if svg_children.len() < 2 || svg_children[0] != "title" || svg_children[1] != "desc" {
        bad.push(format!("svg's children begin {svg_children:?}"));
    }
    bad
}

#[test]
fn only_the_vocabulary_is_emitted() {
    // The checker refuses what it must.
    for wrong in [
        "<svg role=\"img\"></svg>",
        "<svg role=\"img\" viewBox=\"0 0 1.00 1.00\" width=\"1.00\" height=\"1.00\" class=\"rd rd-flowchart\" font-family=\"Inter, sans-serif\" font-size=\"14\"><title></title><desc></desc><g class=\"rd-nodes\"><circle></circle></g></svg>",
        "<svg role=\"img\" viewBox=\"0 0 1.00 1.00\" width=\"1.00\" height=\"1.00\" class=\"rd rd-flowchart\" font-family=\"Inter, sans-serif\" font-size=\"14\"><title></title><desc></desc><g class=\"rd-nodes\"><path class=\"rd-line\" d=\"M1 2\"></path></g></svg>",
        "<svg role=\"img\" viewBox=\"0 0 1.00 1.00\" width=\"1.00\" height=\"1.00\" class=\"rd rd-flowchart\" font-family=\"Inter, sans-serif\" font-size=\"14\"><title></title><desc></desc><g class=\"rd-nodes\"><rect class=\"rd-shape\" x=\"1.00\" y=\"1.00\" width=\"1.00\" height=\"1.00\"/></g></svg>",
        "<svg role=\"img\" viewBox=\"0 0 1.00 1.00\" width=\"1.00\" height=\"1.00\" class=\"rd rd-flowchart\" font-family=\"Inter, sans-serif\" font-size=\"14\"><desc></desc><title></title></svg>",
        "<svg role=\"img\" viewBox=\"0 0 1.00 1.00\" width=\"1.00\" height=\"1.00\" class=\"rd rd-flowchart\" font-family=\"Inter, sans-serif\" font-size=\"14\"><title></title><desc></desc> </svg>",
    ] {
        assert!(!vocabulary_failures(wrong).is_empty(), "accepted {wrong}");
    }
    assert!(is_path(
        "M16.00 20.00 L30.00 40.00 H1.00 V2.00 M3.00 4.00 Z"
    ));
    assert!(!is_path("L16.00 20.00"));
    for (file, src) in corpus() {
        let svg = draw(&file, &src);
        let bad = vocabulary_failures(&svg);
        assert!(bad.is_empty(), "{file}: {}", bad.join("; "));
    }
}

// ---- the source -------------------------------------------------------------------------------

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Whether `word` occurs in `text` as a whole word: no identifier character just before it,
/// and none just after unless the word ends in punctuation (`rand::`).
fn has_word(text: &str, word: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(ident)
            && (!word.ends_with(|c: char| c.is_alphanumeric()) || !after.is_some_and(ident))
    })
}

#[test]
fn the_source_uses_no_map_float_clock_or_environment() {
    assert!(has_word("let m: HashMap<u8, u8>;", "HashMap"));
    assert!(!has_word("let m: MyHashMap;", "HashMap"));
    assert!(!has_word("x as f640", "f64"));
    assert!(has_word("use rand::Rng;", "rand::"));
    let mut files = Vec::new();
    sources(
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")),
        &mut files,
    );
    files.sort();
    assert!(files.len() >= 5, "{files:?}");
    let mut bad = Vec::new();
    for f in &files {
        let text = fs::read_to_string(f).unwrap();
        for word in [
            "HashMap",
            "HashSet",
            "f32",
            "f64",
            "unsafe",
            "std::time",
            "std::env",
            "rand::",
        ] {
            if has_word(&text, word) {
                bad.push(format!("{}: {word}", f.display()));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
