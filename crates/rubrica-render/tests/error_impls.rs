//! Every public error type is a `std::error::Error`, so `?` converts it into a
//! `Box<dyn Error>`.

use std::error::Error;

use rubrica_render::frontmatter::{split_front_matter, FrontMatterError};

fn assert_error<E: Error + Send + Sync + 'static>() {}

fn read(src: &str) -> Result<usize, Box<dyn Error>> {
    let (_, _, offset) = split_front_matter(src)?;
    Ok(offset)
}

#[test]
fn front_matter_error_is_an_error() {
    assert_error::<FrontMatterError>();
    let e = read("no front matter here").unwrap_err();
    assert!(e.source().is_none());
    assert!(e.to_string().contains("front matter"), "{e}");
}

#[test]
fn front_matter_error_names_its_line() {
    let e = read("---\ntitle: [unclosed\n---\n").unwrap_err();
    let fm = e.downcast_ref::<FrontMatterError>().unwrap();
    assert!(fm.line > 0);
    assert!(e.to_string().contains(&format!("line {}", fm.line)), "{e}");
}
