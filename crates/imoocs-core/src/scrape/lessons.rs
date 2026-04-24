//! コース概要ページから lesson 一覧 (sidebar tree) を scrape する。
//!
//! コースページ (`/courses/<year>/<courseId>`) には `<aside>` に `TABLE OF CONTENTS`
//! の見出しがあり、ネストしたリストの各 `<a href>` が lesson URL を指す。
//! lesson は section 見出し (親 `<li>` の先頭 link / text) でグルーピングされることがある。

use std::collections::BTreeSet;

use scraper::{ElementRef, Html};

use crate::error::Result;
use crate::schemas::{LectureGroup, LessonRef, Year};
use crate::scrape::url::{self, MoocsPath};
use crate::session::moocs_url;
use crate::util::html::parse_selector;

pub fn scrape_course_lessons(html: &str, year: Year, course_id: &str) -> Result<Vec<LessonRef>> {
    let document = Html::parse_document(html);
    let aside_selector = parse_selector("aside a[href]")?;

    let mut seen = BTreeSet::new();
    let mut lessons = Vec::new();
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
        // lesson_id で重複排除しつつ、sidebar で最初に現れた順序を保つ。
        if !seen.insert(lesson_id.clone()) {
            continue;
        }

        let section = find_section(&a);
        lessons.push(LessonRef {
            year,
            course_id: course_id.to_string(),
            lesson_id,
            title,
            url: absolute,
            section,
        });
    }

    Ok(lessons)
}

fn find_section(el: &ElementRef<'_>) -> Option<String> {
    let mut node = el.parent();
    // DOM を数階層遡り、直下に <a>/<span> (section ラベル) を持つ <li> を探す。
    // Bootstrap sidebar の構造:
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

/// コースサイドバーの章立て (`ul.sidebar-menu li.treeview`) を走査して
/// LectureGroup の配列を返す。順序はサイドバーの出現順を維持。
/// `treeview` が見つからない / 空の場合は、フラットな `scrape_course_lessons`
/// 結果を 1 つのダミーグループ ("") に詰めて返す。
pub fn scrape_course_lecture_groups(html: &str, year: Year, course_id: &str) -> Result<Vec<LectureGroup>> {
    let document = Html::parse_document(html);
    let treeview_sel = parse_selector("aside ul.sidebar-menu li.treeview")?;
    let submenu_sel = parse_selector(":scope > ul.treeview-menu li a[href]")?;

    let mut groups: Vec<LectureGroup> = Vec::new();
    for li in document.select(&treeview_sel) {
        let title = li
            .children()
            .filter_map(ElementRef::wrap)
            .find(|c| matches!(c.value().name(), "a" | "span"))
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let mut lessons: Vec<LessonRef> = Vec::new();
        for a in li.select(&submenu_sel) {
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
            let lesson_title = a.text().collect::<String>().trim().to_string();
            lessons.push(LessonRef {
                year,
                course_id: course_id.to_string(),
                lesson_id,
                title: lesson_title,
                url: absolute,
                section: if title.is_empty() { None } else { Some(title.clone()) },
            });
        }

        if !lessons.is_empty() {
            groups.push(LectureGroup { title, lessons });
        }
    }

    // fallback: treeview 構造に一致しなかった場合は flat list に切り替え、
    // lessons がある限り CourseDetail.groups が空にならないようにする
    if groups.is_empty() {
        let flat = scrape_course_lessons(html, year, course_id)?;
        if !flat.is_empty() {
            groups.push(LectureGroup {
                title: String::new(),
                lessons: flat,
            });
        }
    }

    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrape_course_lessons_preserves_sidebar_order() {
        let html = r#"
        <aside>
          <ul class="sidebar-menu">
            <li><a href="/courses/2026/INI301/DS-10">Later</a></li>
            <li><a href="/courses/2026/INI301/DS-02">Earlier</a></li>
            <li><a href="/courses/2026/INI301/DS-10">Duplicate Later</a></li>
          </ul>
        </aside>
        "#;

        let lessons = scrape_course_lessons(html, 2026, "INI301").expect("lessons");
        let ids: Vec<_> = lessons.iter().map(|l| l.lesson_id.as_str()).collect();
        assert_eq!(ids, vec!["DS-10", "DS-02"]);
    }
}
