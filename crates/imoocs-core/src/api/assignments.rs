//! HTTP wrappers for `/assignments/<year>/<course>/<problem>/*` endpoints.

use std::collections::HashMap;

use reqwest::{header, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::debug;

use crate::error::{ImoocsError, Result};
use crate::schemas::{
    AnswerEntry, AnswerResult, Assessment, AssignmentDetail, AssignmentKey, AssignmentStatus,
    Lang, ProblemField,
};
use crate::scrape::problem_form::{apply_answers, parse_problem_form};
use crate::session::{moocs_url, Session};

fn prefix(key: &AssignmentKey) -> String {
    moocs_url(&format!(
        "/assignments/{}/{}/{}",
        key.year, key.course_id, key.problem_id
    ))
}

pub async fn get_status(session: &Session, key: &AssignmentKey) -> Result<AssignmentStatus> {
    let resp = session
        .client
        .get(format!("{}/status", prefix(key)))
        .send()
        .await?
        .error_for_status()?;
    let v: StatusRaw = resp.json().await?;
    Ok(v.into_status())
}

pub async fn get_problem_html(session: &Session, key: &AssignmentKey, lang: Lang) -> Result<String> {
    let lang_str = match lang {
        Lang::Ja => "ja",
        Lang::En => "en",
    };
    let resp = session
        .client
        .get(format!("{}/problem", prefix(key)))
        .query(&[("lang", lang_str)])
        .send()
        .await?
        .error_for_status()?;
    let v: ProblemRaw = resp.json().await?;
    Ok(v.html)
}

pub async fn get_answers(
    session: &Session,
    key: &AssignmentKey,
) -> Result<HashMap<String, AnswerEntry>> {
    let resp = session
        .client
        .get(format!("{}/answers", prefix(key)))
        .send()
        .await?
        .error_for_status()?;
    let raw: Value = resp.json().await?;
    debug!(?raw, "GET /answers raw body");
    // `/answers` returns `{pid: {data, file, correct}, ...}`. Keys starting with
    // `$` are server-side metadata (e.g. `$network` exposes the request's
    // perceived network origin — NOT by itself a block). Skip those.
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return Ok(HashMap::new()),
    };
    let mut out = HashMap::new();
    for (k, v) in obj.iter() {
        if k.starts_with('$') {
            continue;
        }
        let entry: AnswerEntry = serde_json::from_value(v.clone()).unwrap_or(AnswerEntry {
            data: None,
            file: None,
            correct: None,
        });
        out.insert(k.clone(), entry);
    }
    Ok(out)
}

pub async fn put_answers(
    session: &Session,
    key: &AssignmentKey,
    data: HashMap<String, Value>,
    force: bool,
) -> Result<AnswerResult> {
    let mut body = json!({ "data": data });
    if force {
        body["force"] = Value::Bool(true);
    }
    let csrf = ensure_csrf(session).await?;
    let resp = session
        .client
        .request(Method::PUT, format!("{}/answers", prefix(key)))
        .header(header::CONTENT_TYPE, "application/json;charset=UTF-8")
        .header("X-CSRF-Token", csrf)
        .body(serde_json::to_vec(&body)?)
        .send()
        .await?;
    let status_code = resp.status();
    if !status_code.is_success() {
        return Err(map_http_err(status_code, resp.text().await.ok()));
    }
    // Re-fetch status for the response envelope.
    let stat = get_status(session, key).await?;
    Ok(AnswerResult {
        ok: true,
        status: stat,
        submitted: force,
        saved_at: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    })
}

pub async fn post_file(
    session: &Session,
    key: &AssignmentKey,
    pid: &str,
    path: &std::path::Path,
    force: bool,
) -> Result<()> {
    let csrf = ensure_csrf(session).await?;
    let file_bytes = tokio::fs::read(path).await?;
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("upload.bin")
        .to_string();

    let mut form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(file_bytes).file_name(filename),
        );
    if force {
        form = form.text("force", "true");
    }

    let resp = session
        .client
        .post(format!("{}/file/{}", prefix(key), pid))
        .header("X-CSRF-Token", csrf)
        .multipart(form)
        .send()
        .await?;
    let status_code = resp.status();
    if !status_code.is_success() {
        return Err(map_http_err(status_code, resp.text().await.ok()));
    }
    let _ = resp.bytes().await?;
    Ok(())
}

