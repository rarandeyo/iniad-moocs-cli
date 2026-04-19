//! High-level HTTP helpers that combine fetch + scrape.

use futures::stream::{self, StreamExt};
use tracing::debug;

use crate::api::assignments::get_status;
use crate::auth::is_logged_in_moocs;
use crate::error::{ImoocsError, Result};
use crate::schemas::{
    AssignmentKey, AssignmentSummary, Course, CourseDetail, Embed, Lesson, LessonContent, Page,
    Year,
};
use crate::scrape::{
    assignments::scrape_assignments_on_page,
    content::scrape_lesson_content,
    courses::scrape_course_list,
    lessons::scrape_course_lessons,
    pages::scrape_lesson_pages,
    url::{self, MoocsPath},
};
use crate::session::{moocs_url, Session};

/// Discover the "latest" year by following `/courses` redirects.
pub async fn resolve_latest_year(session: &Session) -> Result<Year> {
    let resp = session.client.get(moocs_url("/courses")).send().await?;
    let final_url = resp.url().clone();
    let status = resp.status();
    let _body = resp.text().await?; // consume response so cookies land

    // /courses redirects to /signin when unauthenticated — surface as Auth, not Internal.
    if final_url.path().starts_with("/signin") || status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ImoocsError::Auth {
            reason: "not logged in to MOOCs".into(),
            hint: Some("run `imoocs auth login`".into()),
        });
    }
    if !status.is_success() {
        return Err(ImoocsError::Api(format!(
            "GET /courses returned status {status}"
        )));
    }

    match url::parse(final_url.as_str()) {
        Some(MoocsPath::Year(y)) => Ok(y),
        Some(MoocsPath::CoursesIndex) => Err(ImoocsError::Internal(
            "MOOCs did not redirect /courses to a specific year".into(),
        )),
        Some(other) => Err(ImoocsError::Internal(format!(
            "unexpected /courses redirect target: {other:?}"
        ))),
        None => Err(ImoocsError::Internal(format!(
            "could not parse /courses redirect URL: {final_url}"
        ))),
    }
}

async fn ensure_authenticated(session: &Session) -> Result<()> {
    if is_logged_in_moocs(session).await? {
        Ok(())
    } else {
        Err(ImoocsError::Auth {
            reason: "not logged in to MOOCs".into(),
            hint: Some("run `imoocs auth login`".into()),
        })
    }
}

pub async fn get_course_list(session: &Session, year: Option<Year>) -> Result<Vec<Course>> {
    ensure_authenticated(session).await?;
    let url = url::build_courses_year(year);
    debug!(%url, "fetching course list");
    let html = session.client.get(&url).send().await?.error_for_status()?.text().await?;
    scrape_course_list(&html)
}

pub async fn get_course_detail(
    session: &Session,
    year: Year,
    course_id: &str,
) -> Result<CourseDetail> {
    ensure_authenticated(session).await?;
    let url = url::build_course(year, course_id);
    debug!(%url, "fetching course detail");
    let html = session.client.get(&url).send().await?.error_for_status()?.text().await?;
    let lessons = scrape_course_lessons(&html, year, course_id)?;

    // Also recover the course name from the top of the lesson list (course page title).
    // For simplicity, pick the first course in the courses-list that matches (we already
    // have the course_id/year, so a name fallback is fine).
    let course = Course {
        year,
        course_id: course_id.to_string(),
        name: extract_course_name(&html).unwrap_or_else(|| course_id.to_string()),
        url,
    };
    Ok(CourseDetail { course, lessons })
}

