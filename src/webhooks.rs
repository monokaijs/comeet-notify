use std::{collections::HashMap, sync::Arc};

use regex::Regex;
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::{
    android_updates,
    fcm::{FcmClient, Notification},
    live_activity::{build_start, build_update, is_terminal_status},
    models::{GitLabProject, GitLabWebhookEvent, JobEvent, PipelineBuild, PipelineEvent},
    parser::parse_event,
};

const MAX_LIVE_ACTIVITY_REGISTRATIONS: usize = 8;
const MAX_LIVE_ACTIVITY_AGE_SECONDS: i64 = 8 * 60 * 60;
const REMOTE_START_DEDUPLICATION_SECONDS: i64 = 8 * 60 * 60;
const PIPELINE_SNAPSHOT_TTL_SECONDS: i64 = 8 * 60 * 60;

#[derive(Clone, Debug, Default)]
pub struct LiveActivityHeaders {
    pub android_registrations: Option<String>,
    pub token: Option<String>,
    pub pipeline_id: Option<String>,
    pub registrations: Option<String>,
    pub push_to_start_token: Option<String>,
    pub instance_id: Option<String>,
    pub gitlab_instance: Option<String>,
    pub pipeline_delivery_mode: Option<String>,
}

#[derive(Clone, Debug)]
struct CachedPipelineSnapshot {
    pipeline: PipelineEvent,
    updated_at: i64,
}

type AndroidDeliveryQueue = Arc<Mutex<i64>>;

#[derive(Clone)]
pub struct WebhookProcessor {
    fcm: FcmClient,
    snapshot_revision: Arc<std::sync::atomic::AtomicI64>,
    android_delivery_queues: Arc<Mutex<HashMap<String, (i64, AndroidDeliveryQueue)>>>,
    remotely_started_pipelines: Arc<Mutex<HashMap<String, i64>>>,
    pipeline_snapshots: Arc<Mutex<HashMap<String, CachedPipelineSnapshot>>>,
    live_activity_token_pattern: Arc<Regex>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PipelineDeliveryMode {
    LiveActivity,
    Notification,
    Both,
}

impl WebhookProcessor {
    pub fn new(fcm: FcmClient) -> Self {
        Self {
            fcm,
            snapshot_revision: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            android_delivery_queues: Arc::new(Mutex::new(HashMap::new())),
            remotely_started_pipelines: Arc::new(Mutex::new(HashMap::new())),
            pipeline_snapshots: Arc::new(Mutex::new(HashMap::new())),
            live_activity_token_pattern: Arc::new(
                Regex::new(r"^[a-fA-F0-9]{32,512}$").expect("valid token regex"),
            ),
        }
    }

