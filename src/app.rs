use std::time::Duration;

use axum::{
    Json, Router,
    body::Body,
    extract::{
        DefaultBodyLimit, FromRequest, OriginalUri, Request, State, rejection::JsonRejection,
    },
    http::{HeaderMap, Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use tower_http::{
    cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use tracing::{Span, info, warn};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    fcm::FcmClient,
    models::{GitLabWebhookDto, GitLabWebhookEvent, WebhookErrorResponse, WebhookSuccessResponse},
    webhooks::{LiveActivityHeaders, WebhookProcessor},
};

const MAX_JSON_BODY_BYTES: usize = 100 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub processor: WebhookProcessor,
}

impl AppState {
    pub fn new(fcm: FcmClient) -> Self {
        Self {
            processor: WebhookProcessor::new(fcm),
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Comeet Notify API",
        description = "GitLab webhook notification service API",
        version = "1.0"
    ),
    tags(
        (name = "webhooks", description = "GitLab webhook endpoints"),
        (name = "app", description = "Application endpoints")
    ),
    paths(root_get, root_post, handle_gitlab_webhook),
    components(schemas(GitLabWebhookDto, WebhookSuccessResponse, WebhookErrorResponse))
)]
struct ApiDoc;

pub fn build_app(state: AppState) -> Router {
    let openapi = ApiDoc::openapi();
    let openapi_yaml = serde_yaml::to_string(&openapi).expect("OpenAPI serializes as YAML");
    let docs_yaml = move || {
        let body = openapi_yaml.clone();
        async move { ([(header::CONTENT_TYPE, "application/yaml")], body) }
    };

    Router::new()
        .route("/", get(root_get).post(root_post))
        .route("/webhooks/gitlab", post(handle_gitlab_webhook))
        .route("/docs-yaml", get(docs_yaml))
        .merge(SwaggerUi::new("/docs").url("/docs-json", openapi))
        .layer(middleware::from_fn(docs_root_compatibility))
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::mirror_request())
                .allow_credentials(true)
                .allow_methods(AllowMethods::mirror_request())
                .allow_headers(AllowHeaders::mirror_request()),
        )
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<Body>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        path = %request.uri(),
                        status = tracing::field::Empty,
                        latency_ms = tracing::field::Empty,
                    )
                })
                .on_request(|request: &Request<Body>, _span: &Span| {
                    info!(method = %request.method(), path = %request.uri(), "Incoming request");
                })
                .on_response(
                    |response: &Response<Body>, latency: Duration, span: &Span| {
                        span.record("status", response.status().as_u16());
                        span.record("latency_ms", latency.as_millis() as u64);
                        info!(
                            status = response.status().as_u16(),
                            latency_ms = latency.as_millis() as u64,
                            "Request completed"
                        );
                    },
                ),
        )
        .with_state(state)
}

async fn docs_root_compatibility(request: Request, next: Next) -> Response {
    if request.uri().path() == "/docs" {
        return Html(
            r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=/docs/"><title>Comeet Notify API</title></head><body><a href="/docs/">Open Swagger UI</a></body></html>"#,
        )
        .into_response();
    }
    next.run(request).await
}

#[utoipa::path(
    get,
    path = "/",
    tag = "app",
    responses((status = 200, description = "Application is running", body = String))
)]
async fn root_get() -> impl IntoResponse {
    root_response()
}

#[utoipa::path(
    post,
    path = "/",
    tag = "app",
    responses((status = 200, description = "Application is running", body = String))
)]
async fn root_post() -> impl IntoResponse {
    root_response()
}

fn root_response() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/plain")], "Hello World!")
}

#[utoipa::path(
    post,
    path = "/webhooks/gitlab",
    tag = "webhooks",
    request_body = GitLabWebhookDto,
    params(
        ("x-gitlab-event" = Option<String>, Header, description = "GitLab event type"),
        ("x-fcm-token" = String, Header, description = "Firebase Cloud Messaging token"),
        ("x-live-activity-token" = Option<String>, Header, description = "ActivityKit update token"),
        ("x-live-activity-pipeline-id" = Option<String>, Header, description = "Pipeline ID for the update token"),
        ("x-live-activity-registrations" = Option<String>, Header, description = "JSON ActivityKit registrations"),
        ("x-live-activity-push-to-start-token" = Option<String>, Header, description = "ActivityKit push-to-start token"),
        ("x-comeet-instance-id" = Option<String>, Header, description = "Comeet GitLab instance identifier"),
        ("x-pipeline-delivery-mode" = Option<String>, Header, description = "live_activity, notification, or both")
    ),
    responses(
        (status = 201, description = "Webhook processed successfully", body = WebhookSuccessResponse),
        (status = 400, description = "Invalid webhook request", body = WebhookErrorResponse)
    )
)]
async fn handle_gitlab_webhook(
    State(state): State<AppState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let parsed = Json::<GitLabWebhookEvent>::from_request(request, &state).await;
    let payload = match parsed {
        Ok(Json(payload)) => payload,
        Err(rejection) => {
            return webhook_json_error(rejection, &method, &uri.to_string());
        }
    };
    let fcm_token = header_value(&headers, "x-fcm-token").unwrap_or_default();
    if fcm_token.is_empty() {
        return webhook_error(
            StatusCode::BAD_REQUEST,
            &method,
            &uri.to_string(),
            "Missing FCM token in X-FCM-Token header",
        );
    }

    info!(
        event = header_value(&headers, "x-gitlab-event").unwrap_or(payload.object_kind()),
        "Received GitLab webhook"
    );
    let live_activity = LiveActivityHeaders {
        token: owned_header(&headers, "x-live-activity-token"),
        pipeline_id: owned_header(&headers, "x-live-activity-pipeline-id"),
        registrations: owned_header(&headers, "x-live-activity-registrations"),
        push_to_start_token: owned_header(&headers, "x-live-activity-push-to-start-token"),
        instance_id: owned_header(&headers, "x-comeet-instance-id"),
        pipeline_delivery_mode: owned_header(&headers, "x-pipeline-delivery-mode"),
    };
    match state
        .processor
        .process(&payload, fcm_token, &live_activity)
        .await
    {
        Ok(()) => (
            StatusCode::CREATED,
            Json(WebhookSuccessResponse {
                success: true,
                message: "Webhook processed successfully".to_owned(),
            }),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, "Error processing webhook");
            webhook_error(
                StatusCode::BAD_REQUEST,
                &method,
                &uri.to_string(),
                "Failed to process webhook",
            )
        }
    }
}

fn webhook_json_error(rejection: JsonRejection, method: &Method, path: &str) -> Response {
    let status = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        StatusCode::BAD_REQUEST
    };
    webhook_error(status, method, path, &rejection.body_text())
}

fn webhook_error(status: StatusCode, method: &Method, path: &str, message: &str) -> Response {
    (
        status,
        Json(WebhookErrorResponse {
            status_code: status.as_u16(),
            timestamp: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .expect("UTC time formats as RFC3339"),
            path: path.to_owned(),
            method: method.to_string(),
            message: message.to_owned(),
        }),
    )
        .into_response()
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn owned_header(headers: &HeaderMap, name: &str) -> Option<String> {
    header_value(headers, name).map(ToOwned::to_owned)
}
