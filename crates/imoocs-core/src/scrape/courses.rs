//! Scrape `/courses[/<year>]` for the list of courses.
//!
//! Adapted from moocs-collect `src/repository/course.rs:48-64` (MIT, Copyright 2024 Yuki Natori).

use scraper::{ElementRef, Html};
use tracing::debug;

use crate::error::{ImoocsError, Result};
use crate::schemas::{Course, Year};
use crate::scrape::url::{self, MoocsPath};
use crate::session::moocs_url;
use crate::util::html::{extract_element_attribute, parse_selector};

/// Extract courses from the HTML of `/courses[/<year>]`.
pub fn scrape_course_list(html: &str) -> Result<Vec<Course>> {
    let document = Html::parse_document(html);
    let card_selector = parse_selector(".content .media")?;

    let mut out = Vec::new();
    for element in document.select(&card_selector) {
        if let Some(course) = extract_course(&element)? {
            out.push(course);
        }
    }
    debug!(count = out.len(), "scraped courses from /courses page");
    Ok(out)
}

fn extract_course(element: &ElementRef<'_>) -> Result<Option<Course>> {
    let name_selector = parse_selector(".media-body h4.media-heading")?;
    let name = match element.select(&name_selector).next() {
        Some(el) => el.text().collect::<String>().trim().to_string(),
        None => return Ok(None),
    };
    let href = extract_element_attribute(element, "a", "href")?;
    let absolute = absolutize(&href);

    match url::parse(&absolute) {
        Some(MoocsPath::Course { year, course_id }) => Ok(Some(Course {
            year,
            course_id,
            name,
            url: absolute,
        })),
        _ => Err(ImoocsError::Parse(format!("unexpected course card href: {href}"))),
    }
}

fn absolutize(href: &str) -> String {
    if href.starts_with("http") {
        href.to_string()
    } else {
        moocs_url(href)
    }
}

/// Scrape available archive years from the sidebar (from `/courses` response).
pub fn scrape_archive_years(html: &str) -> Result<Vec<Year>> {
    let document = Html::parse_document(html);
    let treeview_selector = parse_selector(".treeview-menu li a")?;

    let mut years = Vec::new();
    for el in document.select(&treeview_selector) {
        if let Some(href) = el.value().attr("href") {
            if let Some(year_str) = href.strip_prefix("/courses/") {
                if let Ok(year) = year_str.trim_end_matches('/').parse::<u32>() {
                    years.push(year);
                }
            }
        }
    }
    years.sort_by(|a, b| b.cmp(a));
    years.dedup();
    Ok(years)
}
