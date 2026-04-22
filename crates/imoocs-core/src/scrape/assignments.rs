//! Find assignments on a lesson page by inspecting `.problem-container` elements.
//!
//! Each element carries a `data-urlprefix` like
//! `/assignments/<year>/<course>/<problemId>` and a `data-problem` id.

use scraper::Html;

use crate::error::Result;
use crate::schemas::{AssignmentSummary, DerivedStatus, Year};
use crate::util::html::parse_selector;

/// Scan a single page's HTML and return any assignments present.
///
/// `status` is filled as `AssignmentStatus::NonPublic` for now; the caller
/// should GET `/status` for each assignment to fill in the real status.
pub fn scrape_assignments_on_page(
    html: &str,
    year: Year,
    course_id: &str,
    page_id: &str,
) -> Result<Vec<AssignmentSummary>> {
    let doc = Html::parse_document(html);
    let sel = parse_selector(".problem-container")?;
    let mut out = Vec::new();
    for el in doc.select(&sel) {
        let Some(problem_id) = el.value().attr("data-problem") else {
            continue;
        };
        out.push(AssignmentSummary {
            year,
            course_id: course_id.to_string(),
            problem_id: problem_id.to_string(),
            page_id: page_id.to_string(),
            status: crate::schemas::AssignmentStatus::NonPublic,
            derived_status: DerivedStatus::NonPublic,
            lesson_id: None,
            title: None,
        });
    }
    Ok(out)
}
