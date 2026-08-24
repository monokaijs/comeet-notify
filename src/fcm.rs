use std::{collections::BTreeMap, sync::Arc, time::Duration};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::{
    config::FirebaseConfig,
    live_activity::{LiveActivityStart, LiveActivityUpdate},
};

const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const FCM_BASE_URL: &str = "https://fcm.googleapis.com/v1/projects";
const FIREBASE_MESSAGING_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const TOKEN_EXPIRY_MARGIN_SECONDS: i64 = 60;

#[derive(Clone, Debug)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub data: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FcmResponse {
    pub success: bool,
    pub message_id: Option<String>,
    pub error: Option<String>,
}

impl FcmResponse {
    fn success(message_id: String) -> Self {
        Self {
            success: true,
            message_id: Some(message_id),
            error: None,
        }
    }

    fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message_id: None,
            error: Some(message.into()),
        }
    }
}

#[derive(Clone)]
pub struct FcmClient {
    inner: Option<Arc<FcmInner>>,
}

struct FcmInner {
    project_id: String,
    client_email: String,
    private_key: EncodingKey,
    http: Client,
    token_url: String,
    fcm_base_url: String,
    token: Mutex<Option<CachedToken>>,
}

#[derive(Clone, Debug)]
struct CachedToken {
    access_token: String,
    refresh_at: i64,
}

#[derive(Debug, Error)]
enum FcmError {
    #[error("Firebase not initialized")]
    Disabled,
    #[error("failed to create OAuth assertion: {0}")]
    Assertion(#[from] jsonwebtoken::errors::Error),
    #[error("OAuth token request failed: {0}")]
    TokenRequest(#[from] reqwest::Error),
    #[error("OAuth token endpoint returned {status}: {body}")]
    TokenEndpoint { status: StatusCode, body: String },
    #[error("OAuth token response did not contain an access token")]
    MissingAccessToken,
    #[error("failed to encode FCM request: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Serialize)]
struct ServiceAccountClaims<'a> {
    iss: &'a str,
    scope: &'static str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct OAuthTokenResponse {
    access_token: Option<String>,
    #[serde(default = "default_token_expiry")]
    expires_in: i64,
}

fn default_token_expiry() -> i64 {
    3600
}

#[derive(Deserialize)]
struct SendResponse {
    name: String,
}

impl FcmClient {
    pub fn disabled() -> Self {
        Self { inner: None }
    }

    pub fn new(config: Option<FirebaseConfig>) -> Self {
        let Some(config) = config else {
            warn!("Firebase configuration is incomplete; FCM is disabled");
            return Self::disabled();
        };
        Self::with_endpoints(config, GOOGLE_TOKEN_URL, FCM_BASE_URL)
    }

    #[doc(hidden)]
    pub fn with_endpoints(
        config: FirebaseConfig,
        token_url: impl Into<String>,
        fcm_base_url: impl Into<String>,
    ) -> Self {
        let normalized_private_key = config.private_key.replace("\\n", "\n");
        let private_key = match EncodingKey::from_rsa_pem(normalized_private_key.as_bytes()) {
            Ok(key) => key,
            Err(error) => {
                error!(%error, "Firebase private key is invalid; FCM is disabled");
                return Self::disabled();
            }
        };
        let http = match Client::builder()
            .use_rustls_tls()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                error!(%error, "FCM HTTP client could not be created; FCM is disabled");
                return Self::disabled();
            }
        };

        info!(project_id = %config.project_id, "FCM HTTP v1 client initialized");
        Self {
            inner: Some(Arc::new(FcmInner {
                project_id: config.project_id,
                client_email: config.client_email,
                private_key,
                http,
                token_url: token_url.into(),
                fcm_base_url: fcm_base_url.into(),
                token: Mutex::new(None),
            })),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    pub async fn send_notification(
        &self,
        fcm_token: &str,
        notification: &Notification,
    ) -> FcmResponse {
        let request = notification_request(fcm_token, notification);
        self.send(request).await
    }

    pub async fn send_live_activity_update(
        &self,
        fcm_token: &str,
        live_activity_token: &str,
        update: &LiveActivityUpdate,
        collapse_id: Option<&str>,
    ) -> FcmResponse {
        match live_activity_request(fcm_token, live_activity_token, update, collapse_id) {
            Ok(request) => self.send(request).await,
            Err(error) => FcmResponse::failure(error.to_string()),
        }
    }

    pub async fn send_live_activity_start(
        &self,
        fcm_token: &str,
        live_activity_token: &str,
        start: &LiveActivityStart,
        collapse_id: Option<&str>,
    ) -> FcmResponse {
        match live_activity_request(fcm_token, live_activity_token, start, collapse_id) {
            Ok(request) => self.send(request).await,
            Err(error) => FcmResponse::failure(error.to_string()),
        }
    }

    async fn send(&self, request: Value) -> FcmResponse {
        let Some(inner) = &self.inner else {
            error!("FCM is disabled");
            return FcmResponse::failure(FcmError::Disabled.to_string());
        };
        let access_token = match inner.access_token().await {
            Ok(token) => token,
            Err(error) => {
                error!(%error, "Failed to authenticate with FCM");
                return FcmResponse::failure(error.to_string());
            }
        };
        let endpoint = format!(
            "{}/{}/messages:send",
            inner.fcm_base_url.trim_end_matches('/'),
            urlencoding::encode(&inner.project_id)
        );
        let response = match inner
            .http
            .post(endpoint)
            .bearer_auth(access_token)
            .json(&request)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                error!(%error, "FCM request failed");
                return FcmResponse::failure(error.to_string());
            }
        };
        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(error) => {
                error!(%error, "Could not read the FCM response");
                return FcmResponse::failure(error.to_string());
            }
        };
        if !status.is_success() {
            let message = if body.contains("UNREGISTERED") {
                "Invalid FCM token".to_owned()
            } else {
                format!("FCM returned {status}: {body}")
            };
            warn!(%status, response = %body, "FCM rejected a message");
            return FcmResponse::failure(message);
        }
        match serde_json::from_str::<SendResponse>(&body) {
            Ok(response) => {
                info!(message_id = %response.name, "FCM notification sent successfully");
                FcmResponse::success(response.name)
            }
            Err(error) => FcmResponse::failure(format!("Invalid FCM response: {error}")),
        }
    }
}

