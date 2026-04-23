# 出力 JSON スキーマ抜粋

すべて camelCase。top-level envelope はこの形:

```json
{ "success": true, "data": <T> }
{ "success": false, "error": { "code": "AUTH_EXPIRED",
                               "message": "...",
                               "hint": "run `imoocs auth login`" } }
```

## 主要データ型

### Course
```json
{
  "year": 2026,
  "courseId": "INI301",
  "name": "機械学習と人工知能",
  "url": "https://moocs.iniad.org/courses/2026/INI301"
}
```

### CourseDetail (`course show`)
```json
{
  "course": { ...Course... },
  "lessons": [ { "year": 2026, "courseId": "INI301",
                 "lessonId": "DS-00", "title": "...",
                 "url": "...", "section": "データ解析のための環境構築" } ],
  "groups": [ { "title": "データ解析のための環境構築",
                "lessons": [ ...LessonRef... ] } ]
}
```

### LessonContent (`lesson show`)
```json
{
  "year": 2026, "courseId": "INI301",
  "lessonId": "AI-s02", "pageId": "09",
  "title": "AI-s02: numpy / pandas 2",
  "markdown": "...",
  "embeds": [
    { "type": "google-slides",
      "embedUrl": "...", "exportPdfUrl": "...", "exportPptxUrl": "...",
      "localPdfPath": "/home/.../slides/<sha1>.pdf", "pageCount": 16,
      "fetchedAt": "2026-04-20T00:00:00Z" },
    { "type": "google-drive", "kind": "file",
      "id": "FAKE_DRIVE_FILE_ID_FOR_TESTS_0001",
      "embedUrl": "https://drive.google.com/file/d/FAKE_DRIVE_FILE_ID_FOR_TESTS_0001/preview" },
    { "type": "iframe", "src": "..." }
  ],
  "assignments": ["ai-s02-assign2"]
}
```

### LessonWithAssignments (`lesson show --with-assignments` / `open` lesson)
```json
{
  "lesson": { ...LessonContent... },
  "assignments": [ { ...AssignmentDetail... } | null, ... ]
}
```

### AssignmentSummary (`assignment list`)
```json
{
  "year": 2026, "courseId": "COS201", "problemId": "assignment-02",
  "pageId": "exercise", "lessonId": "02",
  "status": "closed",
  "derivedStatus": "closed"
}
```

### AssignmentDetail (`assignment show`)
```json
{
  "year": 2026, "courseId": "INI301", "problemId": "ai-s02-assign2",
  "status": "open", "lang": "ja",
  "fields": [
    { "type": "textarea", "pid": "p1", "label": "...",
      "currentValue": "draft..." },
    { "type": "text", "pid": "p2", "label": "...",
      "currentValue": null },
    { "type": "radio", "pid": "p3", "label": "...",
      "options": [ {"value": "OK", "text": "3D 表示された"},
                   {"value": "NG", "text": "..."} ],
      "currentValue": null },
    { "type": "checkbox", "pid": "p4", "label": "...",
      "options": [...], "currentValue": null },
    { "type": "file", "pid": "ipynb", "label": "ipynb",
      "accept": null, "uploadedFile": null }
  ]
}
```

### AnswerResult (`answer` / `submit`)
```json
{ "ok": true, "status": "open",
  "submitted": false, "savedAt": "2026-04-20T00:00:00Z" }
```

### OpenResult (`imoocs open`)
tag-based enum with `type`:
- `"courses"`: `{ type, year, courses: [Course, ...] }`
- `"course"`: `{ type, course, lessons, groups }`
- `"lesson"`: `{ type, lesson, assignments: [AssignmentDetail|null, ...] }`
- `"assignment"`: `{ type, ...AssignmentDetail }` (単一課題)

## AssignmentStatus vs DerivedStatus

| AssignmentStatus (サーバ側 `/status`) | DerivedStatus (派生フィルタ) |
|---|---|
| `open` (受付中) | `pending` (未入力) or `submitted` (全 pid 埋まっている) |
| `closed` (期間終了) | `closed` |
| `graded` (採点済み) | `graded` |
| `network` (学内 IP 限定; 出席確認と一部のみ) | `network` |
| `error` | `error` |
| `nonpublic` (公開前) | `nonpublic` |

`assignment list --status pending` / `--status submitted` は `derivedStatus`
で絞る。`--status open` は Pending と Submitted を両方残す。

## Drive

### DriveItem (`drive list` の items 要素)
```json
{
  "id": "FAKE_DRIVE_FILE_ID_FOR_TESTS_0001",
  "name": "ai-01.zip",
  "mime": "application/x-zip-compressed",
  "kind": "file",
  "modifiedAt": "2025-04-05T23:20:49.091Z"
}
```
`mime == "application/vnd.google-apps.folder"` のときだけ `kind == "folder"`。
`modifiedAt` は Drive UI が持つミリ秒 timestamp を RFC3339 に変換したもの。

### DriveFolderListing (`drive list`)
```json
{
  "folderId": "FAKE_DRIVE_FOLDER_ID_FOR_TESTS_0001",
  "items": [ /* DriveItem */ ],
  "truncated": false,
  "fetchedAt": "2026-04-22T00:00:00Z"
}
```
`truncated: true` は初期 HTML に 50 件ちょうどで切れた印 — それ以上は
本 CLI では取れない (v2 でページング予定)。

### DriveFileFetchResult (`drive fetch`)
```json
{
  "fileId": "FAKE_DRIVE_FILE_ID_FOR_TESTS_0001",
  "filename": "ai-01.zip",
  "mime": "application/octet-stream",
  "localPath": "/home/<user>/.cache/imoocs/drive/FAKE_DRIVE_FILE_ID_FOR_TESTS_0001.zip",
  "sizeBytes": 99655,
  "fetchedAt": "2026-04-22T07:17:02.507682872Z",
  "fromCache": false
}
```
`localPath` は `$XDG_CACHE_HOME/imoocs/drive/<fileId>.<ext>`。
拡張子は Content-Disposition の filename から決定。
`mime` は Drive が返した Content-Type (`application/octet-stream` が多い)。
