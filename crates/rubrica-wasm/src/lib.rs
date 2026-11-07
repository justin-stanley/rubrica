//! Rubrica's search, on `/_search` only: the query from `?q=`, matched against
//! `/_search.json` (titles, then headings, then body text), the hits written as elements
//! with their text set, never as markup. The rules are [`search`]'s; this module only
//! reads the page and writes into it.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Document, HtmlElement, HtmlInputElement, HtmlOListElement, RequestCredentials, RequestInit,
    Response, UrlSearchParams,
};

pub mod search;

/// The page's entry point, run once the module loads.
#[wasm_bindgen(start)]
pub async fn main() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let input = document
        .get_element_by_id("r:search-page")
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
    let results = document
        .get_element_by_id("r:results")
        .and_then(|e| e.dyn_into::<HtmlOListElement>().ok());
    let status = document
        .get_element_by_id("r:status")
        .and_then(|e| e.dyn_into::<HtmlElement>().ok());

    let params = UrlSearchParams::new_with_str(&window.location().search()?)?;
    let query = params.get("q").unwrap_or_default();
    let query = search::trim(&query);
    if let Some(input) = input {
        input.set_value(query);
    }
    let words = search::words(query);
    let (Some(first), Some(results)) = (words.first(), results) else {
        return Ok(());
    };

    let init = RequestInit::new();
    init.set_credentials(RequestCredentials::SameOrigin);
    let response: Response = JsFuture::from(window.fetch_with_str_and_init("/_search.json", &init))
        .await?
        .dyn_into()?;
    let text = JsFuture::from(response.text()?)
        .await?
        .as_string()
        .ok_or("the index is not text")?;
    let pages: Vec<search::Page> =
        serde_json::from_str(&text).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let hits = search::ranked(&pages, &words);
    for hit in hits.iter().take(search::LIMIT) {
        let li = item(&document, hit, first)?;
        results.append_child(&li)?;
    }
    if let Some(status) = status {
        status.set_text_content(Some(&search::status_line(hits.len())));
    }
    Ok(())
}

/// A hit's `<li>`: the link, its text set as text, and the snippet's paragraph.
fn item(
    document: &Document,
    hit: &search::Hit<'_>,
    first: &str,
) -> Result<web_sys::Element, JsValue> {
    let li = document.create_element("li")?;
    let a = document.create_element("a")?;
    a.set_attribute("href", &hit.href())?;
    a.set_text_content(Some(&hit.text()));
    li.append_child(&a)?;
    let p = document.create_element("p")?;
    p.set_text_content(Some(&search::snippet(&hit.page.body, first)));
    li.append_child(&p)?;
    Ok(li)
}
