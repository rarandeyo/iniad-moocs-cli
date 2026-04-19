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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Embed {
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentSummary {
    pub year: Year,
    pub course_id: String,
    pub problem_id: String,
    pub page_id: String,
    pub status: AssignmentStatus,
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
    Textarea {
        pid: String,
        label: String,
        current_value: Option<String>,
    },
    Text {
        pid: String,
        label: String,
        current_value: Option<String>,
    },
    Radio {
        pid: String,
        label: String,
        options: Vec<RadioOption>,
        current_value: Option<String>,
    },
    Checkbox {
        pid: String,
        label: String,
        options: Vec<RadioOption>,
        current_value: Option<String>,
    },
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
