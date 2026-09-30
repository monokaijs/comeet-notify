use std::{collections::BTreeMap, sync::OnceLock};

use comeet_notify::{
    FcmClient,
    config::FirebaseConfig,
    fcm::{Notification, live_activity_request, notification_request},
    live_activity::{build_start, build_update},
    models::{GitLabProject, PipelineAttributes, PipelineBuild, PipelineEvent},
};
use jsonwebtoken::{Algorithm, dangerous::insecure_decode};
use rand::rngs::OsRng;
use rsa::{
    RsaPrivateKey,
    pkcs8::{EncodePrivateKey, LineEnding},
};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, header, method, path},
};

fn private_key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        RsaPrivateKey::new(&mut OsRng, 2048)
            .unwrap()
            .to_pkcs8_pem(LineEnding::LF)
            .unwrap()
            .to_string()
    })
}

fn config() -> FirebaseConfig {
    FirebaseConfig {
        project_id: "comeet-test".into(),
        private_key: private_key().into(),
        client_email: "firebase@comeet-test.iam.gserviceaccount.com".into(),
    }
}

fn notification() -> Notification {
    Notification {
        title: "Pipeline started".into(),
        body: "Pipeline for main is now running".into(),
        data: BTreeMap::from([
            ("eventType".into(), "pipeline".into()),
            ("pipeline_id".into(), "42".into()),
        ]),
    }
}

fn pipeline(status: &str) -> PipelineEvent {
    PipelineEvent {
        object_attributes: PipelineAttributes {
            id: 42,
            r#ref: "main".into(),
            status: status.into(),
            stages: vec!["build".into()],
        },
        project: GitLabProject {
            id: 7,
            name: "Comeet".into(),
            web_url: "https://gitlab/comeet".into(),
        },
        builds: vec![PipelineBuild {
            id: 1,
            name: "compile".into(),
            stage: "build".into(),
            status: status.into(),
            allow_failure: false,
        }],
    }
}

fn client(server: &MockServer, mut config: FirebaseConfig) -> FcmClient {
    config.project_id = "comeet-test".into();
    FcmClient::with_endpoints(
        config,
        format!("{}/token", server.uri()),
        format!("{}/v1/projects", server.uri()),
    )
}

async fn mount_success(server: &MockServer, expires_in: i64, token_calls: u64, sends: u64) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "oauth-access-token", "expires_in": expires_in
        })))
        .expect(token_calls)
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/comeet-test/messages:send"))
        .and(header("authorization", "Bearer oauth-access-token"))
        .and(body_json(notification_request(
            "fcm-token",
            &notification(),
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "projects/comeet-test/messages/123"
        })))
        .expect(sends)
        .mount(server)
        .await;
}

#[test]
fn regular_notification_has_firebase_admin_parity() {
    assert_eq!(
        notification_request("fcm-token", &notification()),
        json!({"message": {
            "notification": {"title": "Pipeline started", "body": "Pipeline for main is now running"},
            "data": {"eventType": "pipeline", "pipeline_id": "42"},
            "token": "fcm-token",
            "android": {"notification": {"channel_id": "gitlab_notifications", "notification_priority": "PRIORITY_HIGH"}},
            "apns": {"payload": {"aps": {
                "alert": {"title": "Pipeline started", "body": "Pipeline for main is now running"},
                "badge": 1, "sound": "default"
            }}}
        }})
    );
}

#[test]
fn live_activity_update_end_and_start_have_activitykit_names() {
    let update = live_activity_request(
        "fcm",
        "activity",
        &build_update(&pipeline("running"), 1_786_320_000),
        Some("comeet-7-42"),
    )
    .unwrap();
    let aps = &update["message"]["apns"]["payload"]["aps"];
    assert_eq!(update["message"]["apns"]["live_activity_token"], "activity");
    assert_eq!(update["message"]["apns"]["headers"]["apns-priority"], "10");
    assert_eq!(
        update["message"]["apns"]["headers"]["apns-collapse-id"],
        "comeet-7-42"
    );
    assert_eq!(aps["event"], "update");
    assert_eq!(aps["stale-date"], 1_786_320_900_i64);
    assert_eq!(aps["content-state"]["status"], "running");
    assert_eq!(aps["content-state"]["completedJobCount"], 0);
    assert_eq!(aps["content-state"]["totalJobCount"], 1);
    assert_eq!(aps["content-state"]["stages"][0]["completedJobCount"], 0);
    assert_eq!(aps["content-state"]["stages"][0]["totalJobCount"], 1);

    let end = live_activity_request(
        "fcm",
        "activity",
        &build_update(&pipeline("failed"), 1_786_320_000),
        None,
    )
    .unwrap();
    let aps = &end["message"]["apns"]["payload"]["aps"];
    assert_eq!(aps["event"], "end");
    assert_eq!(aps["dismissal-date"], 1_786_323_600_i64);
    assert!(aps.get("stale-date").is_none());

    let start = live_activity_request(
        "fcm",
        "start-token",
        &build_start(&pipeline("running"), 1_786_320_000, Some("work")),
        None,
    )
    .unwrap();
    let aps = &start["message"]["apns"]["payload"]["aps"];
    assert_eq!(aps["event"], "start");
    assert_eq!(aps["input-push-token"], 1);
    assert_eq!(aps["attributes-type"], "PipelineActivityAttributes");
    assert_eq!(aps["attributes"]["instanceId"], "work");
    assert_eq!(aps["alert"]["sound"], "default");
}

