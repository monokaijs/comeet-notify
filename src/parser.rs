use std::collections::BTreeMap;

use crate::models::GitLabWebhookEvent;

mod changes;

use changes::{compact, item_title, user_name};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitLabEventType {
    Push,
    MergeRequest,
    Issue,
    Pipeline,
    TagPush,
}

impl GitLabEventType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::MergeRequest => "merge_request",
            Self::Issue => "issue",
            Self::Pipeline => "pipeline",
            Self::TagPush => "tag_push",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedNotificationData {
    pub event_type: GitLabEventType,
    pub title: String,
    pub message: String,
    pub repository_name: String,
    pub repository_url: String,
    pub deep_link_data: BTreeMap<String, String>,
}

pub fn parse_event(payload: &GitLabWebhookEvent) -> Option<ParsedNotificationData> {
    match payload {
        GitLabWebhookEvent::Push(event) => {
            let branch_name = event.r#ref.replace("refs/heads/", "");
            let suffix = if event.total_commits_count == 1 {
                ""
            } else {
                "s"
            };
            let mut deep_link_data = BTreeMap::from([
                ("event_type".to_owned(), "push".to_owned()),
                ("project_id".to_owned(), event.project_id.to_string()),
            ]);
            if let Some(commit_sha) = &event.checkout_sha {
                deep_link_data.insert("commit_sha".to_owned(), commit_sha.clone());
            }
            Some(ParsedNotificationData {
                event_type: GitLabEventType::Push,
                title: format!("New push to {branch_name}"),
                message: format!(
                    "{} pushed {} commit{} to {} in {}",
                    event.user_name,
                    event.total_commits_count,
                    suffix,
                    branch_name,
                    event.project.name
                ),
                repository_name: event.project.name.clone(),
                repository_url: event.project.web_url.clone(),
                deep_link_data,
            })
        }
        GitLabWebhookEvent::MergeRequest(event) => {
            let mr = &event.object_attributes;
            let actor = if mr.system {
                "GitLab".to_owned()
            } else {
                user_name(&event.user)
            };
            let action = match mr.action.as_str() {
                "open"
                    if !mr.source_branch.trim().is_empty()
                        && !mr.target_branch.trim().is_empty() =>
                {
                    format!(
                        "opened merge request ({} → {})",
                        compact(&mr.source_branch, 40),
                        compact(&mr.target_branch, 40)
                    )
                }
                "open" => "opened merge request".to_owned(),
                "close" => "closed merge request".to_owned(),
                "reopen" => "reopened merge request".to_owned(),
                "merge" if !mr.target_branch.trim().is_empty() => {
                    format!("merged into {}", compact(&mr.target_branch, 60))
                }
                "merge" => "merged merge request".to_owned(),
                "approval" => "approved merge request".to_owned(),
                "approved" => "completed all required approvals".to_owned(),
                "unapproval" if mr.system => "reset an approval after new commits".to_owned(),
                "unapproval" => "removed their approval".to_owned(),
                "unapproved" if mr.system => "reset approvals after new commits".to_owned(),
                "unapproved" => "removed approval; merge request needs approval again".to_owned(),
                "update" => {
                    let mut details = changes::summarize(
                        &event.changes,
                        event.assignees.as_deref().unwrap_or_default(),
                        event.reviewers.as_deref().unwrap_or_default(),
                        true,
                    );
                    if mr
                        .oldrev
                        .as_ref()
                        .is_some_and(|revision| !revision.is_empty())
                    {
                        details.insert(0, "pushed new commits".to_owned());
                    }
                    changes::join(details)?
                }
                _ => return None,
            };
            Some(ParsedNotificationData {
                event_type: GitLabEventType::MergeRequest,
                title: item_title(&event.project.name, "MR", "!", mr.iid, &mr.title),
                message: compact(&format!("{actor} {action}"), 320),
                repository_name: event.project.name.clone(),
                repository_url: event.project.web_url.clone(),
                deep_link_data: BTreeMap::from([
                    ("event_type".to_owned(), "merge_request".to_owned()),
                    ("project_id".to_owned(), event.project.id.to_string()),
                    ("merge_request_iid".to_owned(), mr.iid.to_string()),
                ]),
            })
        }
        GitLabWebhookEvent::Issue(event) => {
            let issue = &event.object_attributes;
            let action = match issue.action.as_str() {
                "open" => "created issue".to_owned(),
                "close" => "closed issue".to_owned(),
                "reopen" => "reopened issue".to_owned(),
                "update" => changes::join(changes::summarize(
                    &event.changes,
                    event.assignees.as_deref().unwrap_or_default(),
                    &[],
                    false,
                ))?,
                _ => return None,
            };
            Some(ParsedNotificationData {
                event_type: GitLabEventType::Issue,
                title: item_title(&event.project.name, "Issue", "#", issue.iid, &issue.title),
                message: compact(&format!("{} {action}", user_name(&event.user)), 320),
                repository_name: event.project.name.clone(),
                repository_url: event.project.web_url.clone(),
                deep_link_data: BTreeMap::from([
                    ("event_type".to_owned(), "issue".to_owned()),
                    ("project_id".to_owned(), event.project.id.to_string()),
                    ("issue_iid".to_owned(), issue.iid.to_string()),
                ]),
            })
        }
        GitLabWebhookEvent::Pipeline(event) => {
            let pipeline = &event.object_attributes;
            let (title, message) = match pipeline.status.as_str() {
                "success" => (
                    "Pipeline succeeded".to_owned(),
                    format!("Pipeline for {} completed successfully", pipeline.r#ref),
                ),
                "failed" => (
                    "Pipeline failed".to_owned(),
                    format!("Pipeline for {} failed", pipeline.r#ref),
                ),
                "canceled" => (
                    "Pipeline canceled".to_owned(),
                    format!("Pipeline for {} was canceled", pipeline.r#ref),
                ),
                "running" => (
                    "Pipeline started".to_owned(),
                    format!("Pipeline for {} is now running", pipeline.r#ref),
                ),
                status => (
                    "Pipeline update".to_owned(),
                    format!("Pipeline for {} is {status}", pipeline.r#ref),
                ),
            };
            Some(ParsedNotificationData {
                event_type: GitLabEventType::Pipeline,
                title,
                message,
                repository_name: event.project.name.clone(),
                repository_url: event.project.web_url.clone(),
                deep_link_data: BTreeMap::from([
                    ("event_type".to_owned(), "pipeline".to_owned()),
                    ("project_id".to_owned(), event.project.id.to_string()),
                    ("pipeline_id".to_owned(), pipeline.id.to_string()),
                ]),
            })
        }
        GitLabWebhookEvent::TagPush(event) => {
            let tag_name = event.r#ref.replace("refs/tags/", "");
            Some(ParsedNotificationData {
                event_type: GitLabEventType::TagPush,
                title: "New tag created".to_owned(),
                message: format!(
                    "{} created tag {} in {}",
                    event.user_name, tag_name, event.project.name
                ),
                repository_name: event.project.name.clone(),
                repository_url: event.project.web_url.clone(),
                deep_link_data: BTreeMap::from([
                    ("event_type".to_owned(), "tag_push".to_owned()),
                    ("project_id".to_owned(), event.project_id.to_string()),
                ]),
            })
        }
        GitLabWebhookEvent::Job(_) | GitLabWebhookEvent::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(value: serde_json::Value) -> ParsedNotificationData {
        let event: GitLabWebhookEvent = serde_json::from_value(value).unwrap();
        parse_event(&event).unwrap()
    }

    #[test]
    fn parses_push_event() {
        let parsed = parse(json!({
            "object_kind": "push", "ref": "refs/heads/main", "checkout_sha": "abc",
            "user_name": "Mona", "project_id": 7, "total_commits_count": 2,
            "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"}
        }));
        assert_eq!(parsed.title, "New push to main");
        assert_eq!(parsed.message, "Mona pushed 2 commits to main in Comeet");
        assert_eq!(parsed.deep_link_data["commit_sha"], "abc");
    }

    #[test]
    fn uses_singular_commit_word() {
        let parsed = parse(json!({
            "object_kind": "push", "ref": "refs/heads/fix", "checkout_sha": "def",
            "user_name": "Mona", "project_id": 7, "total_commits_count": 1,
            "project": {"id": 7, "name": "Comeet", "web_url": ""}
        }));
        assert!(parsed.message.contains("1 commit to"));
        assert!(!parsed.message.contains("commits"));
    }

    #[test]
    fn accepts_null_checkout_sha_for_deleted_branch_push() {
        let parsed = parse(json!({
            "object_kind": "push", "ref": "refs/heads/deleted", "checkout_sha": null,
            "user_name": "Mona", "project_id": 7, "total_commits_count": 0,
            "project": {"id": 7, "name": "Comeet", "web_url": ""}
        }));
        assert!(!parsed.deep_link_data.contains_key("commit_sha"));
    }

    #[test]
    fn parses_each_merge_request_action() {
        let cases = [
            ("open", "opened merge request (feature → main)"),
            ("close", "closed merge request"),
            ("reopen", "reopened merge request"),
            ("merge", "merged into main"),
            ("approval", "approved merge request"),
            ("approved", "completed all required approvals"),
            ("unapproval", "removed their approval"),
            (
                "unapproved",
                "removed approval; merge request needs approval again",
            ),
        ];
        for (action, fragment) in cases {
            let parsed = parse(json!({
                "object_kind": "merge_request", "user": {"name": "Mona"},
                "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
                "object_attributes": {"iid": 12, "title": "Ship it", "source_branch": "feature", "target_branch": "main", "action": action}
            }));
            assert_eq!(parsed.title, "Comeet · MR !12: Ship it");
            assert_eq!(parsed.message, format!("Mona {fragment}"));
            assert_eq!(parsed.deep_link_data["merge_request_iid"], "12");
        }
    }

    #[test]
    fn parses_each_issue_action() {
        let cases = [
            ("open", "created issue"),
            ("close", "closed issue"),
            ("reopen", "reopened issue"),
        ];
        for (action, fragment) in cases {
            let parsed = parse(json!({
                "object_kind": "issue", "user": {"name": "Mona"},
                "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
                "object_attributes": {"iid": 13, "title": "Broken", "action": action}
            }));
            assert_eq!(parsed.title, "Comeet · Issue #13: Broken");
            assert_eq!(parsed.message, format!("Mona {fragment}"));
            assert_eq!(parsed.deep_link_data["issue_iid"], "13");
        }
    }

    #[test]
    fn parses_each_pipeline_status() {
        let cases = [
            ("success", "Pipeline succeeded", "completed successfully"),
            ("failed", "Pipeline failed", "failed"),
            ("canceled", "Pipeline canceled", "was canceled"),
            ("running", "Pipeline started", "is now running"),
            ("pending", "Pipeline update", "is pending"),
        ];
        for (status, title, fragment) in cases {
            let parsed = parse(json!({
                "object_kind": "pipeline",
                "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
                "object_attributes": {"id": 42, "ref": "main", "status": status, "stages": []},
                "builds": []
            }));
            assert_eq!(parsed.title, title);
            assert!(parsed.message.contains(fragment));
            assert_eq!(parsed.deep_link_data["pipeline_id"], "42");
        }
    }

    #[test]
    fn parses_tag_push_event() {
        let parsed = parse(json!({
            "object_kind": "tag_push", "ref": "refs/tags/v1.0", "user_name": "Mona",
            "project_id": 7, "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"}
        }));
        assert_eq!(parsed.title, "New tag created");
        assert_eq!(parsed.message, "Mona created tag v1.0 in Comeet");
    }

    #[test]
    fn acknowledges_unknown_event_without_parsing() {
        let event: GitLabWebhookEvent = serde_json::from_value(json!({
            "object_kind": "wiki_page", "project": {"id": 7}
        }))
        .unwrap();
        assert!(parse_event(&event).is_none());
    }
}
