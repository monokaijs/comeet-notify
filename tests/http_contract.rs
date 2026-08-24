use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use comeet_notify::{AppState, FcmClient, build_app};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn app() -> Router {
    build_app(AppState::new(FcmClient::disabled()))
}

fn request(method: Method, path: &str, body: impl Into<Body>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(body.into())
        .unwrap()
}

async fn body(response: axum::response::Response) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

fn unknown() -> String {
    json!({"object_kind": "wiki_page"}).to_string()
}

#[tokio::test]
async fn root_get_and_post_are_plain_text_200() {
    for method in [Method::GET, Method::POST] {
        let response = app()
            .oneshot(request(method, "/", Body::empty()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain");
        assert_eq!(body(response).await, "Hello World!");
    }
}

#[tokio::test]
async fn unknown_webhook_is_acknowledged_with_201_json() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/gitlab")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-fcm-token", "device")
                .body(Body::from(unknown()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        serde_json::from_str::<Value>(&body(response).await).unwrap(),
        json!({"success": true, "message": "Webhook processed successfully"})
    );
}

#[tokio::test]
async fn missing_token_has_structured_400_shape() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/gitlab?source=test")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(unknown()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(error["statusCode"], 400);
    assert_eq!(error["path"], "/webhooks/gitlab?source=test");
    assert_eq!(error["method"], "POST");
    assert_eq!(error["message"], "Missing FCM token in X-FCM-Token header");
    assert!(error["timestamp"].as_str().unwrap().contains('T'));
}

#[tokio::test]
async fn malformed_json_is_structured_400() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/gitlab")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-fcm-token", "device")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(error["statusCode"], 400);
    assert!(error["message"].as_str().unwrap().contains("JSON"));
}

#[tokio::test]
async fn json_body_limit_is_100_kib() {
    let oversized = json!({"object_kind": "wiki_page", "padding": "x".repeat(101 * 1024)});
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/gitlab")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-fcm-token", "device")
                .body(Body::from(oversized.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let error: Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(error["statusCode"], 413);
}

#[tokio::test]
async fn cors_mirrors_origin_with_credentials() {
    let response = app()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/webhooks/gitlab")
                .header(header::ORIGIN, "https://comeet.example")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(
                    header::ACCESS_CONTROL_REQUEST_HEADERS,
                    "content-type,x-fcm-token",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://comeet.example"
    );
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_CREDENTIALS],
        "true"
    );
}

#[tokio::test]
async fn swagger_json_yaml_and_ui_are_available() {
    for path in ["/docs", "/docs/", "/docs-json", "/docs-yaml"] {
        let response = app()
            .oneshot(request(Method::GET, path, Body::empty()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(!body(response).await.is_empty());
    }
    let response = app()
        .oneshot(request(Method::GET, "/docs-json", Body::empty()))
        .await
        .unwrap();
    let spec: Value = serde_json::from_str(&body(response).await).unwrap();
    assert!(spec["paths"]["/"]["get"].is_object());
    assert!(spec["paths"]["/"]["post"].is_object());
    assert!(spec["paths"]["/webhooks/gitlab"]["post"]["responses"]["201"].is_object());
}

#[tokio::test]
async fn missing_firebase_still_acknowledges_known_webhook() {
    let pipeline = json!({
        "object_kind": "pipeline",
        "project": {"id": 7, "name": "Comeet", "web_url": "https://gitlab/comeet"},
        "object_attributes": {"id": 42, "ref": "main", "status": "running", "stages": []},
        "builds": []
    });
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/webhooks/gitlab")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-fcm-token", "device")
                .header("x-live-activity-push-to-start-token", "a".repeat(64))
                .body(Body::from(pipeline.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}
