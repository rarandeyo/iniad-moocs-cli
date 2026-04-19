//! Parse the `/assignments/<...>/problem` HTML fragment into a typed list of
//! [`ProblemField`]s.
//!
//! Inputs observed so far:
//! - `<textarea name="pid">`  → `ProblemField::Textarea`
//! - `<input type="text" name="pid">` / `input type="number"` → `Text`
//! - `<input type="radio" name="pid" value="...">`   — one pid with many radios
//! - `<input type="checkbox" name="pid" value="...">` — same shape as radio
//! - `<input type="file" name="pid" accept="...">`  → `File`
//!
//! Question labels are the `<label>` / `<p>` text that immediately precedes a
//! non-radio-inline field. Radio/checkbox option texts are taken from the
//! surrounding `<label class="radio-inline">` (stripping leading whitespace).

use std::collections::BTreeMap;

use scraper::{ElementRef, Html, Node};

use crate::schemas::{ProblemField, RadioOption};

pub fn parse_problem_form(html: &str) -> Vec<ProblemField> {
    let doc = Html::parse_fragment(html);
    let root = doc.root_element();

    // Walk descendants in document order; keep a sliding `last_label` as context.
    let mut fields: Vec<ProblemField> = Vec::new();
    let mut radio_groups: BTreeMap<String, RadioGroup> = BTreeMap::new();
    let mut last_label: Option<String> = None;

    for descendant in root.descendants() {
        let Some(el) = ElementRef::wrap(descendant) else { continue };
        let name = el.value().name();

        match name {
            // Track potential question labels — but skip wrappers that contain an input
            // (those are per-option labels, handled separately).
            "label" | "p" | "h3" | "h4" | "h5" | "h6" => {
                if contains_input(&el) {
                    continue;
                }
                let text = el.text().collect::<String>().trim().to_string();
                if !text.is_empty() && text.len() < 500 {
                    last_label = Some(text);
                }
            }
            "textarea" => {
                if let Some(pid) = el.value().attr("name") {
                    let pid = pid.to_string();
                    let label = last_label.clone().unwrap_or_else(|| pid.clone());
                    fields.push(ProblemField::Textarea {
                        pid,
                        label,
                        current_value: None,
                    });
                    last_label = None;
                }
            }
            "input" => {
                let ty = el.value().attr("type").unwrap_or("text").to_lowercase();
                let pid = match el.value().attr("name") {
                    Some(p) => p.to_string(),
                    None => continue,
                };

                match ty.as_str() {
                    "text" | "number" | "email" | "url" => {
                        let label = last_label.clone().unwrap_or_else(|| pid.clone());
                        fields.push(ProblemField::Text {
                            pid,
                            label,
                            current_value: None,
                        });
                        last_label = None;
                    }
                    "file" => {
                        let accept = el.value().attr("accept").map(str::to_string);
                        let label = last_label.clone().unwrap_or_else(|| pid.clone());
                        fields.push(ProblemField::File {
                            pid,
                            label,
                            accept,
                            uploaded_file: None,
                        });
                        last_label = None;
                    }
                    "radio" | "checkbox" => {
                        let value = el.value().attr("value").unwrap_or("").to_string();
                        let option_text = nearest_label_text(&el);
                        let entry = radio_groups.entry(pid.clone()).or_insert_with(|| RadioGroup {
                            pid: pid.clone(),
                            label: last_label.clone().unwrap_or_else(|| pid.clone()),
                            is_checkbox: ty == "checkbox",
                            options: Vec::new(),
                        });
                        entry.options.push(RadioOption {
                            value,
                            text: option_text,
                        });
                        // Consume last_label only for the first option of the group.
                        last_label = None;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Append radio/checkbox groups in the order we first saw their labels (BTreeMap
    // is by name, which is close enough for determinism).
    for (_, group) in radio_groups.into_iter() {
        let variant = if group.is_checkbox {
            ProblemField::Checkbox {
                pid: group.pid,
                label: group.label,
                options: group.options,
                current_value: None,
            }
        } else {
            ProblemField::Radio {
                pid: group.pid,
                label: group.label,
                options: group.options,
                current_value: None,
            }
        };
        fields.push(variant);
    }

    fields
}

struct RadioGroup {
    pid: String,
    label: String,
    is_checkbox: bool,
    options: Vec<RadioOption>,
}

fn contains_input(el: &ElementRef<'_>) -> bool {
    el.descendants().any(|n| {
        if let Node::Element(e) = n.value() {
            matches!(e.name(), "input" | "textarea" | "select")
        } else {
            false
        }
    })
}

/// For a radio/checkbox input, return the text of the enclosing `<label>` with
/// the input's own children removed. Falls back to the next sibling text node.
fn nearest_label_text(input: &ElementRef<'_>) -> String {
    // Walk up: find the closest <label> ancestor.
    let mut cur = input.parent();
    for _ in 0..4 {
        let Some(node) = cur else { break };
        if let Node::Element(e) = node.value() {
            if e.name() == "label" {
                if let Some(label_ref) = ElementRef::wrap(node) {
                    let mut text = String::new();
                    for child in label_ref.children() {
                        match child.value() {
                            Node::Text(t) => text.push_str(&t.text),
                            Node::Element(child_el) => {
                                // Skip the input and non-textual elements
                                if matches!(child_el.name(), "input" | "textarea" | "select") {
                                    continue;
                                }
                                if let Some(cref) = ElementRef::wrap(child) {
                                    text.push_str(&cref.text().collect::<String>());
                                }
                            }
                            _ => {}
                        }
                    }
                    let t = text.trim().to_string();
                    if !t.is_empty() {
                        return t;
                    }
                }
            }
        }
        cur = node.parent();
    }

    // Fallback: next sibling text node after the input
    for sib in input.next_siblings() {
        if let Node::Text(t) = sib.value() {
            let tr = t.text.trim();
            if !tr.is_empty() {
                return tr.to_string();
            }
        }
    }
    String::new()
}

/// Merge `/answers` response into the parsed fields. `current_value` is a JSON
/// scalar (string / array). For files, the answers response provides the
/// uploaded filename under an implementation-specific shape.
pub fn apply_answers(
    fields: &mut [ProblemField],
    answers: &std::collections::HashMap<String, crate::schemas::AnswerEntry>,
) {
    use serde_json::Value;
    for field in fields.iter_mut() {
        let pid = field_pid(field);
        let Some(entry) = answers.get(pid) else { continue };
        match field {
            ProblemField::Textarea { current_value, .. }
            | ProblemField::Text { current_value, .. }
            | ProblemField::Radio { current_value, .. }
            | ProblemField::Checkbox { current_value, .. } => {
                *current_value = match &entry.data {
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(Value::Array(arr)) => Some(
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                    ),
                    Some(other) => Some(other.to_string()),
                    None => None,
                };
            }
            ProblemField::File { uploaded_file, .. } => {
                *uploaded_file = entry.file.clone();
            }
        }
    }
}

fn field_pid(f: &ProblemField) -> &str {
    match f {
        ProblemField::Textarea { pid, .. }
        | ProblemField::Text { pid, .. }
        | ProblemField::Radio { pid, .. }
        | ProblemField::Checkbox { pid, .. }
        | ProblemField::File { pid, .. } => pid,
    }
}
