use std::{collections::HashMap, sync::Arc};

use regex::Regex;
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::{
    fcm::{FcmClient, Notification},
    live_activity::{build_start, build_update, is_terminal_status},
    models::{GitLabWebhookEvent, PipelineEvent},
    parser::parse_event,
};

const MAX_LIVE_ACTIVITY_REGISTRATIONS: usize = 8;
const MAX_LIVE_ACTIVITY_AGE_SECONDS: i64 = 8 * 60 * 60;
const REMOTE_START_DEDUPLICATION_SECONDS: i64 = 8 * 60 * 60;

#[derive(Clone, Debug, Default)]
pub struct LiveActivityHeaders {
    pub token: Option<String>,
    pub pipeline_id: Option<String>,
    pub registrations: Option<String>,
    pub push_to_start_token: Option<String>,
    pub instance_id: Option<String>,
    pub pipeline_delivery_mode: Option<String>,
}

#[derive(Clone)]
pub struct WebhookProcessor {
    fcm: FcmClient,
    remotely_started_pipelines: Arc<Mutex<HashMap<String, i64>>>,
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
            remotely_started_pipelines: Arc::new(Mutex::new(HashMap::new())),
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
        let Some(notification_data) = parse_event(payload) else {
            warn!(
                object_kind = payload.object_kind(),
                "Unsupported event type"
            );
            return Ok(());
        };

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
        let notification = Notification {
            title: notification_data.title,
            body: notification_data.message,
            data: fcm_data,
        };

        let pipeline = payload.pipeline();
        let delivery_mode = pipeline_delivery_mode(live_activity);
        let allows_notification =
            pipeline.is_none() || delivery_mode != PipelineDeliveryMode::LiveActivity;
        let allows_live_activity =
            pipeline.is_some() && delivery_mode != PipelineDeliveryMode::Notification;

        let notification_future = async {
            if allows_notification {
                Some(self.fcm.send_notification(fcm_token, &notification).await)
            } else {
                None
            }
        };
        let live_activity_future = async {
            let pipeline = pipeline?;
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

            if allows_live_activity
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

        let (notification_result, live_activity_result) =
            tokio::join!(notification_future, live_activity_future);
        if let Some(result) = live_activity_result.filter(|result| !result.success) {
            warn!(error = ?result.error, "Live Activity update failed");
        }
        match notification_result {
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