    pub async fn process(
        &self,
        payload: &GitLabWebhookEvent,
        fcm_token: &str,
        live_activity: &LiveActivityHeaders,
    ) -> Result<(), String> {
        let is_job_event = matches!(payload, GitLabWebhookEvent::Job(_));
        let snapshot = self
            .pipeline_snapshot(payload, live_activity, unix_timestamp())
            .await;
        let pipeline = snapshot.as_ref().map(|(pipeline, _)| pipeline);
        let revision = snapshot.as_ref().map_or(0, |(_, revision)| *revision);
        let android_registrations = pipeline.map_or_else(Vec::new, |pipeline| {
            android_updates::registrations(
                live_activity.android_registrations.as_deref(),
                live_activity.instance_id.as_deref(),
                pipeline.object_attributes.id,
                unix_timestamp(),
            )
        });
        let notification_data = parse_event(payload);
        if notification_data.is_none() && !is_job_event {
            warn!(
                object_kind = payload.object_kind(),
                "Unsupported event type"
            );
            return Ok(());
        }
        if is_job_event && pipeline.is_none() {
            return Ok(());
        }

        let notification = notification_data.map(|notification_data| {
            let mut fcm_data = notification_data.deep_link_data.clone();
            if fcm_data.get("commit_sha").is_some_and(String::is_empty) {
                fcm_data.remove("commit_sha");
            }
            fcm_data.insert(
                "eventType".to_owned(),
                notification_data.event_type.as_str().to_owned(),
            );
            fcm_data.insert(
                "repositoryName".to_owned(),
                notification_data.repository_name,
            );
            fcm_data.insert("repositoryUrl".to_owned(), notification_data.repository_url);
            if let Some(instance_id) = &live_activity.instance_id {
                fcm_data.insert("instance_id".to_owned(), instance_id.clone());
            }
            Notification {
                title: notification_data.title,
                body: notification_data.message,
                data: fcm_data,
            }
        });

        let delivery_mode = pipeline_delivery_mode(live_activity);
        let allows_notification = android_registrations.is_empty()
            && notification.is_some()
            && (payload.pipeline().is_none()
                || delivery_mode != PipelineDeliveryMode::LiveActivity);
        let allows_live_activity =
            pipeline.is_some() && delivery_mode != PipelineDeliveryMode::Notification;

        let notification_future = async {
            if allows_notification {
                Some(
                    self.fcm
                        .send_notification(
                            fcm_token,
                            notification.as_ref().expect("notification is available"),
                        )
                        .await,
                )
            } else {
                None
            }
        };
        let live_activity_future = async {
            let pipeline = pipeline.as_ref()?;
            let start_key = live_activity_start_key(pipeline, fcm_token);
            let token = if allows_live_activity {
                self.find_live_activity_token(pipeline, live_activity)
            } else {
                None
            };
            if let Some(live_activity_token) = token {
                let now = unix_timestamp();
                let update = build_update(pipeline, now);
                let collapse_id = collapse_id(pipeline);
                let result = self
                    .fcm
                    .send_live_activity_update(
                        fcm_token,
                        &live_activity_token,
                        &update,
                        Some(&collapse_id),
                    )
                    .await;
                if is_terminal_status(&pipeline.object_attributes.status) {
                    self.remotely_started_pipelines
                        .lock()
                        .await
                        .remove(&start_key);
                }
                return Some(result);
            }

            if !is_job_event
                && allows_live_activity
                && self
                    .reserve_remote_start(pipeline, live_activity, fcm_token)
                    .await
            {
                let start = build_start(
                    pipeline,
                    unix_timestamp(),
                    live_activity.instance_id.as_deref(),
                );
                let collapse_id = collapse_id(pipeline);
                let result = self
                    .fcm
                    .send_live_activity_start(
                        fcm_token,
                        live_activity
                            .push_to_start_token
                            .as_deref()
                            .expect("validated push-to-start token"),
                        &start,
                        Some(&collapse_id),
                    )
                    .await;
                if !result.success {
                    self.remotely_started_pipelines
                        .lock()
                        .await
                        .remove(&start_key);
                }
                return Some(result);
            }

            if is_terminal_status(&pipeline.object_attributes.status) {
                self.remotely_started_pipelines
                    .lock()
                    .await
                    .remove(&start_key);
            }
            None
        };

        let android_future = async {
            let Some(pipeline) = pipeline else {
                return;
            };
            let content = build_update(pipeline, unix_timestamp()).content_state;
            for registration in &android_registrations {
                // Serialize each collapse key. A delayed older request must never replace a queued terminal snapshot.
                let queue = {
                    let mut queues = self.android_delivery_queues.lock().await;
                    queues.retain(|_, (expires, _)| *expires > unix_timestamp());
                    let key = format!("{fcm_token}:{}", registration.registration_id);
                    if queues.len() >= 4096 && !queues.contains_key(&key) {
                        continue;
                    }
                    queues
                        .entry(key)
                        .or_insert_with(|| (registration.expires_at, Arc::new(Mutex::new(0))))
                        .1
                        .clone()
                };
                let mut sent_revision = queue.lock().await;
                if revision <= *sent_revision || registration.expires_at <= unix_timestamp() {
                    continue;
                }
                *sent_revision = revision;
                let request = android_updates::request(
                    fcm_token,
                    registration,
                    pipeline.project.id,
                    revision,
                    &content,
                );
                let result = self.fcm.send_android_pipeline_update(request).await;
                if !result.success {
                    warn!(error = ?result.error, "Android pipeline update failed");
                }
            }
        };
        let (notification_result, live_activity_result, ()) =
            tokio::join!(notification_future, live_activity_future, android_future);
        if let Some(result) = live_activity_result.filter(|result| !result.success) {
            warn!(error = ?result.error, "Live Activity update failed");
        }
        match notification_result {
            None if is_job_event => info!("Job Hook applied to Live Activity progress"),
            None => info!("Regular pipeline notification suppressed by delivery preference"),
            Some(result) if result.success => {
                info!(
                    object_kind = payload.object_kind(),
                    "Notification sent successfully"
                )
            }
            Some(result) => warn!(
                error = ?result.error,
                "Notification delivery failed without rejecting the GitLab webhook"
            ),
        }
        Ok(())
    }

