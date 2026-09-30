use std::sync::OnceLock;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use comeet_notify::{AppState, FcmClient, build_app, config::FirebaseConfig};
use rand::rngs::OsRng;
use rsa::{
    RsaPrivateKey,
    pkcs8::{EncodePrivateKey, LineEnding},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
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

fn app(server: &MockServer) -> Router {
    let fcm = FcmClient::with_endpoints(
        FirebaseConfig {
            project_id: "comeet-test".into(),
            private_key: private_key().into(),
            client_email: "firebase@comeet-test.iam.gserviceaccount.com".into(),
        },
        format!("{}/token", server.uri()),
        format!("{}/v1/projects", server.uri()),
    );
    build_app(AppState::new(fcm))
}

async fn mock_server(fcm_status: u16) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access-token", "expires_in": 3600
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/projects/comeet-test/messages:send"))
        .respond_with(
            ResponseTemplate::new(fcm_status)
                .set_body_json(json!({"name": "projects/comeet-test/messages/1"})),
        )
        .mount(&server)
        .await;
    server
}

fn pipeline(status: &str, id: i64) -> Value {
    json!({
        "object_kind": "pipeline",
        "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
        "object_attributes": {"id": id, "ref": "main", "status": status, "stages": ["build"]},
        "builds": [{"id": 1, "name": "compile", "stage": "build", "status": status}]
    })
}

fn pipeline_with_parallel_jobs(status: &str, id: i64) -> Value {
    json!({
        "object_kind": "pipeline",
        "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
        "object_attributes": {"id": id, "ref": "main", "status": status, "stages": ["build"]},
        "builds": [
            {"id": 1, "name": "web", "stage": "build", "status": "running"},
            {"id": 2, "name": "ios", "stage": "build", "status": "pending"}
        ]
    })
}

fn job(build_id: i64, build_status: &str, pipeline_status: &str, pipeline_id: i64) -> Value {
    json!({
        "object_kind": "build",
        "ref": "main",
        "build_id": build_id,
        "build_name": if build_id == 1 { "web" } else { "ios" },
        "build_stage": "build",
        "build_status": build_status,
        "build_allow_failure": false,
        "pipeline_id": pipeline_id,
        "project_id": 7,
        "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
        "commit": {"id": pipeline_id, "status": pipeline_status}
    })
}

async fn send(app: Router, payload: Value, headers: &[(&str, String)]) -> StatusCode {
    let mut request = Request::builder()
        .method("POST")
        .uri("/webhooks/gitlab")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-fcm-token", "fcm-token");
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    app.oneshot(request.body(Body::from(payload.to_string())).unwrap())
        .await
        .unwrap()
        .status()
}

async fn messages(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.url.path().ends_with("messages:send"))
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect()
}

fn is_regular(message: &&Value) -> bool {
    message["message"].get("notification").is_some()
}

fn is_live(message: &&Value) -> bool {
    message["message"]["apns"]
        .get("live_activity_token")
        .is_some()
}

