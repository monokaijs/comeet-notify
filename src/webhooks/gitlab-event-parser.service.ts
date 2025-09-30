import { Injectable } from '@nestjs/common';
import {
  GitLabWebhookEvent,
  GitLabPushEvent,
  GitLabMergeRequestEvent,
  GitLabIssueEvent,
  GitLabPipelineEvent,
  GitLabTagPushEvent
} from './dto/gitlab-webhook.dto';

export enum GitLabEventType {
  PUSH = 'push',
  MERGE_REQUEST = 'merge_request',
  ISSUE = 'issue',
  PIPELINE = 'pipeline',
  TAG_PUSH = 'tag_push',
  WIKI_PAGE = 'wiki_page',
}

export interface ParsedNotificationData {
  eventType: GitLabEventType;
  title: string;
  message: string;
  repositoryName: string;
  repositoryUrl: string;
  deepLinkData: {
    event_type: string;
    project_id?: number;
    commit_sha?: string;
    issue_iid?: number;
    merge_request_iid?: number;
    pipeline_id?: number;
  };
}

@Injectable()
export class GitLabEventParserService {
  parseEvent(payload: GitLabWebhookEvent): ParsedNotificationData | null {
    switch (payload.object_kind) {
      case 'push':
        return this.parsePushEvent(payload as GitLabPushEvent);
      case 'merge_request':
        return this.parseMergeRequestEvent(payload as GitLabMergeRequestEvent);
      case 'issue':
        return this.parseIssueEvent(payload as GitLabIssueEvent);
      case 'pipeline':
        return this.parsePipelineEvent(payload as GitLabPipelineEvent);
      case 'tag_push':
        return this.parseTagPushEvent(payload as GitLabTagPushEvent);
      default:
        return null;
    }
  }

  private parsePushEvent(payload: GitLabPushEvent): ParsedNotificationData {
    const branchName = payload.ref.replace('refs/heads/', '');
    const commitCount = payload.total_commits_count;
    const userName = payload.user_name;

    return {
      eventType: GitLabEventType.PUSH,
      title: `New push to ${branchName}`,
      message: `${userName} pushed ${commitCount} commit${commitCount !== 1 ? 's' : ''} to ${branchName} in ${payload.project.name}`,
      repositoryName: payload.project.name,
      repositoryUrl: payload.project.web_url,
      deepLinkData: {
        event_type: 'push',
        project_id: payload.project_id,
        commit_sha: payload.checkout_sha,
      },
    };
  }

  private parseMergeRequestEvent(payload: GitLabMergeRequestEvent): ParsedNotificationData {
    const { object_attributes: mr } = payload;
    const action = mr.action;
    const userName = payload.user.name;

    let title: string;
    let message: string;

    switch (action) {
      case 'open':
        title = 'New merge request';
        message = `${userName} opened merge request "${mr.title}" from ${mr.source_branch} to ${mr.target_branch}`;
        break;
      case 'close':
        title = 'Merge request closed';
        message = `${userName} closed merge request "${mr.title}"`;
        break;
      case 'merge':
        title = 'Merge request merged';
        message = `${userName} merged "${mr.title}" into ${mr.target_branch}`;
        break;
      case 'update':
        title = 'Merge request updated';
        message = `${userName} updated merge request "${mr.title}"`;
        break;
      default:
        title = 'Merge request activity';
        message = `${userName} ${action} merge request "${mr.title}"`;
    }

    return {
      eventType: GitLabEventType.MERGE_REQUEST,
      title,
      message,
      repositoryName: payload.project.name,
      repositoryUrl: payload.project.web_url,
      deepLinkData: {
        event_type: 'merge_request',
        project_id: payload.project.id,
        merge_request_iid: mr.iid,
      },
    };
  }

  private parseIssueEvent(payload: GitLabIssueEvent): ParsedNotificationData {
    const { object_attributes: issue } = payload;
    const action = issue.action;
    const userName = payload.user.name;

    let title: string;
    let message: string;

    switch (action) {
      case 'open':
        title = 'New issue created';
        message = `${userName} created issue "${issue.title}"`;
        break;
      case 'close':
        title = 'Issue closed';
        message = `${userName} closed issue "${issue.title}"`;
        break;
      case 'reopen':
        title = 'Issue reopened';
        message = `${userName} reopened issue "${issue.title}"`;
        break;
      case 'update':
        title = 'Issue updated';
        message = `${userName} updated issue "${issue.title}"`;
        break;
      default:
        title = 'Issue activity';
        message = `${userName} ${action} issue "${issue.title}"`;
    }

    return {
      eventType: GitLabEventType.ISSUE,
      title,
      message,
      repositoryName: payload.project.name,
      repositoryUrl: payload.project.web_url,
      deepLinkData: {
        event_type: 'issue',
        project_id: payload.project.id,
        issue_iid: issue.iid,
      },
    };
  }

  private parsePipelineEvent(payload: GitLabPipelineEvent): ParsedNotificationData {
    const { object_attributes: pipeline } = payload;
    const status = pipeline.status;
    const ref = pipeline.ref;

    let title: string;
    let message: string;

    switch (status) {
      case 'success':
        title = 'Pipeline succeeded';
        message = `Pipeline for ${ref} completed successfully`;
        break;
      case 'failed':
        title = 'Pipeline failed';
        message = `Pipeline for ${ref} failed`;
        break;
      case 'canceled':
        title = 'Pipeline canceled';
        message = `Pipeline for ${ref} was canceled`;
        break;
      case 'running':
        title = 'Pipeline started';
        message = `Pipeline for ${ref} is now running`;
        break;
      default:
        title = 'Pipeline update';
        message = `Pipeline for ${ref} is ${status}`;
    }

    return {
      eventType: GitLabEventType.PIPELINE,
      title,
      message,
      repositoryName: payload.project.name,
      repositoryUrl: payload.project.web_url,
      deepLinkData: {
        event_type: 'pipeline',
        project_id: payload.project.id,
        pipeline_id: pipeline.id,
      },
    };
  }

  private parseTagPushEvent(payload: GitLabTagPushEvent): ParsedNotificationData {
    const tagName = payload.ref.replace('refs/tags/', '');
    const userName = payload.user_name;

    return {
      eventType: GitLabEventType.TAG_PUSH,
      title: 'New tag created',
      message: `${userName} created tag ${tagName} in ${payload.project.name}`,
      repositoryName: payload.project.name,
      repositoryUrl: payload.project.web_url,
      deepLinkData: {
        event_type: 'tag_push',
        project_id: payload.project_id,
      },
    };
  }
}