    async fn pipeline_snapshot(
        &self,
        payload: &GitLabWebhookEvent,
        headers: &LiveActivityHeaders,
        now: i64,
    ) -> Option<(PipelineEvent, i64)> {
        let mut snapshots = self.pipeline_snapshots.lock().await;
        snapshots.retain(|_, snapshot| now - snapshot.updated_at < PIPELINE_SNAPSHOT_TTL_SECONDS);

        match payload {
            GitLabWebhookEvent::Pipeline(pipeline) => {
                snapshots.insert(
                    pipeline_snapshot_key(
                        &pipeline.project,
                        pipeline.object_attributes.id,
                        headers.gitlab_instance.as_deref(),
                    ),
                    CachedPipelineSnapshot {
                        pipeline: pipeline.clone(),
                        updated_at: now,
                    },
                );
                Some((pipeline.clone(), self.next_snapshot_revision()))
            }
            GitLabWebhookEvent::Job(job) => {
                let key = pipeline_snapshot_key(
                    &job.project,
                    job.pipeline_id,
                    headers.gitlab_instance.as_deref(),
                );
                let Some(snapshot) = snapshots.get_mut(&key) else {
                    warn!(
                        project_id = job.project.id,
                        pipeline_id = job.pipeline_id,
                        "Ignoring Job Hook without a cached Pipeline Hook snapshot"
                    );
                    return None;
                };
                apply_job_event(&mut snapshot.pipeline, job);
                snapshot.updated_at = now;
                Some((snapshot.pipeline.clone(), self.next_snapshot_revision()))
            }
            _ => None,
        }
    }

