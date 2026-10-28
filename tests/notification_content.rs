use comeet_notify::{
    models::GitLabWebhookEvent,
    parser::{ParsedNotificationData, parse_event},
};
use serde_json::{Value, json};

fn event(kind: &str, changes: Value) -> Value {
    json!({
        "object_kind": kind,
        "user": {"id": 1, "name": "Mona", "username": "mona"},
        "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
        "object_attributes": {"iid": 13, "title": "Fix login", "action": "update"},
        "changes": changes
    })
}

fn parse(value: Value) -> Option<ParsedNotificationData> {
    let event: GitLabWebhookEvent = serde_json::from_value(value).unwrap();
    parse_event(&event)
}

#[test]
fn issue_status_change_shows_actor_new_status_and_issue_context() {
    let parsed = parse(event(
        "issue",
        json!({
            "state_id": {"previous": 1, "current": 2},
            "updated_at": {"previous": "yesterday", "current": "today"}
        }),
    ))
    .unwrap();
    assert_eq!(parsed.title, "Comeet · Issue #13: Fix login");
    assert_eq!(parsed.message, "Mona changed status to closed");
    assert_eq!(parsed.deep_link_data["event_type"], "issue");
    assert_eq!(parsed.deep_link_data["project_id"], "7");
    assert_eq!(parsed.deep_link_data["issue_iid"], "13");
}

#[test]
fn named_status_and_state_changes_are_readable_and_not_duplicated() {
    for (kind, changes, expected) in [
        (
            "issue",
            json!({"state": {"previous": "closed", "current": "opened"}, "state_id": {"previous": 2, "current": 1}}),
            "opened",
        ),
        (
            "issue",
            json!({"status": {"previous": {"name": "Todo"}, "current": {"name": "In progress"}}}),
            "In progress",
        ),
        (
            "merge_request",
            json!({"state_id": {"previous": 1, "current": 3}}),
            "merged",
        ),
    ] {
        assert_eq!(
            parse(event(kind, changes)).unwrap().message,
            format!("Mona changed status to {expected}")
        );
    }
}

#[test]
fn assignment_changes_include_people_and_removal() {
    for (key, previous, current, expected) in [
        (
            "assignees",
            json!([]),
            json!([{"name": "Alex"}]),
            "assigned to Alex",
        ),
        (
            "assignees",
            json!([{"name": "Alex"}]),
            json!([]),
            "unassigned Alex",
        ),
        (
            "assignees",
            json!([]),
            json!([{"username": "alex"}]),
            "assigned to @alex",
        ),
        ("assignee_id", json!(1), Value::Null, "unassigned user #1"),
    ] {
        let changes = json!({key: {"previous": previous, "current": current}});
        assert_eq!(
            parse(event("issue", changes)).unwrap().message,
            format!("Mona {expected}")
        );
    }
}

#[test]
fn assignment_ids_use_names_from_the_webhook_without_api_requests() {
    let mut payload = event(
        "issue",
        json!({
            "assignee_ids": {"previous": [], "current": [2, 3]}
        }),
    );
    payload["assignees"] = json!([{"id": 2, "name": "Alex"}, {"id": 3, "username": "sam"}]);
    assert_eq!(
        parse(payload).unwrap().message,
        "Mona assigned to @sam, Alex"
    );
}

#[test]
fn labels_show_only_added_and_removed_names() {
    let parsed = parse(event(
        "issue",
        json!({
            "labels": {
                "previous": [{"id": 1, "title": "bug"}, {"id": 2, "title": "status::Todo"}],
                "current": [{"id": 1, "title": "bug"}, {"id": 3, "title": "status::Doing"}]
            }
        }),
    ))
    .unwrap();
    assert_eq!(
        parsed.message,
        "Mona added labels: status::Doing; removed labels: status::Todo"
    );
}

#[test]
fn timestamp_only_empty_missing_and_unknown_changes_are_silent() {
    for kind in ["issue", "merge_request"] {
        for changes in [
            Value::Null,
            json!({}),
            json!({
                "updated_at": {"previous": "yesterday", "current": "today"},
                "updated_by_id": {"previous": 1, "current": 2},
                "last_edited_at": {"previous": "yesterday", "current": "today"},
                "merge_status": {"previous": "preparing", "current": "checking"}
            }),
            json!({"new_internal_field": {"previous": 1, "current": 2}}),
        ] {
            assert!(parse(event(kind, changes)).is_none());
        }
        let mut missing = event(kind, Value::Null);
        missing.as_object_mut().unwrap().remove("changes");
        assert!(parse(missing).is_none());
    }
}

