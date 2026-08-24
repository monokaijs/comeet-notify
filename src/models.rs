use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Clone, Debug, Deserialize)]
pub struct GitLabProject {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub web_url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GitLabUser {
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PushEvent {
    #[serde(default)]
    pub r#ref: String,
    #[serde(default)]
    pub checkout_sha: Option<String>,
    #[serde(default)]
    pub user_name: String,
    pub project_id: i64,
    pub project: GitLabProject,
    #[serde(default)]
    pub total_commits_count: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MergeRequestAttributes {
    #[serde(default)]
    pub target_branch: String,
    #[serde(default)]
    pub source_branch: String,
    #[serde(default)]
    pub title: String,
    pub iid: i64,
    #[serde(default)]
    pub action: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MergeRequestEvent {
    pub user: GitLabUser,
    pub project: GitLabProject,
    pub object_attributes: MergeRequestAttributes,
}

#[derive(Clone, Debug, Deserialize)]
pub struct IssueAttributes {
    #[serde(default)]
    pub title: String,
    pub iid: i64,
    #[serde(default)]
    pub action: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct IssueEvent {
    pub user: GitLabUser,
    pub project: GitLabProject,
    pub object_attributes: IssueAttributes,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PipelineAttributes {
    pub id: i64,
    #[serde(default)]
    pub r#ref: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub stages: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PipelineBuild {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub allow_failure: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PipelineEvent {
    pub object_attributes: PipelineAttributes,
    pub project: GitLabProject,
    #[serde(default)]
    pub builds: Vec<PipelineBuild>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TagPushEvent {
    #[serde(default)]
    pub r#ref: String,
    #[serde(default)]
    pub user_name: String,
    pub project_id: i64,
    pub project: GitLabProject,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "object_kind")]
pub enum GitLabWebhookEvent {
    #[serde(rename = "push")]
    Push(PushEvent),
    #[serde(rename = "merge_request")]
    MergeRequest(MergeRequestEvent),
    #[serde(rename = "issue")]
    Issue(IssueEvent),
    #[serde(rename = "pipeline")]
    Pipeline(PipelineEvent),
    #[serde(rename = "tag_push")]
    TagPush(TagPushEvent),
    #[serde(other)]
    Unknown,
}

impl GitLabWebhookEvent {
    pub fn object_kind(&self) -> &'static str {
        match self {
            Self::Push(_) => "push",
            Self::MergeRequest(_) => "merge_request",
            Self::Issue(_) => "issue",
            Self::Pipeline(_) => "pipeline",
            Self::TagPush(_) => "tag_push",
            Self::Unknown => "unknown",
        }
    }

    pub fn pipeline(&self) -> Option<&PipelineEvent> {
        match self {
            Self::Pipeline(pipeline) => Some(pipeline),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookSuccessResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookErrorResponse {
    pub status_code: u16,
    pub timestamp: String,
    pub path: String,
    pub method: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct GitLabWebhookDto {
    pub object_kind: String,
    #[serde(flatten)]
    #[schema(value_type = Object)]
    pub body: std::collections::HashMap<String, Value>,
}