    fn next_snapshot_revision(&self) -> i64 {
        use std::sync::atomic::Ordering;
        let now = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1000) as i64;
        let previous = self
            .snapshot_revision
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |previous| {
                Some(now.max(previous + 1))
            })
            .expect("revision always advances");
        now.max(previous + 1)
    }

    fn find_live_activity_token(
        &self,
        pipeline: &PipelineEvent,
        headers: &LiveActivityHeaders,
    ) -> Option<String> {
        if let Some(registrations) = headers
            .registrations
            .as_deref()
            .filter(|registrations| registrations.len() <= 8192)
        {
            match serde_json::from_str::<Value>(registrations) {
                Ok(Value::Array(items)) => {
                    let first = items.len().saturating_sub(MAX_LIVE_ACTIVITY_REGISTRATIONS);
                    if let Some(registration) = items[first..].iter().find(|item| {
                        item.get("pipelineId").and_then(Value::as_i64)
                            == Some(pipeline.object_attributes.id)
                    }) {
                        if registration
                            .get("registeredAt")
                            .and_then(Value::as_f64)
                            .is_some_and(|registered_at| {
                                registered_at.is_finite()
                                    && unix_timestamp() - registered_at.floor() as i64
                                        >= MAX_LIVE_ACTIVITY_AGE_SECONDS
                            })
                        {
                            info!(
                                pipeline_id = pipeline.object_attributes.id,
                                "Ignoring an expired Live Activity registration"
                            );
                            return None;
                        }
                        if let Some(token) = registration
                            .get("pushToken")
                            .and_then(Value::as_str)
                            .filter(|token| self.valid_live_activity_token(token))
                        {
                            return Some(token.to_owned());
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => warn!("Ignoring invalid Live Activity registrations"),
            }
        }

        match (&headers.token, &headers.pipeline_id) {
            (Some(token), Some(pipeline_id))
                if pipeline_id == &pipeline.object_attributes.id.to_string()
                    && self.valid_live_activity_token(token) =>
            {
                Some(token.clone())
            }
            _ => None,
        }
    }

    async fn reserve_remote_start(
        &self,
        pipeline: &PipelineEvent,
        headers: &LiveActivityHeaders,
        fcm_token: &str,
    ) -> bool {
        let Some(token) = headers.push_to_start_token.as_deref() else {
            return false;
        };
        if !self.valid_live_activity_token(token) {
            warn!("Ignoring an invalid Live Activity push-to-start token");
            return false;
        }
        if pipeline.object_attributes.status != "running" {
            return false;
        }

        let key = live_activity_start_key(pipeline, fcm_token);
        let now = unix_timestamp();
        let mut starts = self.remotely_started_pipelines.lock().await;
        starts.retain(|_, started_at| now - *started_at < REMOTE_START_DEDUPLICATION_SECONDS);
        if let std::collections::hash_map::Entry::Vacant(entry) = starts.entry(key) {
            entry.insert(now);
            true
        } else {
            false
        }
    }

    fn valid_live_activity_token(&self, token: &str) -> bool {
        self.live_activity_token_pattern.is_match(token)
    }
}

fn pipeline_snapshot_key(
    project: &GitLabProject,
    pipeline_id: i64,
    gitlab_instance: Option<&str>,
) -> String {
    let scope = if project.web_url.is_empty() {
        gitlab_instance.unwrap_or("unknown-gitlab-instance")
    } else {
        &project.web_url
    };
    format!("{scope}:{}:{pipeline_id}", project.id)
}

fn apply_job_event(pipeline: &mut PipelineEvent, job: &JobEvent) {
    pipeline
        .object_attributes
        .status
        .clone_from(&job.commit.status);
    if !job.r#ref.is_empty() {
        pipeline.object_attributes.r#ref.clone_from(&job.r#ref);
    }
    if !job.build_stage.is_empty() && !pipeline.object_attributes.stages.contains(&job.build_stage)
    {
        pipeline
            .object_attributes
            .stages
            .push(job.build_stage.clone());
    }

    let updated_build = PipelineBuild {
        id: job.build_id,
        name: job.build_name.clone(),
        stage: job.build_stage.clone(),
        status: job.build_status.clone(),
        allow_failure: job.build_allow_failure,
    };
    if let Some(build) = pipeline
        .builds
        .iter_mut()
        .find(|build| build.id == job.build_id)
    {
        *build = updated_build;
    } else {
        pipeline.builds.push(updated_build);
    }
}

fn pipeline_delivery_mode(headers: &LiveActivityHeaders) -> PipelineDeliveryMode {
    match headers.pipeline_delivery_mode.as_deref() {
        Some("live_activity") => PipelineDeliveryMode::LiveActivity,
        Some("notification") => PipelineDeliveryMode::Notification,
        Some("both") => PipelineDeliveryMode::Both,
        _ => PipelineDeliveryMode::Both,
    }
}

fn live_activity_start_key(pipeline: &PipelineEvent, fcm_token: &str) -> String {
    format!(
        "{}:{}:{}",
        fcm_token, pipeline.project.id, pipeline.object_attributes.id
    )
}

fn collapse_id(pipeline: &PipelineEvent) -> String {
    format!(
        "comeet-{}-{}",
        pipeline.project.id, pipeline.object_attributes.id
    )
}

fn unix_timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn expired_pipeline_snapshots_do_not_accept_job_updates() {
        let processor = WebhookProcessor::new(FcmClient::disabled());
        let pipeline: GitLabWebhookEvent = serde_json::from_value(json!({
            "object_kind": "pipeline",
            "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
            "object_attributes": {"id": 42, "ref": "main", "status": "running", "stages": ["build"]},
            "builds": [{"id": 1, "name": "web", "stage": "build", "status": "running"}]
        }))
        .unwrap();
        let job: GitLabWebhookEvent = serde_json::from_value(json!({
            "object_kind": "build",
            "ref": "main",
            "build_id": 1,
            "build_name": "web",
            "build_stage": "build",
            "build_status": "success",
            "pipeline_id": 42,
            "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
            "commit": {"status": "running"}
        }))
        .unwrap();

        assert!(
            processor
                .pipeline_snapshot(&pipeline, &LiveActivityHeaders::default(), 0)
                .await
                .is_some()
        );
        assert!(
            processor
                .pipeline_snapshot(
                    &job,
                    &LiveActivityHeaders::default(),
                    PIPELINE_SNAPSHOT_TTL_SECONDS,
                )
                .await
                .is_none()
        );
    }
}
