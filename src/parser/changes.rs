use std::collections::BTreeSet;

use serde_json::Value;

use crate::models::GitLabUser;

pub(super) fn compact(text: &str, limit: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= limit {
        text
    } else {
        text.chars().take(limit - 1).chain(['…']).collect()
    }
}

pub(super) fn user_name(user: &GitLabUser) -> String {
    if !user.name.trim().is_empty() {
        compact(&user.name, 40)
    } else if !user.username.trim().is_empty() {
        format!("@{}", compact(&user.username, 39))
    } else {
        "Someone".to_owned()
    }
}

pub(super) fn item_title(project: &str, kind: &str, marker: &str, iid: i64, title: &str) -> String {
    let item = format!("{kind} {marker}{iid}: {}", compact(title, 70));
    if project.trim().is_empty() {
        item
    } else {
        compact(&format!("{} · {item}", compact(project, 32)), 120)
    }
}

// GitLab also uses [previous, current] pairs for reviewer state changes.
fn change<'a>(changes: &'a Value, keys: &[&str]) -> Option<(&'a Value, &'a Value)> {
    keys.iter().find_map(|key| {
        let value = changes.get(key)?;
        let (previous, current) = if value.is_array() {
            (value.get(0)?, value.get(1)?)
        } else {
            (value.get("previous")?, value.get("current")?)
        };
        (previous != current).then_some((previous, current))
    })
}

