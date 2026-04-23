//! `/assignments/<year>/<course>/<problem>/*` endpoint 群の HTTP ラッパー。

use std::collections::HashMap;

use reqwest::{header, Method};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

use crate::error::{ImoocsError, Result};
use crate::schemas::{
    AnswerEntry, AnswerResult, Assessment, AssignmentDetail, AssignmentKey, AssignmentStatus, Lang, ProblemField,
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
    let resp = send_and_check(session.client.get(format!("{}/status", prefix(key))), "/status").await?;
    let v: StatusRaw = resp.json().await?;
    Ok(v.into_status())
}

pub async fn get_problem_html(session: &Session, key: &AssignmentKey, lang: Lang) -> Result<String> {
    let lang_str = match lang {
        Lang::Ja => "ja",
        Lang::En => "en",
    };
    let resp = send_and_check(
        session
            .client
            .get(format!("{}/problem", prefix(key)))
            .query(&[("lang", lang_str)]),
        "/problem",
    )
    .await?;
    let v: ProblemRaw = resp.json().await?;
    Ok(v.html)
}

pub async fn get_answers(session: &Session, key: &AssignmentKey) -> Result<HashMap<String, AnswerEntry>> {
    let resp = send_and_check(session.client.get(format!("{}/answers", prefix(key))), "/answers").await?;
    let raw: Value = resp.json().await?;
    debug!(?raw, "GET /answers raw body");
    // `/answers` は `{pid: {data, file, correct}, ...}` を返す。`$` で始まる
    // キーは server side のメタ情報 (例: `$network` は server から見たリクエスト
    // 元のネットワーク種別を示すだけで、それ自体はブロックではない) なので skip
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

    // MOOCs の browser render は `file.filetype` が set されていないと
    // 「未提出」と表示する (application.js の render_file)。Content-Type を
    // 拡張子から推定して Part にセットすると、server 側が `filetype` を
    // 記録してくれる。推定できなければ `application/octet-stream`。
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(filename)
        .mime_str(&mime)
        .map_err(|e| ImoocsError::Internal(format!("invalid mime {mime}: {e}")))?;

    let mut form = reqwest::multipart::Form::new().part("file", part);
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
    let resp = send_and_check(session.client.get(format!("{}/file/{}", prefix(key), pid)), "/file").await?;
    let bytes = resp.bytes().await?;
    Ok(bytes)
}

pub async fn get_assessment(session: &Session, key: &AssignmentKey) -> Result<Assessment> {
    let resp = send_and_check(session.client.get(format!("{}/assessment", prefix(key))), "/assessment").await?;
    let v: AssessmentRaw = resp.json().await?;
    Ok(Assessment {
        mark: v.mark.unwrap_or(0.0),
        full_mark: v.fullmark.unwrap_or(0.0),
        comment: v.comment.unwrap_or_default(),
    })
}

pub async fn get_assignment_detail(session: &Session, key: &AssignmentKey, lang: Lang) -> Result<AssignmentDetail> {
    let (status, html, answers) = futures::future::join3(
        get_status(session, key),
        get_problem_html(session, key, lang),
        get_answers(session, key),
    )
    .await;

    // NonPublic は 3 endpoint のどれでも独立に返しうるので、他のエラーより先に昇格する。
    // NetworkRestricted は現状 answers でしか観測されていないため据え置き (後続課題)。
    if let Err(ImoocsError::NonPublic { endpoint }) = &status {
        return Err(ImoocsError::NonPublic {
            endpoint: endpoint.clone(),
        });
    }
    if let Err(ImoocsError::NonPublic { endpoint }) = &html {
        return Err(ImoocsError::NonPublic {
            endpoint: endpoint.clone(),
        });
    }
    if let Err(ImoocsError::NonPublic { endpoint }) = &answers {
        return Err(ImoocsError::NonPublic {
            endpoint: endpoint.clone(),
        });
    }

    let status = status?;
    let html = html?;
    let answers = match answers {
        Ok(a) => a,
        Err(ImoocsError::NetworkRestricted) => return Err(ImoocsError::NetworkRestricted),
        Err(e) => return Err(e),
    };

    let mut fields: Vec<ProblemField> = parse_problem_form(&html);
    apply_answers(&mut fields, &answers, Some(key));

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

/// request を送り、check 済みの response を返す。HTTP 4xx/5xx は生の
/// reqwest エラーではなくドメインエラー (Auth / NotFound / Api / Network) に
/// 変換するので、CLI の exit code が正しい意味を持つ。
async fn send_and_check(req: reqwest::RequestBuilder, endpoint_hint: &str) -> Result<reqwest::Response> {
    let resp = req.send().await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.ok();
    Err(map_http_err_with_context(status, body, endpoint_hint))
}

fn map_http_err_with_context(status: reqwest::StatusCode, body: Option<String>, endpoint_hint: &str) -> ImoocsError {
    use reqwest::StatusCode;
    let msg_src = body
        .as_deref()
        .and_then(|b| if b.len() > 500 { None } else { Some(b) })
        .unwrap_or_default();
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FOUND => ImoocsError::Auth {
            reason: format!("HTTP {status} on {endpoint_hint}"),
            hint: Some("run `imoocs auth login`".into()),
        },
        StatusCode::FORBIDDEN => ImoocsError::NonPublic {
            endpoint: endpoint_hint.to_string(),
        },
        StatusCode::NOT_FOUND => ImoocsError::NotFound {
            what: format!(
                "{endpoint_hint} returned 404{}",
                if msg_src.is_empty() {
                    String::new()
                } else {
                    format!(": {msg_src}")
                }
            ),
        },
        StatusCode::SERVICE_UNAVAILABLE => {
            ImoocsError::Api(format!("service unavailable (503) on {endpoint_hint}: {msg_src}"))
        }
        _ => ImoocsError::Api(format!("HTTP {status} on {endpoint_hint}: {msg_src}")),
    }
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
        StatusCode::FORBIDDEN => ImoocsError::NonPublic {
            endpoint: "assignment".into(),
        },
        StatusCode::SERVICE_UNAVAILABLE => ImoocsError::Api(format!("service unavailable (503): {msg}")),
        StatusCode::NOT_FOUND => ImoocsError::NotFound {
            what: format!("assignment endpoint returned 404: {msg}"),
        },
        _ => ImoocsError::Api(format!("HTTP {status}: {msg}")),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn forbidden_maps_to_non_public_with_endpoint() {
        let err = map_http_err_with_context(StatusCode::FORBIDDEN, None, "/problem");
        match err {
            ImoocsError::NonPublic { endpoint } => assert_eq!(endpoint, "/problem"),
            other => panic!("expected NonPublic, got {other:?}"),
        }
    }

    #[test]
    fn forbidden_without_context_maps_to_non_public() {
        let err = map_http_err(StatusCode::FORBIDDEN, None);
        match err {
            ImoocsError::NonPublic { endpoint } => assert_eq!(endpoint, "assignment"),
            other => panic!("expected NonPublic, got {other:?}"),
        }
    }

    #[test]
    fn unauthorized_still_maps_to_auth() {
        let err = map_http_err_with_context(StatusCode::UNAUTHORIZED, None, "/status");
        assert!(matches!(err, ImoocsError::Auth { .. }));
    }

    #[test]
    fn not_found_still_maps_to_not_found() {
        let err = map_http_err_with_context(StatusCode::NOT_FOUND, None, "/problem");
        assert!(matches!(err, ImoocsError::NotFound { .. }));
    }
}
