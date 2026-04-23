//! Parse and build MOOCs URLs.
//!
//! URL patterns:
//!   `/courses`
//!   `/courses/<year>`
//!   `/courses/<year>/<courseId>`
//!   `/courses/<year>/<courseId>/<lessonId>`
//!   `/courses/<year>/<courseId>/<lessonId>/<pageId>`

use once_cell::sync::Lazy;
use regex::Regex;

use crate::schemas::Year;
use crate::session::{moocs_url, MOOCS_BASE};

static RE_YEAR_ONLY: Lazy<Regex> = Lazy::new(|| Regex::new(r"^/courses/(\d{4})/?$").unwrap());
static RE_COURSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^/courses/(\d{4})/([^/]+)/?$").unwrap());
static RE_LESSON: Lazy<Regex> = Lazy::new(|| Regex::new(r"^/courses/(\d{4})/([^/]+)/([^/]+)/?$").unwrap());
static RE_PAGE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^/courses/(\d{4})/([^/]+)/([^/]+)/([^/]+)/?$").unwrap());

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoocsPath {
    CoursesIndex,
    Year(Year),
    Course {
        year: Year,
        course_id: String,
    },
    Lesson {
        year: Year,
        course_id: String,
        lesson_id: String,
    },
    Page {
        year: Year,
        course_id: String,
        lesson_id: String,
        page_id: String,
    },
}

pub fn extract_path(url_or_path: &str) -> &str {
    if let Some(stripped) = url_or_path.strip_prefix(MOOCS_BASE) {
        stripped
    } else if url_or_path.starts_with('/') {
        url_or_path
    } else {
        // そのまま渡す。この後の regex が match しないだけなので安全
        url_or_path
    }
}

pub fn parse(url_or_path: &str) -> Option<MoocsPath> {
    let path = extract_path(url_or_path);
    if path == "/courses" || path == "/courses/" {
        return Some(MoocsPath::CoursesIndex);
    }
    if let Some(c) = RE_YEAR_ONLY.captures(path) {
        return Some(MoocsPath::Year(c[1].parse().ok()?));
    }
    if let Some(c) = RE_COURSE.captures(path) {
        return Some(MoocsPath::Course {
            year: c[1].parse().ok()?,
            course_id: c[2].to_string(),
        });
    }
    if let Some(c) = RE_LESSON.captures(path) {
        return Some(MoocsPath::Lesson {
            year: c[1].parse().ok()?,
            course_id: c[2].to_string(),
            lesson_id: c[3].to_string(),
        });
    }
    if let Some(c) = RE_PAGE.captures(path) {
        return Some(MoocsPath::Page {
            year: c[1].parse().ok()?,
            course_id: c[2].to_string(),
            lesson_id: c[3].to_string(),
            page_id: c[4].to_string(),
        });
    }
    None
}

pub fn build_course(year: Year, course_id: &str) -> String {
    moocs_url(&format!("/courses/{year}/{course_id}"))
}

pub fn build_lesson(year: Year, course_id: &str, lesson_id: &str) -> String {
    moocs_url(&format!("/courses/{year}/{course_id}/{lesson_id}"))
}

pub fn build_page(year: Year, course_id: &str, lesson_id: &str, page_id: &str) -> String {
    moocs_url(&format!("/courses/{year}/{course_id}/{lesson_id}/{page_id}"))
}

pub fn build_courses_year(year: Option<Year>) -> String {
    match year {
        Some(y) => moocs_url(&format!("/courses/{y}")),
        None => moocs_url("/courses"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_courses_index() {
        assert_eq!(parse("/courses"), Some(MoocsPath::CoursesIndex));
        assert_eq!(parse("https://moocs.iniad.org/courses"), Some(MoocsPath::CoursesIndex));
    }

    #[test]
    fn parses_year() {
        assert_eq!(parse("/courses/2026"), Some(MoocsPath::Year(2026)));
    }

    #[test]
    fn parses_course() {
        assert_eq!(
            parse("/courses/2026/INI301"),
            Some(MoocsPath::Course {
                year: 2026,
                course_id: "INI301".into()
            })
        );
    }

    #[test]
    fn parses_lesson_and_page() {
        assert_eq!(
            parse("/courses/2026/INI301/DS-00"),
            Some(MoocsPath::Lesson {
                year: 2026,
                course_id: "INI301".into(),
                lesson_id: "DS-00".into()
            })
        );
        assert_eq!(
            parse("/courses/2026/INI301/DS-00/05"),
            Some(MoocsPath::Page {
                year: 2026,
                course_id: "INI301".into(),
                lesson_id: "DS-00".into(),
                page_id: "05".into()
            })
        );
        assert_eq!(
            parse("/courses/2026/COS201/02/atnd"),
            Some(MoocsPath::Page {
                year: 2026,
                course_id: "COS201".into(),
                lesson_id: "02".into(),
                page_id: "atnd".into()
            })
        );
    }
}