pub async fn get_file(session: &Session, key: &AssignmentKey, pid: &str) -> Result<bytes::Bytes> {
    let resp = session
        .client
        .get(format!("{}/file/{}", prefix(key), pid))
        .send()
        .await?
        .error_for_status()?;
    let bytes = resp.bytes().await?;
    Ok(bytes)
}

pub async fn get_assessment(session: &Session, key: &AssignmentKey) -> Result<Assessment> {
    let resp = session
        .client
        .get(format!("{}/assessment", prefix(key)))
        .send()
        .await?
        .error_for_status()?;
    let v: AssessmentRaw = resp.json().await?;
    Ok(Assessment {
        mark: v.mark.unwrap_or(0.0),
        full_mark: v.fullmark.unwrap_or(0.0),
        comment: v.comment.unwrap_or_default(),
    })
}

/// Fetch problem HTML + answers in parallel, build AssignmentDetail.
pub async fn get_assignment_detail(
    session: &Session,
    key: &AssignmentKey,
    lang: Lang,
) -> Result<AssignmentDetail> {
    let (status, html, answers) = futures::future::join3(
        get_status(session, key),
        get_problem_html(session, key, lang),
        get_answers(session, key),
    )
    .await;

    let status = status?;
    let html = html?;
    let answers = match answers {
        Ok(a) => a,
        Err(ImoocsError::NetworkRestricted) => return Err(ImoocsError::NetworkRestricted),
        Err(e) => return Err(e),
    };

    let mut fields: Vec<ProblemField> = parse_problem_form(&html);
    apply_answers(&mut fields, &answers);

    Ok(AssignmentDetail {
        year: key.year,
        course_id: key.course_id.clone(),
        problem_id: key.problem_id.clone(),
        status,
        lang: match lang {
            Lang::Ja => "ja".into(),
            Lang::En => "en".into(),
        },
        fields,
    })
}

async fn ensure_csrf(session: &Session) -> Result<String> {
    if let Some(t) = session.csrf_token().await {
        return Ok(t);
    }
    refresh_csrf(session).await
}

async fn refresh_csrf(session: &Session) -> Result<String> {
    let body = session
        .client
        .get(moocs_url("/account"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let doc = scraper::Html::parse_document(&body);
    let sel = crate::util::html::parse_selector(r#"meta[name="csrf-token"]"#)?;
    let content = doc
        .select(&sel)
        .next()
        .and_then(|el| el.value().attr("content"))
        .ok_or_else(|| ImoocsError::Parse("csrf-token meta not found on /account".into()))?
        .to_string();
    session.set_csrf_token(Some(content.clone())).await;
    debug!("refreshed CSRF token ({} chars)", content.len());
    Ok(content)
}

fn map_http_err(status: reqwest::StatusCode, body: Option<String>) -> ImoocsError {
    use reqwest::StatusCode;
    let msg = body
        .and_then(|b| if b.len() > 500 { None } else { Some(b) })
        .unwrap_or_default();
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FOUND => ImoocsError::Auth {
            reason: format!("HTTP {status}"),
            hint: Some("run `imoocs auth login`".into()),
        },
        StatusCode::SERVICE_UNAVAILABLE => {
            ImoocsError::Api(format!("service unavailable (503): {msg}"))
        }
        StatusCode::NOT_FOUND => ImoocsError::NotFound {
            what: format!("assignment endpoint returned 404: {msg}"),
        },
        _ => ImoocsError::Api(format!("HTTP {status}: {msg}")),
    }
}

// -------- wire types --------
#[derive(Deserialize)]
struct StatusRaw {
    status: String,
}

impl StatusRaw {
    fn into_status(self) -> AssignmentStatus {
        match self.status.as_str() {
            "open" => AssignmentStatus::Open,
            "closed" => AssignmentStatus::Closed,
            "graded" => AssignmentStatus::Graded,
            "network" => AssignmentStatus::Network,
            "error" => AssignmentStatus::Error,
            _ => AssignmentStatus::NonPublic,
        }
    }
}

#[derive(Deserialize)]
struct ProblemRaw {
    html: String,
}

#[derive(Deserialize)]
struct AssessmentRaw {
    mark: Option<f64>,
    fullmark: Option<f64>,
    comment: Option<String>,
}

// Dummy to silence unused warning if Serialize is ever required.
#[derive(Serialize)]
struct _Unused;