#[test]
fn unchanged_values_reordered_labels_and_label_metadata_do_not_notify() {
    for changes in [
        json!({"description": {"previous": "same", "current": "same"}}),
        json!({"labels": {"previous": ["bug", "urgent"], "current": ["urgent", "bug"]}}),
        json!({"labels": {"previous": [{"title": "bug", "color": "red"}], "current": [{"title": "bug", "color": "blue"}]}}),
    ] {
        assert!(parse(event("issue", changes)).is_none());
    }
}

#[test]
fn malformed_change_details_do_not_reject_valid_lifecycle_events_or_invent_updates() {
    let mut payload = event(
        "issue",
        json!({"state": {"current": "closed"}, "assignees": false}),
    );
    assert!(parse(payload.clone()).is_none());
    payload["object_attributes"]["action"] = json!("open");
    payload["assignees"] = Value::Null;
    assert_eq!(parse(payload).unwrap().message, "Mona created issue");
}

#[test]
fn field_edits_describe_the_change_without_dumping_the_description() {
    for (key, previous, current, expected) in [
        (
            "title",
            json!("Old title"),
            json!("Fix login"),
            "changed title to Fix login",
        ),
        (
            "description",
            json!("old private text"),
            json!("new private text"),
            "edited description",
        ),
        (
            "due_date",
            Value::Null,
            json!("2026-11-03"),
            "changed due date to 2026-11-03",
        ),
        (
            "due_date",
            json!("2026-11-03"),
            Value::Null,
            "removed due date",
        ),
        (
            "milestone",
            Value::Null,
            json!({"name": "Launch"}),
            "changed milestone to Launch",
        ),
        ("weight", json!(1), json!(3), "changed weight to 3"),
        (
            "confidential",
            json!(false),
            json!(true),
            "made issue confidential",
        ),
    ] {
        let parsed = parse(event(
            "issue",
            json!({key: {"previous": previous, "current": current}}),
        ))
        .unwrap();
        assert_eq!(parsed.message, format!("Mona {expected}"));
    }
}

#[test]
fn multiple_changes_prioritize_status_and_keep_the_summary_short() {
    let parsed = parse(event(
        "issue",
        json!({
            "state_id": {"previous": 1, "current": 2},
            "assignees": {"previous": [], "current": [{"name": "Alex"}]},
            "title": {"previous": "Old", "current": "Fix login"},
            "description": {"previous": "old private text", "current": "new private text"}
        }),
    ))
    .unwrap();
    assert_eq!(
        parsed.message,
        "Mona changed status to closed; assigned to Alex; changed title to Fix login; +1 more change"
    );
    assert!(!parsed.message.contains("private text"));
}

#[test]
fn merge_request_edits_cover_draft_reviewers_target_branch_and_auto_merge() {
    for (key, previous, current, expected) in [
        (
            "draft",
            json!(true),
            json!(false),
            "marked ready for review",
        ),
        (
            "work_in_progress",
            json!(false),
            json!(true),
            "marked as draft",
        ),
        (
            "reviewers",
            json!([]),
            json!([{"name": "Alex"}]),
            "requested review from Alex",
        ),
        (
            "target_branch",
            json!("main"),
            json!("release"),
            "changed target branch to release",
        ),
        (
            "merge_when_pipeline_succeeds",
            json!(false),
            json!(true),
            "enabled auto-merge",
        ),
    ] {
        let parsed = parse(event(
            "merge_request",
            json!({key: {"previous": previous, "current": current}}),
        ))
        .unwrap();
        assert_eq!(parsed.title, "Comeet · MR !13: Fix login");
        assert_eq!(parsed.message, format!("Mona {expected}"));
        assert_eq!(parsed.deep_link_data["merge_request_iid"], "13");
    }
}

#[test]
fn reviewer_re_requests_use_gitlabs_array_pair_format() {
    let parsed = parse(event(
        "merge_request",
        json!({
            "reviewers": [
                [{"id": 2, "name": "Alex", "state": "approved", "re_requested": false}],
                [{"id": 2, "name": "Alex", "state": "unreviewed", "re_requested": true}]
            ]
        }),
    ))
    .unwrap();
    assert_eq!(parsed.message, "Mona re-requested review from Alex");
}

