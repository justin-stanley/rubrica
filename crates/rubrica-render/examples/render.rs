//! Renders a Markdown string to sanitized HTML and prints it, then what else the render
//! found: the headings, the links and the plain text.
//!
//! Run it with `cargo run -p rubrica-render --example render`.

use rubrica_render::{render, LinkKind, RenderOptions};

fn main() {
    let markdown = "\
# Field notes

Some *emphasis*, a [link to another page](/notes/birds#owls) and an
[external one](https://example.com).

- [x] write the page
- [ ] check the links

<script>alert(1)</script>
";

    // The page ends with a <script> tag. The sanitizer removes the tag, so the output keeps
    // only its text, `alert(1)`, as plain, harmless text.
    let rendered = render(markdown, &RenderOptions::native());
    print!("{}", rendered.html);

    println!("--");
    for heading in &rendered.headings {
        println!("heading h{}: {}", heading.level, heading.text);
    }
    for link in &rendered.links {
        let kind = match link.kind {
            LinkKind::Internal { .. } => "internal",
            LinkKind::Anchor(_) => "anchor",
            LinkKind::External => "external",
            LinkKind::Relative => "relative",
        };
        println!("link (line {}, {kind}): {}", link.line, link.href);
    }
    println!("words: {}", rendered.word_count);
}
