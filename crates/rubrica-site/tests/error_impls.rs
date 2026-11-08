//! `BuildError` is a `std::error::Error`: `?` converts it into a `Box<dyn Error>`, and an
//! I/O failure is its `source()`.

use std::error::Error;
use std::io;

use rubrica_site::build::BuildError;

fn assert_error<E: Error + Send + Sync + 'static>() {}

fn fails(e: BuildError) -> Result<(), Box<dyn Error>> {
    Err(e)?
}

#[test]
fn build_error_is_an_error() {
    assert_error::<BuildError>();
}

#[test]
fn display_text_is_unchanged() {
    let text = |e: BuildError| fails(e).unwrap_err().to_string();
    assert_eq!(
        text(BuildError::Refused),
        "the site has findings it cannot be built with"
    );
    assert_eq!(
        text(BuildError::Config("no such page".into())),
        "no such page"
    );
    assert_eq!(
        text(BuildError::OutputOverBudget { limit: 9 }),
        "the site would be more than 9 bytes"
    );
    assert_eq!(
        text(BuildError::Io(io::Error::other("disk full"))),
        "disk full"
    );
}

#[test]
fn only_io_has_a_source() {
    for e in [
        BuildError::Refused,
        BuildError::Config("x".into()),
        BuildError::OutputOverBudget { limit: 1 },
    ] {
        assert!(e.source().is_none(), "{e}");
    }
    let e = BuildError::Io(io::Error::other("disk full"));
    let src = e.source().expect("Io has a source");
    assert_eq!(src.to_string(), "disk full");
    assert!(src.downcast_ref::<io::Error>().is_some());
}