fn extract_course_name(html: &str) -> Option<String> {
    let doc = scraper::Html::parse_document(html);
    let sel = crate::util::html::parse_selector(".content-header h1").ok()?;
    let el = doc.select(&sel).next()?;
    let s = el.text().collect::<String>().trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

pub async fn get_lesson_page(
    session: &Session,
    year: Year,
    course_id: &str,
    lesson_id: &str,
    page_id: Option<&str>,
) -> Result<LessonContent> {
    ensure_authenticated(session).await?;
    let url = match page_id {
        Some(pid) => url::build_page(year, course_id, lesson_id, pid),
        None => url::build_lesson(year, course_id, lesson_id),
    };
    debug!(%url, "fetching lesson page");
    let resp = session.client.get(&url).send().await?.error_for_status()?;
    let final_url = resp.url().as_str().to_string();
    let html = resp.text().await?;

    let raw = scrape_lesson_content(&html)?;
    // Derive the resolved page_id (MOOCs redirects bare lesson URLs to their first page).
    let resolved_page_id = match url::parse(&final_url) {
        Some(MoocsPath::Page { page_id, .. }) => page_id,
        Some(MoocsPath::Lesson { .. }) => {
            page_id.unwrap_or_default().to_string()
        }
        _ => page_id.unwrap_or_default().to_string(),
    };

    Ok(LessonContent {
        year,
        course_id: course_id.to_string(),
        lesson_id: lesson_id.to_string(),
        page_id: resolved_page_id,
        title: raw.title,
        markdown: raw.markdown,
        embeds: raw.embeds,
    })
}

/// Crawl all pages of every lesson in a course, harvesting `.problem-container`
/// assignments. Fetches `/status` for each one in parallel to fill in status.
///
/// Concurrency: 4 concurrent page fetches + 4 concurrent status fetches.
pub async fn list_course_assignments(
    session: &Session,
    year: Year,
    course_id: &str,
) -> Result<Vec<AssignmentSummary>> {
    ensure_authenticated(session).await?;
    let detail = get_course_detail(session, year, course_id).await?;

    // Stage 1: for each lesson, fetch its bare URL to learn its pages.
    let lessons = detail.lessons.clone();
    let pages = stream::iter(lessons.into_iter())
        .map(|lref| {
            async move {
                let url = url::build_lesson(lref.year, &lref.course_id, &lref.lesson_id);
                let resp = session.client.get(&url).send().await?.error_for_status()?;
                let final_url = resp.url().as_str().to_string();
                let html = resp.text().await?;
                let pages = scrape_lesson_pages(
                    &html,
                    &final_url,
                    lref.year,
                    &lref.course_id,
                    &lref.lesson_id,
                )?;
                Ok::<_, ImoocsError>((lref, pages))
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;

    let mut all_pages: Vec<(String, String, String)> = Vec::new(); // (lesson_id, page_id, url)
    for res in pages {
        match res {
            Ok((lref, ps)) => {
                for p in ps {
                    all_pages.push((lref.lesson_id.clone(), p.page_id, p.url));
                }
            }
            Err(e) => return Err(e),
        }
    }

    // Stage 2: fetch each page, scrape assignments. Dedupe by problem_id.
    let course_owned = course_id.to_string();
    let assignment_vecs = stream::iter(all_pages.into_iter())
        .map(|(_lesson_id, page_id, page_url)| {
            let course = course_owned.clone();
            async move {
                let html = session.client.get(&page_url).send().await?.error_for_status()?.text().await?;
                let v = scrape_assignments_on_page(&html, year, &course, &page_id)?;
                Ok::<_, ImoocsError>(v)
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;

    let mut seen = std::collections::BTreeMap::<String, AssignmentSummary>::new();
    for res in assignment_vecs {
        for a in res? {
            seen.entry(a.problem_id.clone()).or_insert(a);
        }
    }

    // Stage 3: fetch /status for each in parallel.
    let list: Vec<AssignmentSummary> = stream::iter(seen.into_values())
        .map(|mut a| async move {
            let key = AssignmentKey {
                year: a.year,
                course_id: a.course_id.clone(),
                problem_id: a.problem_id.clone(),
            };
            match get_status(session, &key).await {
                Ok(s) => a.status = s,
                Err(_) => a.status = crate::schemas::AssignmentStatus::Error,
            }
            a
        })
        .buffer_unordered(4)
        .collect()
        .await;

    Ok(list)
}

/// Optional: fetch full page list for a lesson. Not used in MVP `course show` but
/// exposed for `lesson show --pages-only` etc. if we want later.
pub async fn get_lesson_pages(
    session: &Session,
    year: Year,
    course_id: &str,
    lesson_id: &str,
) -> Result<Lesson> {
    ensure_authenticated(session).await?;
    let url = url::build_lesson(year, course_id, lesson_id);
    let resp = session.client.get(&url).send().await?.error_for_status()?;
    let final_url = resp.url().as_str().to_string();
    let html = resp.text().await?;
    let pages: Vec<Page> = scrape_lesson_pages(&html, &final_url, year, course_id, lesson_id)?;

    // Title: the sidebar's section title is already in the lesson list; as a fallback we take
    // the rendered h1 on the lesson page.
    let title = extract_course_name(&html).unwrap_or_else(|| lesson_id.to_string());
    // Drop unused import warning for Embed in this fn: no-op
    let _ = std::marker::PhantomData::<Embed>;
    Ok(Lesson {
        year,
        course_id: course_id.to_string(),
        lesson_id: lesson_id.to_string(),
        title,
        pages,
    })
}
