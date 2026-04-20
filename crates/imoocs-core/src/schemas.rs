//! Domain types for imoocs — serde + schemars.
//!
//! JSON keys are `camelCase` by convention (see plan §Output Schema).

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type Year = u32;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Course {
    pub year: Year,
    pub course_id: String,
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub year: Year,
    pub course_id: String,
    pub lesson_id: String,
    pub title: String,
    pub pages: Vec<Page>,
}

/// Lightweight lesson reference used by `course show` where page enumeration is not performed.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LessonRef {
    pub year: Year,
    pub course_id: String,
    pub lesson_id: String,
    pub title: String,
    pub url: String,
    /// Optional chapter/section heading the lesson belongs to (from sidebar grouping).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CourseDetail {
    pub course: Course,
    pub lessons: Vec<LessonRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub page_id: String,
    pub title: String,
    pub url: String,
    pub has_problem: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LessonContent {
    pub year: Year,
    pub course_id: String,
    pub lesson_id: String,
    pub page_id: String,
    pub title: String,
    pub markdown: String,
    pub embeds: Vec<Embed>,
    /// Problem IDs found on this page (from `.problem-container[data-problem]`).
    /// Empty for pages without assignments.
    #[serde(default)]
    pub assignments: Vec<String>,
}

/// `lesson show --with-assignments` や `imoocs open <lesson-url>` で返る合成ビュー。
/// 各 problem_id を AssignmentDetail に展開して同梱する。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LessonWithAssignments {
    pub lesson: LessonContent,
    /// `lesson.assignments` と同じ順序で各 problem_id の詳細を返す。
    /// 個別の expansion に失敗した場合はその要素を null にする。
    pub assignments: Vec<Option<AssignmentDetail>>,
}

/// `imoocs open <url>` の envelope data。URL の種類に応じて中身が切り替わる。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum OpenResult {
    /// `/courses[/<year>]` — コース一覧
    Courses { year: Year, courses: Vec<Course> },
    /// `/courses/<year>/<courseId>` — コース詳細
    Course(CourseDetail),
    /// `/courses/<year>/<courseId>/<lessonId>[/<pageId>]` — レッスン本文 + 全課題詳細
    Lesson(LessonWithAssignments),
    /// `/assignments/<year>/<courseId>/<problemId>/...` — 単一課題の詳細
    Assignment(AssignmentDetail),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Embed {
    #[serde(rename_all = "camelCase")]
    GoogleSlides {
        embed_url: String,
        export_pdf_url: String,
        export_pptx_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        local_pdf_path: Option<PathBuf>,
        #[serde(skip_serializing_if = "Option::is_none")]
        size_bytes: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        page_count: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        fetched_at: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    GoogleDrive {
        embed_url: String,
    },
    Iframe {
        src: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AssignmentStatus {
    Open,
    Closed,
    Graded,
    Network,
    Error,
    NonPublic,
}

/// `assignment list --status <filter>` で絞るための派生ステータス。
///
/// - `status==open` かつ **全 pid に currentValue or uploadedFile** → Submitted
/// - `status==open` かつ **1 つでも未入力** → Pending
/// - それ以外は元の AssignmentStatus から対応づけ。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DerivedStatus {
    Pending,
    Submitted,
    Closed,
    Graded,
    Network,
    Error,
    NonPublic,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentSummary {
    pub year: Year,
    pub course_id: String,
    pub problem_id: String,
    pub page_id: String,
    pub status: AssignmentStatus,
    /// Derived filter-friendly status (Pending/Submitted/…)
    pub derived_status: DerivedStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lesson_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RadioOption {
    pub value: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadedFile {
    pub name: String,
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProblemField {
    #[serde(rename_all = "camelCase")]
    Textarea {
        pid: String,
        label: String,
        current_value: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Text {
        pid: String,
        label: String,
        current_value: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Radio {
        pid: String,
        label: String,
        options: Vec<RadioOption>,
        current_value: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Checkbox {
        pid: String,
        label: String,
        options: Vec<RadioOption>,
        current_value: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    File {
        pid: String,
        label: String,
        accept: Option<String>,
        uploaded_file: Option<UploadedFile>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentDetail {
    pub year: Year,
    pub course_id: String,
    pub problem_id: String,
    pub status: AssignmentStatus,
    pub lang: String,
    pub fields: Vec<ProblemField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnswerResult {
    pub ok: bool,
    pub status: AssignmentStatus,
    pub submitted: bool,
    pub saved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    pub mark: f64,
    pub full_mark: f64,
    pub comment: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Ja,
    En,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentKey {
    pub year: Year,
    pub course_id: String,
    pub problem_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnswerEntry {
    pub data: Option<serde_json::Value>,
    pub file: Option<UploadedFile>,
    pub correct: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub version: String,
    pub moocs_authenticated: bool,
    pub google_authenticated: bool,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub username: Option<String>,
}
