export interface GitLabUser {
  id: number;
  name: string;
  username: string;
  email: string;
  avatar_url: string;
}

export interface GitLabProject {
  id: number;
  name: string;
  description: string;
  web_url: string;
  avatar_url: string;
  git_ssh_url: string;
  git_http_url: string;
  namespace: string;
  visibility_level: number;
  path_with_namespace: string;
  default_branch: string;
  homepage: string;
  url: string;
  ssh_url: string;
  http_url: string;
}

export interface GitLabCommit {
  id: string;
  message: string;
  title: string;
  timestamp: string;
  url: string;
  author: {
    name: string;
    email: string;
  };
  added: string[];
  modified: string[];
  removed: string[];
}

export interface GitLabRepository {
  name: string;
  url: string;
  description: string;
  homepage: string;
  git_http_url: string;
  git_ssh_url: string;
  visibility_level: number;
}

export interface GitLabPushEvent {
  object_kind: 'push';
  event_name: 'push';
  before: string;
  after: string;
  ref: string;
  checkout_sha: string;
  message: string;
  user_id: number;
  user_name: string;
  user_username: string;
  user_email: string;
  user_avatar: string;
  project_id: number;
  project: GitLabProject;
  commits: GitLabCommit[];
  total_commits_count: number;
  push_options: Record<string, any>;
  repository: GitLabRepository;
}

export interface GitLabMergeRequestEvent {
  object_kind: 'merge_request';
  event_type: 'merge_request';
  user: GitLabUser;
  project: GitLabProject;
  object_attributes: {
    id: number;
    target_branch: string;
    source_branch: string;
    source_project_id: number;
    author_id: number;
    assignee_id: number;
    title: string;
    created_at: string;
    updated_at: string;
    milestone_id: number;
    state: string;
    merge_status: string;
    target_project_id: number;
    iid: number;
    description: string;
    source: GitLabProject;
    target: GitLabProject;
    last_commit: GitLabCommit;
    work_in_progress: boolean;
    url: string;
    action: string;
    assignee: GitLabUser;
  };
  labels: any[];
  changes: Record<string, any>;
  repository: GitLabRepository;
}

export interface GitLabIssueEvent {
  object_kind: 'issue';
  event_type: 'issue';
  user: GitLabUser;
  project: GitLabProject;
  object_attributes: {
    id: number;
    title: string;
    assignee_ids: number[];
    assignee_id: number;
    author_id: number;
    project_id: number;
    created_at: string;
    updated_at: string;
    position: number;
    branch_name: string;
    description: string;
    milestone_id: number;
    state: string;
    iid: number;
    url: string;
    action: string;
  };
  labels: any[];
  changes: Record<string, any>;
  repository: GitLabRepository;
}

export interface GitLabPipelineEvent {
  object_kind: 'pipeline';
  object_attributes: {
    id: number;
    ref: string;
    tag: boolean;
    sha: string;
    before_sha: string;
    source: string;
    status: string;
    stages: string[];
    created_at: string;
    finished_at: string;
    duration: number;
    variables: any[];
  };
  merge_request: {
    id: number;
    iid: number;
    title: string;
    source_branch: string;
    source_project_id: number;
    target_branch: string;
    target_project_id: number;
    state: string;
    merge_status: string;
    url: string;
  };
  user: GitLabUser;
  project: GitLabProject;
  commit: GitLabCommit;
  builds: any[];
}

export interface GitLabTagPushEvent {
  object_kind: 'tag_push';
  event_name: 'tag_push';
  before: string;
  after: string;
  ref: string;
  checkout_sha: string;
  message: string;
  user_id: number;
  user_name: string;
  user_avatar: string;
  project_id: number;
  project: GitLabProject;
  commits: GitLabCommit[];
  total_commits_count: number;
  repository: GitLabRepository;
}

export type GitLabWebhookEvent =
  | GitLabPushEvent
  | GitLabMergeRequestEvent
  | GitLabIssueEvent
  | GitLabPipelineEvent
  | GitLabTagPushEvent;
