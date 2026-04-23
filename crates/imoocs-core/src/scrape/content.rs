//! lesson ページの本文 (markdown + 埋め込み iframe) を scrape する。

use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html};

use crate::error::Result;
use crate::schemas::{DriveKind, Embed};
use crate::util::html::parse_selector;

static SLIDES_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https://docs\.google\.com/(a/[^/]+/)?presentation/d/(e/)?[^/?]+/(embed|pubembed)").unwrap()
});

// `[^/?#]+` で `#` を除外するのは、`/file/d/<id>#foo` のような anchor 付き
// URL で `#foo` が fileId に混入するのを防ぐため。`#` が混ざると cache
// ファイル名に入り込むほか、構築される `download?id=<id>#foo&...` で reqwest
// の URL parser が `#` を fragment 区切りとして扱い、`&export=download&confirm=t`
// を黙って落としてしまう。
static DRIVE_FILE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^https://drive\.google\.com/file/d/([^/?#]+)(?:/preview|/view)?").unwrap());
static DRIVE_FOLDER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^https://drive\.google\.com/drive/folders/([^/?#]+)").unwrap());

pub struct LessonContentRaw {
    pub title: String,
    pub markdown: String,
    pub embeds: Vec<Embed>,
    pub has_problem: bool,
    /// `.problem-container[data-problem]` から検出した Problem ID 列。
    pub assignments: Vec<String>,
}

/// lesson ページから、人間が見る title / markdown 本文 / iframe 埋め込み、
/// および `.problem-container` の有無を抽出する。
pub fn scrape_lesson_content(html: &str) -> Result<LessonContentRaw> {
    let doc = Html::parse_document(html);

    let title = doc
        .select(&parse_selector(".content-header h1")?)
        .next()
        .map(|n| n.text().collect::<String>().trim().to_string())
        .or_else(|| {
            doc.select(&parse_selector("title").ok()?)
                .next()
                .map(|n| n.text().collect::<String>().trim().to_string())
        })
        .unwrap_or_default();

    let main_sel = parse_selector(".content-wrapper, main, section.content, .content, body")?;
    let main_root = doc.select(&main_sel).next().unwrap_or_else(|| doc.root_element());

    let md_sel = parse_selector(".markdown-block")?;
    let mut markdown = String::new();
    for block in doc.select(&md_sel) {
        let rendered = crude_markdown_from_element(&block);
        if !markdown.is_empty() {
            markdown.push_str("\n\n");
        }
        markdown.push_str(rendered.trim());
    }

    let iframe_sel = parse_selector("iframe")?;
    let mut embeds = Vec::new();
    for iframe in main_root.select(&iframe_sel) {
        let src = iframe
            .value()
            .attr("src")
            .or_else(|| iframe.value().attr("data-src"))
            .unwrap_or("")
            .to_string();
        if src.is_empty() {
            continue;
        }
        // すべての lesson に埋め込まれている cookie-helper は skip する
        if src.starts_with("https://storage.googleapis.com/moocs-files.iniad.org/tools/3pcc-start.html") {
            continue;
        }
        if SLIDES_RE.is_match(&src) {
            let pdf = derive_export_url(&src, "pdf");
            let pptx = derive_export_url(&src, "pptx");
            embeds.push(Embed::GoogleSlides {
                embed_url: src,
                export_pdf_url: pdf,
                export_pptx_url: pptx,
                local_pdf_path: None,
                size_bytes: None,
                page_count: None,
                fetched_at: None,
            });
        } else if let Some(caps) = DRIVE_FILE_RE.captures(&src) {
            let id = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            embeds.push(Embed::GoogleDrive {
                embed_url: src,
                kind: DriveKind::File,
                id,
            });
        } else if let Some(caps) = DRIVE_FOLDER_RE.captures(&src) {
            let id = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            embeds.push(Embed::GoogleDrive {
                embed_url: src,
                kind: DriveKind::Folder,
                id,
            });
        } else {
            embeds.push(Embed::Iframe { src });
        }
    }

    let problem_sel = parse_selector(".problem-container")?;
    let mut assignments: Vec<String> = Vec::new();
    for el in doc.select(&problem_sel) {
        if let Some(pid) = el.value().attr("data-problem") {
            assignments.push(pid.to_string());
        }
    }
    let has_problem = !assignments.is_empty();

    Ok(LessonContentRaw {
        title,
        markdown,
        embeds,
        has_problem,
        assignments,
    })
}

/// INIAD MOOCs で使われる `.markdown-block` の著者記述スニペット専用の
/// 最小限の HTML → Markdown 変換器。汎用 Markdown renderer ではない。
fn crude_markdown_from_element(el: &ElementRef<'_>) -> String {
    let mut out = String::new();
    render_node(*el, &mut out);
    let collapsed = Regex::new(r"\n{3,}").unwrap().replace_all(&out, "\n\n");
    collapsed.trim().to_string()
}

