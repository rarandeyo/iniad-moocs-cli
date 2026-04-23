//! imoocs の domain 型定義 — serde + schemars。
//!
//! JSON のキーは慣習として `camelCase`。詳細は plan §Output Schema を参照。

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

/// ページを列挙しない `course show` 用の軽量 lesson 参照。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LessonRef {
    pub year: Year,
    pub course_id: String,
    pub lesson_id: String,
    pub title: String,
    pub url: String,
    /// この lesson が属する章 / section 見出し (sidebar の grouping から取得)。省略可。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

/// コースの sidebar 上の section grouping (moocs-collect の LectureGroup 相当)。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LectureGroup {
    pub title: String,
    pub lessons: Vec<LessonRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CourseDetail {
    pub course: Course,
    pub lessons: Vec<LessonRef>,
    /// `lessons` と同じ lesson 群を sidebar の section (章立て) でグルーピングしたもの。
    pub groups: Vec<LectureGroup>,
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
    /// このページ上で検出した Problem ID 列 (`.problem-container[data-problem]` から取得)。
    /// 課題のないページでは空配列になる。
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
        /// `/file/d/<id>` なら `File`、`/drive/folders/<id>` なら `Folder`。
        #[serde(default)]
        kind: DriveKind,
        /// URL から抽出した Drive ID (fileId もしくは folderId)。
        #[serde(default)]
        id: String,
    },
    Iframe {
        src: String,
    },
}

/// `Embed::GoogleDrive` と `DriveItem` の種別判定。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DriveKind {
    #[default]
    File,
    Folder,
}

/// `drive list` の items 要素。
///
/// `mime == "application/vnd.google-apps.folder"` のときは `kind == Folder`。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriveItem {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub kind: DriveKind,
    /// RFC3339 形式の更新時刻。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}

/// `imoocs drive list` の envelope data。
///
/// `truncated` フィールドは v1.0 の HTML 50 件制限時代の名残で、envelope 後方互換
/// (DESIGN §5) のため残してある。v1.1~ で XHR pagination (`clients6.google.com/drive/v2beta/files`)
/// に移行し常に全件取得できるようになったため **常に `false`** を返す。
/// consumer はこのフィールドを見なくてよい (将来別フラグで予約する可能性のみ残す)。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriveFolderListing {
    pub folder_id: String,
    pub items: Vec<DriveItem>,
    pub truncated: bool,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriveFileFetchResult {
    pub file_id: String,
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    pub local_path: PathBuf,
    pub size_bytes: u64,
    pub fetched_at: String,
    pub from_cache: bool,
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

/// file 型 ProblemField に添付される、アップロード済みファイルのメタデータ。
///
/// server 側の `/answers` response は `{filename, filetype, timestamp}`
/// という形状 (filename のみ必須)。agent 向けに camelCase でシリアライズし、
/// デシリアライズは server key そのままで行う。`downloadUrl` は派生フィールドで、
/// `apply_answers` が値を埋める。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadedFile {
    /// server が記録しているオリジナルファイル名。
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filetype: Option<String>,
    /// アップロード時刻 (Unix timestamp、秒単位)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    /// 派生フィールド: `https://moocs.iniad.org/assignments/<y>/<c>/<p>/file/<pid>`。
    /// caller がまだ派生させていない場合は空文字。
    #[serde(default, skip_serializing_if = "String::is_empty")]
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
    /// `course-drive-folders.toml` が存在すれば件数サマリ、無ければ `None`。
    pub drive_folders: Option<DriveFoldersSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriveFoldersSummary {
    pub total: usize,
    pub resolved: usize,
    pub unresolved: usize,
}