#[test]
fn new_commits_notify_even_when_the_changes_map_is_empty() {
    let mut payload = event("merge_request", json!({}));
    payload["object_attributes"]["oldrev"] = json!("abc123");
    assert_eq!(parse(payload).unwrap().message, "Mona pushed new commits");
}

#[test]
fn missing_names_use_username_or_someone_and_missing_project_omits_separator() {
    let mut payload = event(
        "issue",
        json!({"description": {"previous": "", "current": "text"}}),
    );
    payload["user"] = json!({"username": "mona"});
    payload["project"]["name"] = json!("");
    let parsed = parse(payload.clone()).unwrap();
    assert_eq!(parsed.title, "Issue #13: Fix login");
    assert_eq!(parsed.message, "@mona edited description");
    payload["user"] = json!({});
    assert_eq!(
        parse(payload).unwrap().message,
        "Someone edited description"
    );
}

#[test]
fn long_unicode_content_is_bounded_and_multiline_text_is_collapsed() {
    let mut payload = event(
        "issue",
        json!({"title": {"previous": "old", "current": "新\n".repeat(500)}}),
    );
    payload["object_attributes"]["title"] = json!("新\n".repeat(500));
    payload["user"]["name"] = json!("人\n".repeat(500));
    payload["project"]["name"] = json!("项目".repeat(500));
    let parsed = parse(payload).unwrap();
    assert!(parsed.title.chars().count() <= 120);
    assert!(parsed.title.contains("Issue #13"));
    assert!(parsed.message.chars().count() <= 320);
    assert!(!parsed.title.contains('\n'));
    assert!(!parsed.message.contains('\n'));
}

#[test]
fn long_labels_with_shared_prefixes_are_still_detected_as_changes() {
    let prefix = "a".repeat(100);
    assert!(
        parse(event(
            "issue",
            json!({"labels": {
                "previous": [format!("{prefix}old")], "current": [format!("{prefix}new")]
            }})
        ))
        .is_some()
    );
}

#[test]
fn malformed_lists_never_invent_assignment_or_label_removals() {
    for key in ["assignees", "labels", "reviewers"] {
        let payload = event(
            "merge_request",
            json!({key: {
                "previous": [{"name": "Alex", "title": "bug"}], "current": false
            }}),
        );
        assert!(parse(payload).is_none());
    }
}

#[test]
fn submitted_review_state_changes_are_specific() {
    let parsed = parse(event(
        "merge_request",
        json!({"reviewers": [
            [{"id": 2, "name": "Alex", "state": "review_started", "re_requested": false}],
            [{"id": 2, "name": "Alex", "state": "requested_changes", "re_requested": false}]
        ]}),
    ))
    .unwrap();
    assert_eq!(
        parsed.message,
        "Mona changed review from Alex to changes requested"
    );
}

#[test]
fn approval_resets_are_attributed_to_gitlab_instead_of_the_pusher() {
    let mut payload = event("merge_request", json!({}));
    payload["object_attributes"]["action"] = json!("unapproved");
    payload["object_attributes"]["system"] = json!(true);
    assert_eq!(
        parse(payload).unwrap().message,
        "GitLab reset approvals after new commits"
    );
}

#[test]
fn assignment_summaries_exclude_unchanged_people() {
    let parsed = parse(event(
        "issue",
        json!({"assignees": {
            "previous": [{"name": "Alex"}, {"name": "Sam"}],
            "current": [{"name": "Sam"}, {"name": "Lee"}]
        }}),
    ))
    .unwrap();
    assert_eq!(parsed.message, "Mona assigned to Lee; unassigned Alex");
}

#[test]
fn reviewer_summaries_exclude_unchanged_people() {
    let parsed = parse(event(
        "merge_request",
        json!({"reviewers": {
            "previous": [{"name": "Alex"}, {"name": "Sam"}],
            "current": [{"name": "Sam"}, {"name": "Lee"}]
        }}),
    ))
    .unwrap();
    assert_eq!(
        parsed.message,
        "Mona requested review from Lee; removed reviewers: Alex"
    );
}

#[test]
fn lifecycle_events_remain_readable_when_optional_metadata_is_missing() {
    for (action, expected) in [
        ("open", "Mona opened merge request"),
        ("merge", "Mona merged merge request"),
    ] {
        let mut payload = event("merge_request", Value::Null);
        payload["object_attributes"]["action"] = json!(action);
        assert_eq!(parse(payload).unwrap().message, expected);
    }
}
