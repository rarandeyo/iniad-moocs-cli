//! Small helpers for HTML scraping.
//!
//! Adapted from moocs-collect `src/utils.rs:4-22` (MIT, Copyright 2024 Yuki Natori).

use scraper::{ElementRef, Selector};

use crate::error::{ImoocsError, Result};

pub fn parse_selector(query: &str) -> Result<Selector> {
    Selector::parse(query).map_err(|e| ImoocsError::Parse(format!("invalid selector `{query}`: {e}")))
}

/// Extract the first element matched by `query` below `elm`, returning the value of `attribute`.
pub fn extract_element_attribute(elm: &ElementRef<'_>, query: &str, attribute: &str) -> Result<String> {
    let selector = parse_selector(query)?;
    elm.select(&selector)
        .next()
        .and_then(|element| element.value().attr(attribute).map(str::to_string))
        .ok_or_else(|| ImoocsError::Parse(format!("element `{query}` or attribute `{attribute}` not found")))
}
