//! Scrape the main content of a lesson page: markdown body + embedded iframes.

use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{ElementRef, Html};

use crate::error::Result;
use crate::schemas::Embed;
use crate::util::html::parse_selector;

static SLIDES_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^https://docs\.google\.com/(a/[^/]+/)?presentation/d/(e/)?[^/?]+/(embed|pubembed)",
    )
    .unwrap()
});
static DRIVE_FILE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^https://drive\.google\.com/file/d/[^/]+/preview").unwrap());

pub struct LessonContentRaw {
    pub title: String,
    pub markdown: String,
    pub embeds: Vec<Embed>,
    pub has_problem: bool,
    /// Problem IDs found via `.problem-container[data-problem]`.
    pub assignments: Vec<String>,
}

/// Extract a lesson page's human-visible title, markdown body, iframe embeds,
/// and whether a `.problem-container` is present.
pub fn scrape_lesson_content(html: &str) -> Result<LessonContentRaw> {
    let doc = Html::parse_document(html);

    // Title: prefer the content-header h1, fall back to <title>
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

    // Main content area: main article-ish region
    let main_sel = parse_selector(".content-wrapper, main, section.content, .content, body")?;
    let main_root = doc.select(&main_sel).next().unwrap_or_else(|| doc.root_element());

    // Markdown: take the .markdown-block elements and concatenate their rough markdown
    let md_sel = parse_selector(".markdown-block")?;
    let mut markdown = String::new();
    for block in doc.select(&md_sel) {
        let rendered = crude_markdown_from_element(&block);
        if !markdown.is_empty() {
            markdown.push_str("\n\n");
        }
        markdown.push_str(rendered.trim());
    }

    // Embeds: enumerate iframes inside main content, dedupe consecutive, skip helper frames.
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
        // Skip the cookie-helper that appears on every lesson
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
        } else if DRIVE_FILE_RE.is_match(&src) {
            embeds.push(Embed::GoogleDrive { embed_url: src });
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

/// Very small HTML → Markdown reducer for the `.markdown-block` author-authored
/// snippets used on INIAD MOOCs. Not a general Markdown renderer.
fn crude_markdown_from_element(el: &ElementRef<'_>) -> String {
    let mut out = String::new();
    render_node(*el, &mut out);
    // Collapse >=3 consecutive blank lines
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
    // Replace trailing /embed or /pubembed with /export/<format>
    let re = Regex::new(r"/(pubembed|embed)(\?.*)?$").unwrap();
    re.replace(embed_url, format!("/export/{format}")).into_owned()
}