fn text(value: &Value) -> Option<String> {
    let text = value.as_str().or_else(|| value.get("name")?.as_str())?;
    (!text.trim().is_empty()).then(|| text.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn state(value: &Value, merge_request: bool) -> Option<String> {
    text(value).or_else(|| {
        Some(
            match value.as_i64()? {
                1 => "opened",
                2 => "closed",
                3 if merge_request => "merged",
                4 if merge_request => "locked",
                _ => return None,
            }
            .to_owned(),
        )
    })
}

fn entries(value: &Value) -> Vec<&Value> {
    if let Some(values) = value.as_array() {
        values.iter().collect()
    } else if value.is_null() {
        Vec::new()
    } else {
        vec![value]
    }
}

fn names(value: &Value, people: &[GitLabUser], labels: bool) -> Option<BTreeSet<String>> {
    entries(value)
        .into_iter()
        .map(|entry| {
            if labels {
                entry.get("title").and_then(text).or_else(|| text(entry))
            } else {
                let id = entry.as_i64().or_else(|| entry.get("id")?.as_i64());
                text(entry)
                    .or_else(|| {
                        entry
                            .get("username")
                            .and_then(text)
                            .map(|name| format!("@{name}"))
                    })
                    .or_else(|| {
                        people
                            .iter()
                            .find(|user| id.is_some() && user.id == id)
                            .map(user_name)
                    })
                    .or_else(|| id.map(|id| format!("user #{id}")))
            }
        })
        .collect()
}

fn named_change(
    changes: &Value,
    keys: &[&str],
    people: &[GitLabUser],
    labels: bool,
) -> Option<(BTreeSet<String>, BTreeSet<String>)> {
    let (previous, current) = change(changes, keys)?;
    Some((
        names(previous, people, labels)?,
        names(current, people, labels)?,
    ))
}

fn list(names: &BTreeSet<String>) -> String {
    let mut display = names.iter().take(2).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 2 {
        display.push_str(&format!(" +{} more", names.len() - 2));
    }
    compact(&display, 70)
}

pub(super) fn summarize(
    changes: &Value,
    assignees: &[GitLabUser],
    reviewers: &[GitLabUser],
    merge_request: bool,
) -> Vec<String> {
    let mut details = Vec::new();
    if let Some((_, current)) = change(changes, &["status", "state", "state_id"])
        && let Some(status) = state(current, merge_request)
    {
        details.push(format!("changed status to {status}"));
    }
    if let Some((previous, current)) = named_change(
        changes,
        &["assignees", "assignee_ids", "assignee_id"],
        assignees,
        false,
    ) {
        let added = current
            .difference(&previous)
            .cloned()
            .collect::<BTreeSet<_>>();
        let removed = previous
            .difference(&current)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !added.is_empty() {
            details.push(format!("assigned to {}", list(&added)));
        }
        if !removed.is_empty() {
            details.push(format!("unassigned {}", list(&removed)));
        }
    }
    if let Some((previous, current)) = named_change(changes, &["labels"], &[], true) {
        let added = current
            .difference(&previous)
            .cloned()
            .collect::<BTreeSet<_>>();
        let removed = previous
            .difference(&current)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !added.is_empty() {
            details.push(format!("added labels: {}", list(&added)));
        }
        if !removed.is_empty() {
            details.push(format!("removed labels: {}", list(&removed)));
        }
    }
    if merge_request {
        if let Some((_, current)) = change(changes, &["draft", "work_in_progress"])
            && let Some(draft) = current.as_bool()
        {
            details.push(
                if draft {
                    "marked as draft"
                } else {
                    "marked ready for review"
                }
                .to_owned(),
            );
        }
        if let Some((previous, current)) = change(changes, &["reviewers", "reviewer_ids"])
            && let (Some(previous_names), Some(current_names)) = (
                names(previous, reviewers, false),
                names(current, reviewers, false),
            )
        {
            if previous_names != current_names {
                let added = current_names
                    .difference(&previous_names)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let removed = previous_names
                    .difference(&current_names)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                if !added.is_empty() {
                    details.push(format!("requested review from {}", list(&added)));
                }
                if !removed.is_empty() {
                    details.push(format!("removed reviewers: {}", list(&removed)));
                }
            } else {
                // A re-request changes reviewer state without changing membership.
                let requested = entries(current)
                    .into_iter()
                    .filter(|reviewer| {
                        reviewer.get("re_requested").and_then(Value::as_bool) == Some(true)
                            && !entries(previous).contains(reviewer)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let requested =
                    names(&Value::Array(requested), reviewers, false).unwrap_or_default();
                if !requested.is_empty() {
                    details.push(format!("re-requested review from {}", list(&requested)));
                }
                for reviewer in entries(current) {
                    let old = entries(previous).into_iter().find(|old| {
                        reviewer.get("id").is_some() && old.get("id") == reviewer.get("id")
                    });
                    if old.is_some_and(|old| old.get("state") != reviewer.get("state"))
                        && reviewer.get("re_requested").and_then(Value::as_bool) != Some(true)
                    {
                        let review_state = match reviewer.get("state").and_then(Value::as_str) {
                            Some("requested_changes") => Some("changes requested"),
                            Some("approved") => Some("approved"),
                            Some("reviewed") => Some("reviewed"),
                            _ => None,
                        };
                        if let Some(review_state) = review_state
                            && let Some(who) = names(reviewer, reviewers, false)
                        {
                            details.push(format!(
                                "changed review from {} to {review_state}",
                                list(&who)
                            ));
                        }
                    }
                }
            }
        }
        if let Some((_, current)) = change(changes, &["merge_when_pipeline_succeeds"])
            && let Some(enabled) = current.as_bool()
        {
            details.push(
                if enabled {
                    "enabled auto-merge"
                } else {
                    "canceled auto-merge"
                }
                .to_owned(),
            );
        }
    }
    for (keys, label) in [
        (&["title"][..], "title"),
        (&["target_branch"][..], "target branch"),
        (&["milestone", "milestone_id"][..], "milestone"),
        (&["due_date"][..], "due date"),
        (&["weight"][..], "weight"),
    ] {
        if let Some((_, current)) = change(changes, keys) {
            if current.is_null() || current.as_str() == Some("") {
                details.push(format!("removed {label}"));
            } else if let Some(value) = text(current).or_else(|| {
                current.as_i64().map(|id| {
                    if label == "milestone" {
                        format!("#{id}")
                    } else {
                        id.to_string()
                    }
                })
            }) {
                details.push(format!("changed {label} to {value}"));
            }
        }
    }
    if let Some((_, current)) = change(changes, &["confidential"])
        && let Some(confidential) = current.as_bool()
    {
        details.push(
            if confidential {
                "made issue confidential"
            } else {
                "removed issue confidentiality"
            }
            .to_owned(),
        );
    }
    if change(changes, &["description"]).is_some() {
        details.push("edited description".to_owned());
    }
    details
}

pub(super) fn join(details: Vec<String>) -> Option<String> {
    if details.is_empty() {
        return None;
    }
    let mut summary = details
        .iter()
        .take(3)
        .map(|detail| compact(detail, 85))
        .collect::<Vec<_>>()
        .join("; ");
    if details.len() > 3 {
        let more = details.len() - 3;
        summary.push_str(&format!(
            "; +{more} more change{}",
            if more == 1 { "" } else { "s" }
        ));
    }
    Some(summary)
}
