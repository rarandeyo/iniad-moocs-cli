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
    { "type": "google-drive", "embedUrl": "..." },
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
| `network` (学内 IP 限定) | `network` |
| `error` | `error` |
| `nonpublic` (公開前) | `nonpublic` |

`assignment list --status pending` / `--status submitted` は `derivedStatus`
で絞る。`--status open` は Pending と Submitted を両方残す。
