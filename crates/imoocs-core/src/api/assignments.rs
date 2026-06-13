//! `/assignments/<year>/<course>/<problem>/*` endpoint 群の HTTP ラッパー。

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;
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

/// `data-urlprefix` 属性の値 (= `/assignments/<year>/<courseId>/<problemId>`)。
fn data_urlprefix(key: &AssignmentKey) -> String {
    format!("/assignments/{}/{}/{}", key.year, key.course_id, key.problem_id)
}

/// 課題ページに navigate して textarea を fill、`button.submit-answer` を click。
///
/// `page_url`: 課題が載っている lesson page の URL (例 `.../courses/2026/INI301/AI-s01/09`)。
/// `data` の値は agent-browser navigate で扱うため文字列化する (`String` → そのまま、
/// `Number`/`Bool` → `to_string()`、`Array`/`Object`/`Null` → 現状は textarea 想定外なので
/// `Validation` エラー)。
pub async fn put_answers(
    session: &Session,
    key: &AssignmentKey,
    page_url: &str,
    data: HashMap<String, Value>,
    force: bool,
) -> Result<AnswerResult> {
    let answers = data
        .into_iter()
        .map(|(k, v)| match v {
            Value::String(s) => Ok((k, s)),
            Value::Bool(b) => Ok((k, b.to_string())),
            Value::Number(n) => Ok((k, n.to_string())),
            Value::Null => Ok((k, String::new())),
            other => Err(ImoocsError::Validation(format!(
                "pid `{k}` の値 {other} は textarea/text 答案にできません"
            ))),
        })
        .collect::<Result<HashMap<String, String>>>()?;

    let binary = crate::api::agent_binary()?;
    let urlprefix = data_urlprefix(key);
    imoocs_browser::commands::assignment_write::submit_answer(&binary, page_url, &urlprefix, &answers)
        .await
        .map_err(crate::api::map_browser_err)?;

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

/// 課題ページに navigate してファイルを `<input type=file name=<pid>>` に
/// upload、`button.submit-answer` を click。`force` は agent-browser flow では使わない
/// (ブラウザの確認 dialog は内部 JS が auto-accept する想定。実機で観察済)。
pub async fn post_file(
    _session: &Session,
    key: &AssignmentKey,
    page_url: &str,
    pid: &str,
    path: &std::path::Path,
    _force: bool,
) -> Result<()> {
    let binary = crate::api::agent_binary()?;
    let urlprefix = data_urlprefix(key);
    imoocs_browser::commands::assignment_write::upload_file(&binary, page_url, &urlprefix, pid, path)
        .await
        .map_err(crate::api::map_browser_err)?;
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

// write 系は agent-browser navigate 経由のため、CSRF token は
// 不要になった (MOOCs JS が `meta[name=csrf-token]` を自動付与する)。
// もし将来 reqwest write を復活させるなら HEAD コミットの `ensure_csrf` /
// `refresh_csrf` を参照する。

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
