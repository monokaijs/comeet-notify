use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::models::{PipelineBuild, PipelineEvent};

const MAX_VISIBLE_STAGES: usize = 6;
const ACTIVE_STALE_AFTER_SECONDS: i64 = 15 * 60;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StageStatus {
    Success,
    Failed,
    Running,
    Pending,
    Canceled,
    Skipped,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveActivityStage {
    pub name: String,
    pub status: StageStatus,
    pub completed_job_count: usize,
    pub total_job_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveActivityContentState {
    pub status: String,
    pub stages: Vec<LiveActivityStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_stage_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_stage_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_job_name: Option<String>,
    pub completed_stage_count: usize,
    pub total_stage_count: usize,
    pub completed_job_count: usize,
    pub total_job_count: usize,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveActivityUpdate {
    pub event: &'static str,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_date: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissal_date: Option<i64>,
    pub content_state: LiveActivityContentState,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveActivityAttributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    pub project_id: i64,
    pub pipeline_id: i64,
    pub pipeline_name: String,
    pub r#ref: String,
    pub deep_link: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LiveActivityAlert {
    pub title: String,
    pub body: String,
    pub sound: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveActivityStart {
    pub event: &'static str,
    pub timestamp: i64,
    pub stale_date: i64,
    pub input_push_token: u8,
    pub content_state: LiveActivityContentState,
    pub attributes_type: &'static str,
    pub attributes: LiveActivityAttributes,
    pub alert: LiveActivityAlert,
}

pub fn build_update(payload: &PipelineEvent, timestamp: i64) -> LiveActivityUpdate {
    let stages = build_stages(payload);
    let failed_stage_index = find_status(&stages, StageStatus::Failed);
    let running_stage_index = find_status(&stages, StageStatus::Running);
    let waiting_stage_index = stages
        .iter()
        .position(|stage| matches!(stage.status, StageStatus::Pending | StageStatus::Manual));
    let canceled_stage_index = find_status(&stages, StageStatus::Canceled);
    let focus_index = failed_stage_index
        .or(running_stage_index)
        .or(waiting_stage_index)
        .or(canceled_stage_index)
        .unwrap_or_else(|| stages.len().saturating_sub(1));
    let focused_stage = stages.get(focus_index);

    let mut sorted_builds: Vec<&PipelineBuild> = payload.builds.iter().collect();
    sorted_builds.sort_by_key(|build| build.id);
    let failed_build = sorted_builds.into_iter().find(|build| {
        build.status == "failed"
            && !build.allow_failure
            && truncate(
                if build.stage.is_empty() {
                    "Build"
                } else {
                    &build.stage
                },
                32,
            ) == focused_stage.map_or("", |stage| stage.name.as_str())
    });
    let status = payload.object_attributes.status.as_str();
    let is_terminal = is_terminal_status(status);
    let completed_stage_count = stages
        .iter()
        .filter(|stage| matches!(stage.status, StageStatus::Success | StageStatus::Skipped))
        .count();
    let completed_job_count = if matches!(status, "success" | "skipped") {
        payload.builds.len()
    } else {
        payload
            .builds
            .iter()
            .filter(|build| is_completed_job_status(&build.status))
            .count()
    };

    LiveActivityUpdate {
        event: if is_terminal { "end" } else { "update" },
        timestamp,
        stale_date: (!is_terminal).then_some(timestamp + ACTIVE_STALE_AFTER_SECONDS),
        dismissal_date: is_terminal
            .then_some(timestamp + if status == "failed" { 60 * 60 } else { 15 * 60 }),
        content_state: LiveActivityContentState {
            status: status.to_owned(),
            stages: select_visible_stages(&stages, focus_index),
            current_stage_name: focused_stage.map(|stage| stage.name.clone()),
            failed_stage_name: failed_stage_index.map(|index| stages[index].name.clone()),
            failed_job_name: failed_build.map(|build| truncate(&build.name, 100)),
            completed_stage_count,
            total_stage_count: stages.len(),
            completed_job_count,
            total_job_count: payload.builds.len(),
            updated_at: timestamp,
        },
    }
}

pub fn build_start(
    payload: &PipelineEvent,
    timestamp: i64,
    instance_id: Option<&str>,
) -> LiveActivityStart {
    let update = build_update(payload, timestamp);
    let pipeline_id = payload.object_attributes.id;
    let project_id = payload.project.id;
    let instance_parameter = instance_id.map_or_else(String::new, |id| {
        format!("&instanceId={}", urlencoding::encode(id))
    });
    let pipeline_name = if payload.project.name.is_empty() {
        format!("Pipeline #{pipeline_id}")
    } else {
        payload.project.name.clone()
    };
    let pipeline_ref = if payload.object_attributes.r#ref.is_empty() {
        "detached"
    } else {
        &payload.object_attributes.r#ref
    };

    LiveActivityStart {
        event: "start",
        timestamp,
        stale_date: update
            .stale_date
            .unwrap_or(timestamp + ACTIVE_STALE_AFTER_SECONDS),
        input_push_token: 1,
        content_state: update.content_state,
        attributes_type: "PipelineActivityAttributes",
        attributes: LiveActivityAttributes {
            instance_id: instance_id.map(ToOwned::to_owned),
            project_id,
            pipeline_id,
            pipeline_name: truncate(&pipeline_name, 80),
            r#ref: truncate(pipeline_ref, 80),
            deep_link: format!(
                "comeet:///PipelineDetails?projectId={project_id}&pipelineId={pipeline_id}{instance_parameter}&notificationSource=liveActivity"
            ),
        },
        alert: LiveActivityAlert {
            title: format!("{} build started", payload.project.name),
            body: format!(
                "{} · Pipeline #{}",
                payload.object_attributes.r#ref, pipeline_id
            ),
            sound: "default",
        },
    }
}

pub fn is_terminal_status(status: &str) -> bool {
    matches!(status, "success" | "failed" | "canceled" | "skipped")
}

fn truncate(value: &str, max_length: usize) -> String {
    let count = value.chars().count();
    if count > max_length {
        let mut truncated: String = value.chars().take(max_length - 1).collect();
        truncated.push('…');
        truncated
    } else {
        value.to_owned()
    }
}

fn normalize_build_status(status: &str) -> StageStatus {
    match status {
        "created"
        | "waiting_for_resource"
        | "preparing"
        | "pending"
        | "scheduled"
        | "waiting_for_callback"
        | "canceling" => StageStatus::Pending,
        "success" => StageStatus::Success,
        "failed" => StageStatus::Failed,
        "running" => StageStatus::Running,
        "canceled" => StageStatus::Canceled,
        "skipped" => StageStatus::Skipped,
        "manual" => StageStatus::Manual,
        _ => StageStatus::Pending,
    }
}

fn aggregate_stage_status(builds: &[PipelineBuild]) -> StageStatus {
    let statuses: Vec<StageStatus> = builds
        .iter()
        .map(|build| {
            if build.status == "failed" && build.allow_failure {
                StageStatus::Success
            } else {
                normalize_build_status(&build.status)
            }
        })
        .collect();

    for status in [
        StageStatus::Failed,
        StageStatus::Running,
        StageStatus::Pending,
        StageStatus::Manual,
        StageStatus::Canceled,
    ] {
        if statuses.contains(&status) {
            return status;
        }
    }
    if !statuses.is_empty()
        && statuses
            .iter()
            .all(|status| *status == StageStatus::Skipped)
    {
        return StageStatus::Skipped;
    }
    if statuses
        .iter()
        .all(|status| matches!(status, StageStatus::Success | StageStatus::Skipped))
    {
        return StageStatus::Success;
    }
    StageStatus::Pending
}

fn terminal_stage_status(
    status: StageStatus,
    builds: &[PipelineBuild],
    pipeline_status: &str,
) -> StageStatus {
    if pipeline_status == "success" {
        if builds
            .iter()
            .all(|build| matches!(build.status.as_str(), "manual" | "skipped"))
        {
            StageStatus::Skipped
        } else {
            StageStatus::Success
        }
    } else if pipeline_status == "skipped" {
        StageStatus::Skipped
    } else if pipeline_status == "canceled"
        && matches!(
            status,
            StageStatus::Running | StageStatus::Pending | StageStatus::Manual
        )
    {
        StageStatus::Canceled
    } else {
        status
    }
}

fn build_stages(payload: &PipelineEvent) -> Vec<LiveActivityStage> {
    let mut grouped: HashMap<String, Vec<PipelineBuild>> = HashMap::new();
    let mut ordered_stage_names = Vec::new();
    let mut known_names = HashSet::new();

    for stage_name in &payload.object_attributes.stages {
        if known_names.insert(stage_name.clone()) {
            grouped.insert(stage_name.clone(), Vec::new());
            ordered_stage_names.push(stage_name.clone());
        }
    }

    let mut sorted_builds = payload.builds.clone();
    sorted_builds.sort_by_key(|build| build.id);
    for build in sorted_builds {
        let stage_name = if build.stage.is_empty() {
            "Build".to_owned()
        } else {
            build.stage.clone()
        };
        if known_names.insert(stage_name.clone()) {
            grouped.insert(stage_name.clone(), Vec::new());
            ordered_stage_names.push(stage_name.clone());
        }
        grouped.entry(stage_name).or_default().push(build);
    }

    let terminal_pipeline_completed = matches!(
        payload.object_attributes.status.as_str(),
        "success" | "skipped"
    );
    ordered_stage_names
        .into_iter()
        .filter_map(|stage_name| {
            let builds = grouped.remove(&stage_name)?;
            if builds.is_empty() {
                return None;
            }
            let status = terminal_stage_status(
                aggregate_stage_status(&builds),
                &builds,
                &payload.object_attributes.status,
            );
            let completed_job_count = if terminal_pipeline_completed {
                builds.len()
            } else {
                builds
                    .iter()
                    .filter(|build| is_completed_job_status(&build.status))
                    .count()
            };
            Some(LiveActivityStage {
                name: truncate(&stage_name, 32),
                status,
                completed_job_count,
                total_job_count: builds.len(),
            })
        })
        .collect()
}

fn find_status(stages: &[LiveActivityStage], status: StageStatus) -> Option<usize> {
    stages.iter().position(|stage| stage.status == status)
}

fn select_visible_stages(
    stages: &[LiveActivityStage],
    focus_index: usize,
) -> Vec<LiveActivityStage> {
    if stages.len() <= MAX_VISIBLE_STAGES {
        return stages.to_vec();
    }
    let max_start = stages.len() - MAX_VISIBLE_STAGES;
    let start = focus_index.saturating_sub(2).min(max_start);
    stages[start..start + MAX_VISIBLE_STAGES].to_vec()
}

fn is_completed_job_status(status: &str) -> bool {
    matches!(status, "success" | "failed" | "canceled" | "skipped")
}

#[cfg(test)]
mod tests {
    use crate::models::{GitLabProject, PipelineAttributes};

    use super::*;

    fn pipeline() -> PipelineEvent {
        PipelineEvent {
            object_attributes: PipelineAttributes {
                id: 42,
                r#ref: "main".to_owned(),
                status: "failed".to_owned(),
                stages: vec!["prepare", "build", "test", "deploy"]
                    .into_iter()
                    .map(ToOwned::to_owned)
                    .collect(),
            },
            project: GitLabProject {
                id: 7,
                name: "Comeet".to_owned(),
                web_url: String::new(),
            },
            builds: vec![
                build(1, "install", "prepare", "success"),
                build(2, "compile", "build", "success"),
                build(3, "unit tests", "test", "failed"),
                build(4, "release", "deploy", "created"),
            ],
        }
    }

    fn build(id: i64, name: &str, stage: &str, status: &str) -> PipelineBuild {
        PipelineBuild {
            id,
            name: name.to_owned(),
            stage: stage.to_owned(),
            status: status.to_owned(),
            allow_failure: false,
        }
    }

    #[test]
    fn builds_terminal_failure_state() {
        let update = build_update(&pipeline(), 1_786_320_000);
        assert_eq!(update.event, "end");
        assert_eq!(update.stale_date, None);
        assert_eq!(update.dismissal_date, Some(1_786_323_600));
        assert_eq!(
            update.content_state.current_stage_name.as_deref(),
            Some("test")
        );
        assert_eq!(
            update.content_state.failed_stage_name.as_deref(),
            Some("test")
        );
        assert_eq!(
            update.content_state.failed_job_name.as_deref(),
            Some("unit tests")
        );
        assert_eq!(update.content_state.completed_stage_count, 2);
        assert_eq!(update.content_state.completed_job_count, 3);
    }

    #[test]
    fn reports_parallel_job_progress() {
        let mut payload = pipeline();
        payload.object_attributes.status = "running".to_owned();
        payload.builds = vec![
            build(1, "web", "build", "success"),
            build(2, "ios", "build", "running"),
            build(3, "android", "build", "pending"),
            build(4, "unit", "test", "pending"),
            build(5, "e2e", "test", "pending"),
        ];
        let update = build_update(&payload, 100);
        assert_eq!(update.content_state.stages.len(), 2);
        assert_eq!(update.content_state.stages[0].status, StageStatus::Running);
        assert_eq!(update.content_state.stages[0].completed_job_count, 1);
        assert_eq!(update.content_state.completed_job_count, 1);
        assert_eq!(update.content_state.total_job_count, 5);
    }

    #[test]
    fn successful_pipeline_completes_optional_manual_stage() {
        let mut payload = pipeline();
        payload.object_attributes.status = "success".to_owned();
        payload.object_attributes.stages = vec!["build".to_owned(), "deploy".to_owned()];
        payload.builds = vec![
            build(1, "compile", "build", "success"),
            build(2, "production", "deploy", "manual"),
        ];
        let update = build_update(&payload, 100);
        assert_eq!(update.content_state.stages[0].status, StageStatus::Success);
        assert_eq!(update.content_state.stages[1].status, StageStatus::Skipped);
        assert_eq!(update.content_state.completed_job_count, 2);
        assert_eq!(update.content_state.completed_stage_count, 2);
    }

    #[test]
    fn allowed_failure_does_not_capture_focus() {
        let mut payload = pipeline();
        payload.object_attributes.status = "running".to_owned();
        let mut optional = build(1, "optional lint", "test", "failed");
        optional.allow_failure = true;
        payload.builds = vec![optional, build(2, "release", "deploy", "running")];
        let update = build_update(&payload, 100);
        assert_eq!(
            update.content_state.current_stage_name.as_deref(),
            Some("deploy")
        );
        assert!(update.content_state.failed_stage_name.is_none());
        assert!(update.content_state.failed_job_name.is_none());
        assert_eq!(update.content_state.stages[0].status, StageStatus::Success);
    }

    #[test]
    fn active_update_stales_after_fifteen_minutes_and_shows_six_stages() {
        let mut payload = pipeline();
        payload.object_attributes.status = "running".to_owned();
        payload.object_attributes.stages = ["one", "two", "three", "four", "five", "six", "seven"]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect();
        payload.builds = payload
            .object_attributes
            .stages
            .iter()
            .enumerate()
            .map(|(index, stage)| {
                build(
                    index as i64,
                    stage,
                    stage,
                    if stage == "six" {
                        "running"
                    } else if index < 5 {
                        "success"
                    } else {
                        "created"
                    },
                )
            })
            .collect();
        let update = build_update(&payload, 1_786_320_000);
        assert_eq!(update.event, "update");
        assert_eq!(update.stale_date, Some(1_786_320_900));
        assert_eq!(update.content_state.stages.len(), 6);
        assert!(
            update
                .content_state
                .stages
                .iter()
                .any(|stage| stage.name == "six")
        );
    }

    #[test]
    fn builds_remote_start_attributes() {
        let mut payload = pipeline();
        payload.object_attributes.status = "running".to_owned();
        let start = build_start(&payload, 1_786_320_000, None);
        assert_eq!(start.event, "start");
        assert_eq!(start.input_push_token, 1);
        assert_eq!(start.attributes_type, "PipelineActivityAttributes");
        assert_eq!(start.attributes.pipeline_name, "Comeet");
        assert_eq!(start.attributes.r#ref, "main");
        assert_eq!(
            start.attributes.deep_link,
            "comeet:///PipelineDetails?projectId=7&pipelineId=42&notificationSource=liveActivity"
        );
        assert_eq!(start.alert.title, "Comeet build started");
    }

    #[test]
    fn remote_start_encodes_instance_id() {
        let mut payload = pipeline();
        payload.object_attributes.status = "running".to_owned();
        let start = build_start(&payload, 100, Some("gitlab work/+"));
        assert_eq!(
            start.attributes.instance_id.as_deref(),
            Some("gitlab work/+")
        );
        assert!(
            start
                .attributes
                .deep_link
                .contains("instanceId=gitlab%20work%2F%2B")
        );
    }

    #[test]
    fn active_pipeline_statuses_do_not_end() {
        for status in ["manual", "scheduled", "canceling", "waiting_for_callback"] {
            let mut payload = pipeline();
            payload.object_attributes.status = status.to_owned();
            assert_eq!(build_update(&payload, 100).event, "update");
        }
    }

    #[test]
    fn completed_pipeline_statuses_end() {
        for status in ["success", "failed", "canceled", "skipped"] {
            let mut payload = pipeline();
            payload.object_attributes.status = status.to_owned();
            assert_eq!(build_update(&payload, 100).event, "end");
        }
    }

    #[test]
    fn canceled_pipeline_marks_unfinished_stages_canceled() {
        let mut payload = pipeline();
        payload.object_attributes.status = "canceled".to_owned();
        let update = build_update(&payload, 100);
        assert_eq!(update.content_state.stages[3].status, StageStatus::Canceled);
        assert_eq!(update.dismissal_date, Some(1000));
    }

    #[test]
    fn skipped_pipeline_marks_every_stage_skipped() {
        let mut payload = pipeline();
        payload.object_attributes.status = "skipped".to_owned();
        let update = build_update(&payload, 100);
        assert!(
            update
                .content_state
                .stages
                .iter()
                .all(|stage| stage.status == StageStatus::Skipped)
        );
    }

    #[test]
    fn sorts_builds_and_appends_unknown_stages() {
        let mut payload = pipeline();
        payload.object_attributes.stages = vec!["known".to_owned()];
        payload.builds = vec![
            build(2, "b", "later", "running"),
            build(1, "a", "first", "success"),
        ];
        let update = build_update(&payload, 100);
        let names: Vec<_> = update
            .content_state
            .stages
            .iter()
            .map(|stage| stage.name.as_str())
            .collect();
        assert_eq!(names, vec!["first", "later"]);
    }

    #[test]
    fn truncates_names_at_mobile_contract_limits() {
        let mut payload = pipeline();
        payload.project.name = "p".repeat(100);
        payload.object_attributes.r#ref = "r".repeat(100);
        payload.object_attributes.stages = vec!["s".repeat(40)];
        payload.builds = vec![build(1, &"j".repeat(120), &"s".repeat(40), "failed")];
        let update = build_update(&payload, 100);
        assert_eq!(update.content_state.stages[0].name.chars().count(), 32);
        assert_eq!(
            update
                .content_state
                .failed_job_name
                .unwrap()
                .chars()
                .count(),
            100
        );
        let start = build_start(&payload, 100, None);
        assert_eq!(start.attributes.pipeline_name.chars().count(), 80);
        assert_eq!(start.attributes.r#ref.chars().count(), 80);
    }
}