impl FcmInner {
    async fn access_token(&self) -> Result<String, FcmError> {
        // The mutex is intentionally held through refresh so concurrent webhook
        // requests coalesce into a single OAuth exchange.
        let mut cached = self.token.lock().await;
        let now = unix_timestamp();
        if let Some(token) = cached.as_ref().filter(|token| token.refresh_at > now) {
            return Ok(token.access_token.clone());
        }

        let claims = ServiceAccountClaims {
            iss: &self.client_email,
            scope: FIREBASE_MESSAGING_SCOPE,
            aud: &self.token_url,
            iat: now,
            exp: now + 3600,
        };
        let assertion = encode(&Header::new(Algorithm::RS256), &claims, &self.private_key)?;
        let response = self
            .http
            .post(&self.token_url)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(FcmError::TokenEndpoint { status, body });
        }
        let token: OAuthTokenResponse = serde_json::from_str(&body)?;
        let access_token = token.access_token.ok_or(FcmError::MissingAccessToken)?;
        let refresh_at = now + token.expires_in.max(0) - TOKEN_EXPIRY_MARGIN_SECONDS;
        *cached = Some(CachedToken {
            access_token: access_token.clone(),
            refresh_at,
        });
        Ok(access_token)
    }
}

pub fn notification_request(fcm_token: &str, notification: &Notification) -> Value {
    json!({
        "message": {
            "notification": {
                "title": notification.title,
                "body": notification.body,
            },
            "data": notification.data,
            "token": fcm_token,
            "android": {
                "notification": {
                    "channel_id": "gitlab_notifications",
                    "notification_priority": "PRIORITY_HIGH",
                }
            },
            "apns": {
                "payload": {
                    "aps": {
                        "alert": {
                            "title": notification.title,
                            "body": notification.body,
                        },
                        "badge": 1,
                        "sound": "default",
                    }
                }
            }
        }
    })
}

pub fn live_activity_request<T: Serialize>(
    fcm_token: &str,
    live_activity_token: &str,
    update: &T,
    collapse_id: Option<&str>,
) -> Result<Value, serde_json::Error> {
    let source = serde_json::to_value(update)?;
    let source = source
        .as_object()
        .expect("Live Activity message is an object");
    let mut aps = Map::new();
    copy_required(source, &mut aps, "timestamp", "timestamp");
    copy_required(source, &mut aps, "event", "event");
    copy_required(source, &mut aps, "contentState", "content-state");
    copy_optional(source, &mut aps, "staleDate", "stale-date");
    copy_optional(source, &mut aps, "dismissalDate", "dismissal-date");
    copy_optional(source, &mut aps, "inputPushToken", "input-push-token");
    copy_optional(source, &mut aps, "attributesType", "attributes-type");
    copy_optional(source, &mut aps, "attributes", "attributes");
    copy_optional(source, &mut aps, "alert", "alert");

    let mut headers = Map::from_iter([("apns-priority".to_owned(), json!("10"))]);
    if let Some(collapse_id) = collapse_id {
        headers.insert("apns-collapse-id".to_owned(), json!(collapse_id));
    }
    Ok(json!({
        "message": {
            "token": fcm_token,
            "apns": {
                "live_activity_token": live_activity_token,
                "headers": headers,
                "payload": { "aps": aps },
            }
        }
    }))
}

fn copy_required(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    from: &str,
    to: &str,
) {
    target.insert(
        to.to_owned(),
        source
            .get(from)
            .unwrap_or_else(|| panic!("missing required Live Activity field {from}"))
            .clone(),
    );
}

fn copy_optional(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    from: &str,
    to: &str,
) {
    if let Some(value) = source.get(from) {
        target.insert(to.to_owned(), value.clone());
    }
}

fn unix_timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