#[tokio::test]
async fn matching_legacy_token_sends_both_channels_with_instance_data() {
    let server = mock_server(200).await;
    let status = send(
        app(&server),
        pipeline("running", 42),
        &[
            ("x-live-activity-token", "a".repeat(64)),
            ("x-live-activity-pipeline-id", "42".into()),
            ("x-comeet-instance-id", "work".into()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let messages = messages(&server).await;
    assert_eq!(messages.iter().filter(is_regular).count(), 1);
    assert_eq!(messages.iter().filter(is_live).count(), 1);
    let regular = messages.iter().find(is_regular).unwrap();
    assert_eq!(regular["message"]["data"]["instance_id"], "work");
}

#[tokio::test]
async fn token_for_other_pipeline_does_not_send_live_update() {
    let server = mock_server(200).await;
    send(
        app(&server),
        pipeline("running", 42),
        &[
            ("x-live-activity-token", "b".repeat(64)),
            ("x-live-activity-pipeline-id", "41".into()),
        ],
    )
    .await;
    let messages = messages(&server).await;
    assert_eq!(messages.iter().filter(is_regular).count(), 1);
    assert_eq!(messages.iter().filter(is_live).count(), 0);
}

#[tokio::test]
async fn push_to_start_is_used_unless_update_token_exists() {
    let server = mock_server(200).await;
    send(
        app(&server),
        pipeline("running", 42),
        &[("x-live-activity-push-to-start-token", "c".repeat(64))],
    )
    .await;
    let sent = messages(&server).await;
    let live = sent.iter().find(is_live).unwrap();
    assert_eq!(
        live["message"]["apns"]["live_activity_token"],
        "c".repeat(64)
    );
    assert_eq!(live["message"]["apns"]["payload"]["aps"]["event"], "start");

    let server = mock_server(200).await;
    send(
        app(&server),
        pipeline("running", 42),
        &[
            ("x-live-activity-token", "d".repeat(64)),
            ("x-live-activity-pipeline-id", "42".into()),
            ("x-live-activity-push-to-start-token", "e".repeat(64)),
        ],
    )
    .await;
    let sent = messages(&server).await;
    let live = sent.iter().find(is_live).unwrap();
    assert_eq!(
        live["message"]["apns"]["live_activity_token"],
        "d".repeat(64)
    );
    assert_eq!(live["message"]["apns"]["payload"]["aps"]["event"], "update");
}

#[tokio::test]
async fn delivery_modes_select_exact_channels() {
    for (mode, regular, live) in [
        ("live_activity", 0, 1),
        ("notification", 1, 0),
        ("both", 1, 1),
        ("invalid", 1, 1),
    ] {
        let server = mock_server(200).await;
        send(
            app(&server),
            pipeline("running", 42),
            &[
                ("x-pipeline-delivery-mode", mode.into()),
                ("x-live-activity-token", "f".repeat(64)),
                ("x-live-activity-pipeline-id", "42".into()),
            ],
        )
        .await;
        let sent = messages(&server).await;
        assert_eq!(sent.iter().filter(is_regular).count(), regular, "{mode}");
        assert_eq!(sent.iter().filter(is_live).count(), live, "{mode}");
    }
}

#[tokio::test]
async fn registrations_match_only_newest_eight_and_expired_does_not_fallback() {
    let server = mock_server(200).await;
    let registrations: Vec<_> = (0..9)
        .map(|index| {
            json!({
                "pipelineId": if index == 0 { 42 } else { 100 + index },
                "pushToken": format!("{:x}", index + 1).repeat(64)
            })
        })
        .collect();
    send(
        app(&server),
        pipeline("running", 42),
        &[
            (
                "x-live-activity-registrations",
                serde_json::to_string(&registrations).unwrap(),
            ),
            ("x-live-activity-push-to-start-token", "a".repeat(64)),
        ],
    )
    .await;
    let sent = messages(&server).await;
    let live = sent.iter().find(is_live).unwrap();
    assert_eq!(
        live["message"]["apns"]["live_activity_token"],
        "a".repeat(64)
    );

    let server = mock_server(200).await;
    let expired = json!([{"pipelineId": 42, "pushToken": "b".repeat(64), "registeredAt": 1}]);
    send(
        app(&server),
        pipeline("running", 42),
        &[
            ("x-live-activity-registrations", expired.to_string()),
            ("x-live-activity-token", "b".repeat(64)),
            ("x-live-activity-pipeline-id", "42".into()),
            ("x-live-activity-push-to-start-token", "c".repeat(64)),
        ],
    )
    .await;
    let sent = messages(&server).await;
    let live = sent.iter().find(is_live).unwrap();
    assert_eq!(
        live["message"]["apns"]["live_activity_token"],
        "c".repeat(64)
    );
}

#[tokio::test]
async fn malformed_registration_falls_back_to_legacy() {
    let server = mock_server(200).await;
    send(
        app(&server),
        pipeline("running", 42),
        &[
            ("x-live-activity-registrations", "bad-json".into()),
            ("x-live-activity-token", "d".repeat(64)),
            ("x-live-activity-pipeline-id", "42".into()),
        ],
    )
    .await;
    let sent = messages(&server).await;
    let live = sent.iter().find(is_live).unwrap();
    assert_eq!(
        live["message"]["apns"]["live_activity_token"],
        "d".repeat(64)
    );
}

#[tokio::test]
async fn concurrent_remote_starts_are_deduplicated() {
    let server = mock_server(200).await;
    let app = app(&server);
    let headers = [("x-live-activity-push-to-start-token", "e".repeat(64))];
    let (first, second) = tokio::join!(
        send(app.clone(), pipeline("running", 99), &headers),
        send(app, pipeline("running", 99), &headers),
    );
    assert_eq!(first, StatusCode::CREATED);
    assert_eq!(second, StatusCode::CREATED);
    let sent = messages(&server).await;
    assert_eq!(sent.iter().filter(is_regular).count(), 2);
    assert_eq!(sent.iter().filter(is_live).count(), 1);
}

#[tokio::test]
async fn terminal_payload_ends_and_fcm_failure_still_returns_201() {
    let server = mock_server(200).await;
    send(
        app(&server),
        pipeline("failed", 42),
        &[
            ("x-live-activity-token", "f".repeat(64)),
            ("x-live-activity-pipeline-id", "42".into()),
        ],
    )
    .await;
    let sent = messages(&server).await;
    let live = sent.iter().find(is_live).unwrap();
    assert_eq!(live["message"]["apns"]["payload"]["aps"]["event"], "end");
    assert_eq!(
        live["message"]["apns"]["headers"]["apns-collapse-id"],
        "comeet-7-42"
    );

    let server = mock_server(503).await;
    assert_eq!(
        send(app(&server), pipeline("running", 42), &[]).await,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn job_hooks_update_cached_job_progress_without_regular_notifications() {
    let server = mock_server(200).await;
    let app = app(&server);
    assert_eq!(
        send(
            app.clone(),
            pipeline_with_parallel_jobs("running", 42),
            &[("x-pipeline-delivery-mode", "live_activity".into())],
        )
        .await,
        StatusCode::CREATED
    );
    assert!(messages(&server).await.is_empty());

    assert_eq!(
        send(
            app,
            job(2, "success", "running", 42),
            &[
                ("x-live-activity-token", "a".repeat(64)),
                ("x-live-activity-pipeline-id", "42".into()),
            ],
        )
        .await,
        StatusCode::CREATED
    );
    let sent = messages(&server).await;
    assert_eq!(sent.iter().filter(is_regular).count(), 0);
    assert_eq!(sent.iter().filter(is_live).count(), 1);
    let state = &sent[0]["message"]["apns"]["payload"]["aps"]["content-state"];
    assert_eq!(state["status"], "running");
    assert_eq!(state["completedJobCount"], 1);
    assert_eq!(state["totalJobCount"], 2);
    assert_eq!(state["stages"][0]["completedJobCount"], 1);
    assert_eq!(state["stages"][0]["totalJobCount"], 2);
}

#[tokio::test]
async fn terminal_job_hook_ends_the_cached_live_activity() {
    let server = mock_server(200).await;
    let app = app(&server);
    send(
        app.clone(),
        pipeline_with_parallel_jobs("running", 42),
        &[("x-pipeline-delivery-mode", "live_activity".into())],
    )
    .await;
    send(
        app,
        job(1, "failed", "failed", 42),
        &[
            ("x-live-activity-token", "b".repeat(64)),
            ("x-live-activity-pipeline-id", "42".into()),
        ],
    )
    .await;

    let sent = messages(&server).await;
    assert_eq!(sent.iter().filter(is_regular).count(), 0);
    assert_eq!(sent.iter().filter(is_live).count(), 1);
    let aps = &sent[0]["message"]["apns"]["payload"]["aps"];
    assert_eq!(aps["event"], "end");
    assert_eq!(aps["content-state"]["failedJobName"], "web");
}

#[tokio::test]
async fn job_hooks_require_a_snapshot_and_live_activity_delivery() {
    let server = mock_server(200).await;
    let app = app(&server);
    assert_eq!(
        send(
            app.clone(),
            job(2, "success", "running", 42),
            &[
                ("x-live-activity-token", "c".repeat(64)),
                ("x-live-activity-pipeline-id", "42".into()),
            ],
        )
        .await,
        StatusCode::CREATED
    );
    assert!(messages(&server).await.is_empty());

    send(
        app.clone(),
        pipeline_with_parallel_jobs("running", 42),
        &[("x-pipeline-delivery-mode", "live_activity".into())],
    )
    .await;
    send(
        app,
        job(2, "success", "running", 42),
        &[
            ("x-pipeline-delivery-mode", "notification".into()),
            ("x-live-activity-token", "d".repeat(64)),
            ("x-live-activity-pipeline-id", "42".into()),
        ],
    )
    .await;
    assert!(messages(&server).await.is_empty());
}

#[tokio::test]
async fn job_hooks_accumulate_progress_until_pipeline_completion() {
    let server = mock_server(200).await;
    let app = app(&server);
    let headers = [
        ("x-pipeline-delivery-mode", "live_activity".into()),
        ("x-live-activity-token", "a".repeat(64)),
        ("x-live-activity-pipeline-id", "42".into()),
    ];
    send(
        app.clone(),
        pipeline_with_parallel_jobs("running", 42),
        &headers,
    )
    .await;
    send(app.clone(), job(1, "success", "running", 42), &headers).await;
    send(app, job(2, "success", "success", 42), &headers).await;

    let sent = messages(&server).await;
    assert_eq!(sent.len(), 3);
    assert!(sent.iter().all(|message| is_live(&message)));
    for (index, message) in sent.iter().enumerate() {
        let aps = &message["message"]["apns"]["payload"]["aps"];
        assert_eq!(aps["content-state"]["completedJobCount"], index);
        assert_eq!(aps["content-state"]["totalJobCount"], 2);
        assert_eq!(
            aps["content-state"]["stages"][0]["completedJobCount"],
            index
        );
    }
    assert_eq!(
        sent[1]["message"]["apns"]["payload"]["aps"]["event"],
        "update"
    );
    assert_eq!(sent[2]["message"]["apns"]["payload"]["aps"]["event"], "end");
}

#[tokio::test]
async fn job_hooks_do_not_remote_start_without_an_activity_registration() {
    let server = mock_server(200).await;
    let app = app(&server);
    let headers = [
        ("x-pipeline-delivery-mode", "live_activity".into()),
        ("x-live-activity-push-to-start-token", "a".repeat(64)),
    ];
    send(
        app.clone(),
        pipeline_with_parallel_jobs("pending", 42),
        &headers,
    )
    .await;
    send(app, job(1, "running", "running", 42), &headers).await;
    assert!(messages(&server).await.is_empty());
}

#[tokio::test]
async fn job_hook_snapshots_are_isolated_by_gitlab_instance() {
    let server = mock_server(200).await;
    let app = app(&server);
    let headers_one = [
        ("x-gitlab-instance", "https://one.example".into()),
        ("x-pipeline-delivery-mode", "live_activity".into()),
        ("x-live-activity-token", "a".repeat(64)),
        ("x-live-activity-pipeline-id", "42".into()),
    ];
    let headers_two = [
        ("x-gitlab-instance", "https://two.example".into()),
        ("x-pipeline-delivery-mode", "live_activity".into()),
        ("x-live-activity-token", "b".repeat(64)),
        ("x-live-activity-pipeline-id", "42".into()),
    ];
    let mut snapshot = pipeline_with_parallel_jobs("running", 42);
    snapshot["project"]["web_url"] = json!("");
    send(app.clone(), snapshot.clone(), &headers_one).await;
    send(app.clone(), snapshot, &headers_two).await;
    let mut completed_one = job(1, "success", "running", 42);
    completed_one["project"]["web_url"] = json!("");
    send(app.clone(), completed_one, &headers_one).await;
    let mut completed_two = job(2, "success", "running", 42);
    completed_two["project"]["web_url"] = json!("");
    send(app, completed_two, &headers_two).await;

    let sent = messages(&server).await;
    assert_eq!(sent.len(), 4);
    assert!(sent.iter().all(|message| is_live(&message)));
    for message in &sent[2..] {
        let state = &message["message"]["apns"]["payload"]["aps"]["content-state"];
        assert_eq!(state["completedJobCount"], 1);
        assert_eq!(state["totalJobCount"], 2);
    }
}
