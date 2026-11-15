//! Every diagram of the flowchart corpus passes the SVG allow-list's check and survives the
//! sanitizer's final pass byte for byte. The sources are held here, not read from the diagram
//! crate's fixtures: a test reads nothing outside its crate.

use rubrica_render::sanitize::{final_pass, svg_allowed};

/// The flowchart corpus: (name, source).
const CORPUS: [(&str, &str); 20] = [
    (
        "01-two-nodes",
        r#"graph TD
  A --> B
"#,
    ),
    (
        "02-chain-lr",
        r#"flowchart LR
  a --> b --> c --> d
"#,
    ),
    (
        "03-shapes",
        r#"flowchart TD
  r[Rectangle] --> o(Rounded)
  o --> d{Decision}
  d --> s([Stadium])
  d --> u[[Subroutine]]
"#,
    ),
    (
        "04-edge-kinds",
        r#"graph TD
  A --> B
  A --- C
  A -.-> D
  A ==> E
"#,
    ),
    (
        "05-pipe-labels",
        r#"graph TD
  A -->|yes| B
  A -->|no| C
  B -.->|later| D
  C ==>|now| D
"#,
    ),
    (
        "06-inline-labels",
        r#"graph LR
  A -- text --> B
  B -. maybe .-> C
  C == sure ==> D
  D -- plain --- E
"#,
    ),
    (
        "07-bottom-up",
        r#"graph BT
  leaf1 --> root
  leaf2 --> root
  root --> top
"#,
    ),
    (
        "08-right-left",
        r#"graph RL
  start[Start] --> mid[Middle] --> stop[Stop]
"#,
    ),
    (
        "09-cycle",
        r#"graph TD
  A --> B
  B --> C
  C --> A
  C --> D
"#,
    ),
    (
        "10-self-loop",
        r#"graph TD
  A --> A
  A -->|again| A
  A --> B
"#,
    ),
    (
        "11-fan-out",
        r#"graph TD
  hub --> a
  hub --> b
  hub --> c
  hub --> d
  hub --> e
  hub --> f
"#,
    ),
    (
        "12-fan-in",
        r#"graph LR
  a --> hub
  b --> hub
  c --> hub
  d --> hub
"#,
    ),
    (
        "13-long-edges",
        r#"graph TD
  A --> B --> C --> D
  A --> D
  A -->|skip| C
"#,
    ),
    (
        "14-parallel-edges",
        r#"graph LR
  A --> B
  A --> B
  A -.-> B
"#,
    ),
    (
        "15-subgraphs",
        r#"flowchart TB
  subgraph one [First group]
    a1 --> a2
  end
  subgraph two
    b1 --> b2
  end
  a2 --> b1
  c --> a1
"#,
    ),
    (
        "16-nested-subgraphs",
        r#"flowchart LR
  subgraph outer [Outer]
    subgraph inner [Inner]
      x --> y
    end
    y --> z
  end
  w --> x
  z --> v
"#,
    ),
    (
        "17-unicode-labels",
        r#"graph TD
  a[Café crème] --> b[Ελληνικά]
  b --> c[Кириллица]
  c --> d[“quotes” — dash…]
  d --> e[日本語]
"#,
    ),
    (
        "18-quoted-labels",
        r#"graph TD
  a["x < y & z"] --> b["5 > 3"]
  b --> c["a;b (c) [d]"]
"#,
    ),
    (
        "19-semicolons-comments",
        r#"graph TD;
%% a comment line
  A-->B;B-->C;
  C-->D
"#,
    ),
    (
        "20-front-matter-title",
        r#"---
title: Release flow
---
flowchart LR
  dev[Develop] --> rc[Release candidate] --> soak{Soak passes?}
  soak -->|yes| tag[Tag]
  soak -->|no| dev
"#,
    ),
];

#[test]
fn every_corpus_diagram_passes_the_check_and_survives_the_final_pass() {
    let mut bad = Vec::new();
    for (name, src) in CORPUS {
        let svg = rubrica_diagram::render(src).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        if let Err(why) = svg_allowed(&svg) {
            bad.push(format!("{name}: refused: {why}"));
        }
        let html = format!("<figure class=\"diagram\">{svg}</figure>");
        let out = final_pass(&html);
        if out != html {
            let at = out
                .bytes()
                .zip(html.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            bad.push(format!(
                "{name}: the final pass changed it at byte {at}: {:?}",
                out.get(at..(at + 60).min(out.len())).unwrap_or("")
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
