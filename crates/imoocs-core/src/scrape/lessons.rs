//! Scrape a course overview page for its list of lessons (sidebar tree).
//!
//! The course page (`/courses/<year>/<courseId>`) contains an `<aside>` with a
//! `TABLE OF CONTENTS` heading and nested lists where each `<a href>` targets a
//! lesson URL. Lessons are optionally grouped under a section heading (the
//! parent `<li>`'s top-level link text).

use std::collections::BTreeMap;

use scraper::{ElementRef, Html};

use crate::error::Result;
use crate::schemas::{LessonRef, Year};
use crate::scrape::url::{self, MoocsPath};
use crate::session::moocs_url;
use crate::util::html::parse_selector;

/// Extract lessons for the given (year, course_id) from the course page HTML.
pub fn scrape_course_lessons(html: &str, year: Year, course_id: &str) -> Result<Vec<LessonRef>> {
    let document = Html::parse_document(html);
    // Lesson links live inside the aside/sidebar.
    let aside_selector = parse_selector("aside a[href]")?;

    let mut seen = BTreeMap::new();
    for a in document.select(&aside_selector) {
        let Some(href) = a.value().attr("href") else { continue };
        let absolute = absolutize(href);
        let MoocsPath::Lesson {
            year: y,
            course_id: c,
            lesson_id,
        } = url::parse(&absolute).unwrap_or(MoocsPath::CoursesIndex)
        else {
            continue;
        };
        if y != year || c != course_id {
            continue;
        }

        let title = a.text().collect::<String>().trim().to_string();
        // Deduplicate on lesson_id; prefer the first occurrence we saw.
        seen.entry(lesson_id.clone()).or_insert_with(|| {
            let section = find_section(&a);
            LessonRef {
                year,
                course_id: course_id.to_string(),
                lesson_id,
                title,
                url: absolute,
                section,
            }
        });
    }

    Ok(seen.into_values().collect())
}

/// Walk up the DOM to find the closest section heading (from the parent `<li>`'s
/// top-level link/text). Returns `None` if not found.
fn find_section(el: &ElementRef<'_>) -> Option<String> {
    let mut node = el.parent();
    // Walk up a few levels searching for an ancestor <li> that has its own
    // direct <a> or <span> labelling the section (Bootstrap sidebar pattern:
    //   <li><a>SectionTitle</a><ul><li><a>LessonTitle</a></li>...</ul></li>
    for _ in 0..6 {
        let Some(n) = node else { break };
        if let Some(li) = n.value().as_element() {
            if li.name() == "li" {
                if let Some(li_ref) = ElementRef::wrap(n) {
                    for child in li_ref.children() {
                        if let Some(child_el) = ElementRef::wrap(child) {
                            if child_el.value().name() == "a" || child_el.value().name() == "span" {
                                let text = child_el.text().collect::<String>().trim().to_string();
                                if !text.is_empty() {
                                    return Some(text);
                                }
                            }
                        }
                    }
                }
            }
        }
        node = n.parent();
    }
    None
}

fn absolutize(href: &str) -> String {
    if href.starts_with("http") {
        href.to_string()
    } else {
        moocs_url(href)
    }
}
