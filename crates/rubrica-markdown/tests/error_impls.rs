//! Every public error type is a `std::error::Error`, so `?` converts it into a
//! `Box<dyn Error>`.

use std::error::Error;

use rubrica_markdown::{HeadingLevel, InlineStr};

/// `?` into a `Box<dyn Error>`, for any error type (including one the crate does not name).
fn boxed<T, E: Error + 'static>(r: Result<T, E>) -> Result<T, Box<dyn Error>> {
    Ok(r?)
}

#[test]
fn invalid_heading_level_is_an_error() {
    let e = boxed(HeadingLevel::try_from(7usize)).unwrap_err();
    assert!(e.source().is_none());
    assert!(e.to_string().contains('7'), "{e}");
}

#[test]
fn string_too_long_is_an_error() {
    let e = boxed(InlineStr::try_from("this string is far too long to be inline")).unwrap_err();
    assert!(e.source().is_none());
    assert!(!e.to_string().is_empty());
}
