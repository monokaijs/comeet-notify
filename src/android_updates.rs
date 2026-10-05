//! Explicit Android tracking protocol. Reuses ActivityKit job aggregation unchanged.
use crate::live_activity::LiveActivityContentState;
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_AGE: i64 = 8 * 60 * 60;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AndroidRegistration {
    pub registration_id: String,
    pub account_key: String,
    pub pipeline_id: i64,
    pub expires_at: i64,
}

pub fn registrations(
    header: Option<&str>,
    instance: Option<&str>,
    pipeline_id: i64,
    now: i64,
) -> Vec<AndroidRegistration> {
    let Some(instance) = instance.filter(|value| !value.is_empty() && value.len() <= 128) else {
        return vec![];
    };
    let Some(header) = header.filter(|value| value.len() <= 4096) else {
        return vec![];
    };
    let Ok(items) = serde_json::from_str::<Vec<AndroidRegistration>>(header) else {
        return vec![];
    };
    if items.len() > 4 {
        return vec![];
    }
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|item| {
            item.pipeline_id == pipeline_id
                && pipeline_id > 0
                && item.expires_at > now
                && item.expires_at <= now + MAX_AGE
                && item.registration_id.len() == 32
                && item
                    .registration_id
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                && item.account_key.starts_with(&format!("{instance}:"))
                && item.account_key.len() <= 160
                && item.account_key.len() > instance.len() + 1
                && item
                    .account_key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_:".contains(&byte))
                && seen.insert(item.registration_id.clone())
        })
        .collect()
}

pub fn request(
    token: &str,
    registration: &AndroidRegistration,
    project_id: i64,
    revision: i64,
    content: &LiveActivityContentState,
) -> Value {
    json!({"message": {
        "token": token,
        "data": {
            "type": "pipeline_live_update",
            "registration_id": registration.registration_id,
            "account_key": registration.account_key,
            "project_id": project_id.to_string(),
            "pipeline_id": registration.pipeline_id.to_string(),
            "revision": revision.to_string(),
            "payload": serde_json::to_string(content).expect("pipeline content is serializable")
        },
        "android": {"priority": "HIGH", "ttl": "60s", "collapse_key": registration.registration_id}
    }})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routing_is_explicit_bounded_scoped_and_expiring() {
        let mut item = json!({"registrationId": "a".repeat(32), "accountKey": "instance-a:7", "pipelineId": 42, "expiresAt": 2000});
        let header = |item: &Value| json!([item]).to_string();
        assert_eq!(
            registrations(Some(&header(&item)), Some("instance-a"), 42, 1000).len(),
            1
        );
        assert!(registrations(Some(&header(&item)), Some("instance-b"), 42, 1000).is_empty());
        assert!(registrations(Some(&header(&item)), Some("instance-a"), 43, 1000).is_empty());
        assert!(registrations(Some(&header(&item)), None, 42, 1000).is_empty());
        assert!(registrations(None, Some("instance-a"), 42, 1000).is_empty());
        assert!(registrations(Some("[]"), Some("instance-a"), 42, 1000).is_empty());
        assert!(registrations(Some("not-json"), Some("instance-a"), 42, 1000).is_empty());
        assert!(
            registrations(
                Some(&json!([item, item, item, item, item]).to_string()),
                Some("instance-a"),
                42,
                1000
            )
            .is_empty()
        );
        assert_eq!(
            registrations(
                Some(&json!([item, item]).to_string()),
                Some("instance-a"),
                42,
                1000
            )
            .len(),
            1
        );
        item["expiresAt"] = json!(1000);
        assert!(registrations(Some(&header(&item)), Some("instance-a"), 42, 1000).is_empty());
        item["expiresAt"] = json!(1000 + MAX_AGE + 1);
        assert!(registrations(Some(&header(&item)), Some("instance-a"), 42, 1000).is_empty());
        item["expiresAt"] = json!(2000);
        item["registrationId"] = json!("not-a-nonce");
        assert!(registrations(Some(&header(&item)), Some("instance-a"), 42, 1000).is_empty());
    }
}
