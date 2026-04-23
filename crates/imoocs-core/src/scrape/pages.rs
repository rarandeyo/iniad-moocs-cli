//! Enumerate the pages of a single lesson from the bottom `ul.pagination`.
//!
//! Adapted from moocs-collect `src/repository/page.rs:53-100` (MIT, Copyright 2024 Yuki Natori).

use scraper::{ElementRef, Html};

use crate::error::{ImoocsError, Result};
use crate::schemas::{Page, Year};
use crate::scrape::url::{self, MoocsPath};
use crate::session::moocs_url;
use crate::util::html::{extract_element_attribute, parse_selector};

/// Parse the list of pages for the given lesson.
///
/// - `html`: response body of `/courses/<year>/<course>/<lesson>[/<page>]`
/// - `current_url`: final URL after redirects (MOOCs redirects bare lesson
///   URLs to a specific page; we need this to resolve `href="#"` entries)
pub fn scrape_lesson_pages(
    html: &str,
    current_url: &str,
    year: Year,
    course_id: &str,
    lesson_id: &str,
) -> Result<Vec<Page>> {
    let document = Html::parse_document(html);
    let pagination_selector = parse_selector("ul.pagination li")?;

    let items: Vec<_> = document.select(&pagination_selector).collect();
    if items.len() <= 2 {
        // lesson が 1 ページしかない場合は現在の URL 自体をそのページとして返す
        if let Some(MoocsPath::Page {
            year: y,
            course_id: c,
            lesson_id: l,
            page_id,
        }) = url::parse(current_url)
        {
            if y == year && c == course_id && l == lesson_id {
                let title = document
                    .select(&parse_selector(".content-header h1")?)
                    .next()
                    .map(|n| n.text().collect::<String>().trim().to_string())
                    .unwrap_or_default();
                return Ok(vec![Page {
                    page_id,
                    title,
                    url: current_url.to_string(),
                    has_problem: has_problem_container(&document),
                }]);
            }
        }
        return Ok(vec![]);
    }

    let current_page_id = current_page_id(current_url, year, course_id, lesson_id);
    let mut out = Vec::new();
    for li in &items[1..items.len() - 1] {
        out.push(extract_page(
            li,
            year,
            course_id,
            lesson_id,
            current_page_id.as_deref(),
        )?);
    }
    Ok(out)
}

fn extract_page(
    li: &ElementRef<'_>,
    year: Year,
    course_id: &str,
    lesson_id: &str,
    current_page_id: Option<&str>,
) -> Result<Page> {
    let title = extract_element_attribute(li, "a", "title")?;
    let href = extract_element_attribute(li, "a", "href")?;

    let (page_id, url) = if href == "#" {
        let pid = current_page_id
            .ok_or_else(|| ImoocsError::Parse("`href=#` without a current page id".into()))?
            .to_string();
        (
            pid.clone(),
            crate::scrape::url::build_page(year, course_id, lesson_id, &pid),
        )
    } else {
        let absolute = if href.starts_with("http") {
            href.clone()
        } else {
            moocs_url(&href)
        };
        match url::parse(&absolute) {
            Some(MoocsPath::Page {
                year: y,
                course_id: c,
                lesson_id: l,
                page_id,
            }) if y == year && c == course_id && l == lesson_id => (page_id, absolute),
            _ => {
                return Err(ImoocsError::Parse(format!(
                    "pagination href does not match the lesson: {href}"
                )));
            }
        }
    };

    Ok(Page {
        page_id,
        title,
        url,
        has_problem: false, // filled later by content scraper when we fetch the page itself
    })
}

fn current_page_id(url: &str, year: Year, course_id: &str, lesson_id: &str) -> Option<String> {
    match url::parse(url) {
        Some(MoocsPath::Page {
            year: y,
            course_id: c,
            lesson_id: l,
            page_id,
        }) if y == year && c == course_id && l == lesson_id => Some(page_id),
        _ => None,
    }
}

fn has_problem_container(document: &Html) -> bool {
    match parse_selector(".problem-container") {
        Ok(s) => document.select(&s).next().is_some(),
        Err(_) => false,
    }
}
