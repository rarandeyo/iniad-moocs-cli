//! lesson ページから `.problem-container` 要素を見て課題を抽出する。
//!
//! 各要素は `data-urlprefix` (`/assignments/<year>/<course>/<problemId>` 形式) と
//! `data-problem` id を持っている。

use scraper::Html;

use crate::error::Result;
use crate::schemas::{AssignmentSummary, DerivedStatus, Year};
use crate::util::html::parse_selector;

/// 1 ページ分の HTML を走査し、含まれる課題を返す。
///
/// `status` は一旦 `AssignmentStatus::NonPublic` で埋める。caller 側が各課題の
/// `/status` を GET して本来の status を入れ直すことを想定している。
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
