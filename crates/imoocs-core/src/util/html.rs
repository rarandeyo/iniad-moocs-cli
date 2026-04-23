//! HTML scraping 用の小さなヘルパー群。
//!
//! moocs-collect `src/utils.rs:4-22` (MIT, Copyright 2024 Yuki Natori) より転用。

use scraper::{ElementRef, Selector};

use crate::error::{ImoocsError, Result};

pub fn parse_selector(query: &str) -> Result<Selector> {
    Selector::parse(query).map_err(|e| ImoocsError::Parse(format!("invalid selector `{query}`: {e}")))
}

pub fn extract_element_attribute(elm: &ElementRef<'_>, query: &str, attribute: &str) -> Result<String> {
    let selector = parse_selector(query)?;
    elm.select(&selector)
        .next()
        .and_then(|element| element.value().attr(attribute).map(str::to_string))
        .ok_or_else(|| ImoocsError::Parse(format!("element `{query}` or attribute `{attribute}` not found")))
}