#[tokio::test]
async fn assertion_claims_and_success_id_are_correct() {
    let server = MockServer::start().await;
    mount_success(&server, 3600, 1, 1).await;
    let result = client(&server, config())
        .send_notification("fcm-token", &notification())
        .await;
    assert_eq!(
        result.message_id.as_deref(),
        Some("projects/comeet-test/messages/123")
    );
    let requests = server.received_requests().await.unwrap();
    let request = requests
        .iter()
        .find(|request| request.url.path() == "/token")
        .unwrap();
    let form = String::from_utf8(request.body.clone()).unwrap();
    let assertion = form
        .split('&')
        .find_map(|pair| pair.split_once('=').filter(|(key, _)| *key == "assertion"))
        .map(|(_, value)| urlencoding::decode(value).unwrap().into_owned())
        .unwrap();
    let decoded = insecure_decode::<Value>(&assertion).unwrap();
    assert_eq!(decoded.header.alg, Algorithm::RS256);
    assert_eq!(
        decoded.claims["iss"],
        "firebase@comeet-test.iam.gserviceaccount.com"
    );
    assert_eq!(
        decoded.claims["scope"],
        "https://www.googleapis.com/auth/firebase.messaging"
    );
    assert_eq!(decoded.claims["aud"], format!("{}/token", server.uri()));
    assert_eq!(
        decoded.claims["exp"].as_i64().unwrap() - decoded.claims["iat"].as_i64().unwrap(),
        3600
    );
}

#[tokio::test]
async fn escaped_newlines_and_cached_token_work() {
    let server = MockServer::start().await;
    mount_success(&server, 3600, 1, 2).await;
    let mut escaped = config();
    escaped.private_key = escaped.private_key.replace('\n', "\\n");
    let client = client(&server, escaped);
    assert!(client.is_enabled());
    assert!(
        client
            .send_notification("fcm-token", &notification())
            .await
            .success
    );
    assert!(
        client
            .send_notification("fcm-token", &notification())
            .await
            .success
    );
}

#[tokio::test]
async fn concurrent_refresh_is_serialized() {
    let server = MockServer::start().await;
    mount_success(&server, 3600, 1, 8).await;
    let client = client(&server, config());
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let client = client.clone();
        tasks.spawn(async move { client.send_notification("fcm-token", &notification()).await });
    }
    while let Some(result) = tasks.join_next().await {
        assert!(result.unwrap().success);
    }
}

#[tokio::test]
async fn sixty_second_margin_forces_refresh() {
    let server = MockServer::start().await;
    mount_success(&server, 60, 2, 2).await;
    let client = client(&server, config());
    assert!(
        client
            .send_notification("fcm-token", &notification())
            .await
            .success
    );
    assert!(
        client
            .send_notification("fcm-token", &notification())
            .await
            .success
    );
}

#[tokio::test]
async fn token_and_fcm_errors_are_reported_without_panics() {
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(500).set_body_string("unavailable"))
        .mount(&token_server)
        .await;
    let result = client(&token_server, config())
        .send_notification("fcm-token", &notification())
        .await;
    assert!(!result.success);
    assert!(
        result
            .error
            .unwrap()
            .contains("OAuth token endpoint returned 500")
    );

    let fcm_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token": "oauth-access-token", "expires_in": 3600})),
        )
        .mount(&fcm_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/comeet-test/messages:send"))
        .respond_with(ResponseTemplate::new(404).set_body_string("UNREGISTERED"))
        .mount(&fcm_server)
        .await;
    let result = client(&fcm_server, config())
        .send_notification("fcm-token", &notification())
        .await;
    assert_eq!(result.error.as_deref(), Some("Invalid FCM token"));
}

#[test]
fn absent_or_invalid_credentials_disable_fcm() {
    assert!(!FcmClient::new(None).is_enabled());
    let mut invalid = config();
    invalid.private_key = "invalid".into();
    assert!(!FcmClient::new(Some(invalid)).is_enabled());
}