fn render_node(node: ElementRef<'_>, out: &mut String) {
    for child in node.children() {
        match child.value() {
            scraper::Node::Text(t) => {
                out.push_str(&t.text);
            }
            scraper::Node::Element(el) => {
                let tag = el.name();
                let child_ref = match ElementRef::wrap(child) {
                    Some(r) => r,
                    None => continue,
                };
                match tag {
                    "br" => out.push('\n'),
                    "p" => {
                        if !out.ends_with("\n\n") {
                            out.push_str("\n\n");
                        }
                        render_node(child_ref, out);
                        out.push_str("\n\n");
                    }
                    "a" => {
                        let href = el.attr("href").unwrap_or("").to_string();
                        let mut text = String::new();
                        render_node(child_ref, &mut text);
                        let text = text.trim();
                        if text.is_empty() {
                            out.push_str(&href);
                        } else if href.is_empty() {
                            out.push_str(text);
                        } else {
                            out.push_str(&format!("[{text}]({href})"));
                        }
                    }
                    "strong" | "b" => {
                        out.push_str("**");
                        render_node(child_ref, out);
                        out.push_str("**");
                    }
                    "em" | "i" => {
                        out.push('_');
                        render_node(child_ref, out);
                        out.push('_');
                    }
                    "code" => {
                        out.push('`');
                        render_node(child_ref, out);
                        out.push('`');
                    }
                    "pre" => {
                        out.push_str("\n```\n");
                        render_node(child_ref, out);
                        out.push_str("\n```\n");
                    }
                    "h1" => heading(child_ref, out, 1),
                    "h2" => heading(child_ref, out, 2),
                    "h3" => heading(child_ref, out, 3),
                    "h4" => heading(child_ref, out, 4),
                    "h5" => heading(child_ref, out, 5),
                    "h6" => heading(child_ref, out, 6),
                    "li" => {
                        out.push_str("- ");
                        render_node(child_ref, out);
                        out.push('\n');
                    }
                    "ul" | "ol" => {
                        out.push('\n');
                        render_node(child_ref, out);
                        out.push('\n');
                    }
                    _ => render_node(child_ref, out),
                }
            }
            _ => {}
        }
    }
}

fn heading(el: ElementRef<'_>, out: &mut String, level: u8) {
    if !out.ends_with("\n\n") {
        out.push_str("\n\n");
    }
    for _ in 0..level {
        out.push('#');
    }
    out.push(' ');
    render_node(el, out);
    out.push_str("\n\n");
}

fn derive_export_url(embed_url: &str, format: &str) -> String {
    let re = Regex::new(r"/(pubembed|embed)(\?.*)?$").unwrap();
    re.replace(embed_url, format!("/export/{format}")).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embeds(html: &str) -> Vec<Embed> {
        scrape_lesson_content(html).expect("scrape").embeds
    }

    #[test]
    fn classifies_drive_file_view() {
        let html = r#"<html><body><iframe src="https://drive.google.com/file/d/FAKE_DRIVE_FILE_ID_FOR_TESTS_0001/view?usp=drive_link"></iframe></body></html>"#;
        let got = embeds(html);
        assert_eq!(got.len(), 1);
        match &got[0] {
            Embed::GoogleDrive { embed_url, kind, id } => {
                assert!(embed_url.contains("FAKE_DRIVE_FILE_ID_FOR_TESTS_0001"));
                assert_eq!(*kind, DriveKind::File);
                assert_eq!(id, "FAKE_DRIVE_FILE_ID_FOR_TESTS_0001");
            }
            other => panic!("expected GoogleDrive, got {other:?}"),
        }
    }

    #[test]
    fn classifies_drive_file_preview() {
        let html = r#"<html><body><iframe src="https://drive.google.com/file/d/FAKE_DRIVE_FILE_ID_PREVIEW_0001/preview"></iframe></body></html>"#;
        match &embeds(html)[0] {
            Embed::GoogleDrive { kind, id, .. } => {
                assert_eq!(*kind, DriveKind::File);
                assert_eq!(id, "FAKE_DRIVE_FILE_ID_PREVIEW_0001");
            }
            _ => panic!("expected GoogleDrive file"),
        }
    }

    #[test]
    fn drive_file_fragment_does_not_leak_into_id() {
        let html = r#"<html><body><iframe src="https://drive.google.com/file/d/FAKE_DRIVE_FILE_ID_FRAGMENT_0001/view#junk"></iframe></body></html>"#;
        match &embeds(html)[0] {
            Embed::GoogleDrive { id, .. } => assert_eq!(id, "FAKE_DRIVE_FILE_ID_FRAGMENT_0001"),
            _ => panic!("expected GoogleDrive file"),
        }
    }

    #[test]
    fn classifies_drive_folder() {
        let html = r#"<html><body><iframe src="https://drive.google.com/drive/folders/FAKE_DRIVE_FOLDER_ID_FOR_TESTS_0001"></iframe></body></html>"#;
        match &embeds(html)[0] {
            Embed::GoogleDrive { kind, id, .. } => {
                assert_eq!(*kind, DriveKind::Folder);
                assert_eq!(id, "FAKE_DRIVE_FOLDER_ID_FOR_TESTS_0001");
            }
            _ => panic!("expected GoogleDrive folder"),
        }
    }

    #[test]
    fn still_classifies_google_slides() {
        let html = r#"<html><body><iframe src="https://docs.google.com/presentation/d/e/2PACX-ABC/pubembed"></iframe></body></html>"#;
        assert!(matches!(embeds(html)[0], Embed::GoogleSlides { .. }));
    }

    #[test]
    fn unknown_iframe_falls_through_to_iframe_variant() {
        let html = r#"<html><body><iframe src="https://example.com/foo"></iframe></body></html>"#;
        match &embeds(html)[0] {
            Embed::Iframe { src } => assert_eq!(src, "https://example.com/foo"),
            _ => panic!("expected Iframe variant"),
        }
    }
}
