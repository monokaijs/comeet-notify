import { Injectable, Logger } from '@nestjs/common';
import { GitLabWebhookEvent } from './dto/gitlab-webhook.dto';
import { GitLabEventParserService } from './gitlab-event-parser.service';
import { FcmService } from '../fcm/fcm.service';
import { PipelineLiveActivityService } from '../live-activity/pipeline-live-activity.service';

interface LiveActivityHeaders {
  token?: string;
  pipelineId?: string;
  registrations?: string;
  pushToStartToken?: string;
  instanceId?: string;
}

const LIVE_ACTIVITY_TOKEN_PATTERN = /^[a-fA-F0-9]{32,512}$/;
const MAX_LIVE_ACTIVITY_REGISTRATIONS = 8;
const MAX_LIVE_ACTIVITY_AGE_SECONDS = 8 * 60 * 60;
const REMOTE_START_DEDUPLICATION_MS = 8 * 60 * 60 * 1000;
const TERMINAL_PIPELINE_STATUSES = new Set([
  'success',
  'failed',
  'canceled',
  'skipped',
]);

@Injectable()
export class WebhooksService {
  private readonly logger = new Logger(WebhooksService.name);
  private readonly remotelyStartedPipelines = new Map<string, number>();

  constructor(
    private gitlabEventParser: GitLabEventParserService,
    private fcmService: FcmService,
    private pipelineLiveActivityService: PipelineLiveActivityService,
  ) {}

  async processWebhook(
    payload: GitLabWebhookEvent,
    fcmToken: string,
    liveActivity: LiveActivityHeaders = {},
  ): Promise<void> {
    const notificationData = this.gitlabEventParser.parseEvent(payload);
    if (!notificationData) {
      this.logger.warn(`Unsupported event type: ${payload.object_kind}`);
      return;
    }

    try {
      const fcmData: Record<string, string> = {
        eventType: notificationData.eventType,
        repositoryName: notificationData.repositoryName || '',
        repositoryUrl: notificationData.repositoryUrl || '',
        event_type: notificationData.deepLinkData.event_type,
      };

      if (notificationData.deepLinkData.project_id !== undefined) {
        fcmData.project_id = String(notificationData.deepLinkData.project_id);
      }
      if (notificationData.deepLinkData.commit_sha) {
        fcmData.commit_sha = notificationData.deepLinkData.commit_sha;
      }
      if (notificationData.deepLinkData.issue_iid !== undefined) {
        fcmData.issue_iid = String(notificationData.deepLinkData.issue_iid);
      }
      if (notificationData.deepLinkData.merge_request_iid !== undefined) {
        fcmData.merge_request_iid = String(
          notificationData.deepLinkData.merge_request_iid,
        );
      }
      if (notificationData.deepLinkData.pipeline_id !== undefined) {
        fcmData.pipeline_id = String(notificationData.deepLinkData.pipeline_id);
      }
      if (liveActivity.instanceId) {
        fcmData.instance_id = liveActivity.instanceId;
      }

      const notificationPromise = this.fcmService.sendNotification(fcmToken, {
        title: notificationData.title,
        body: notificationData.message,
        data: fcmData,
      });
      let liveActivityPromise = Promise.resolve<Awaited<
        ReturnType<PipelineLiveActivityService['sendUpdate']>
      > | null>(null);
      const liveActivityToken = this.findLiveActivityToken(
        payload,
        liveActivity,
      );
      const startKey = this.liveActivityStartKey(payload, fcmToken);
      if (payload.object_kind === 'pipeline' && liveActivityToken) {
        liveActivityPromise = this.pipelineLiveActivityService.sendUpdate(
          payload,
          fcmToken,
          liveActivityToken,
        );
        if (
          payload.object_kind === 'pipeline' &&
          TERMINAL_PIPELINE_STATUSES.has(payload.object_attributes.status)
        ) {
          this.remotelyStartedPipelines.delete(startKey);
        }
      } else if (
        payload.object_kind === 'pipeline' &&
        this.shouldStartLiveActivity(payload, liveActivity, fcmToken)
      ) {
        this.remotelyStartedPipelines.set(startKey, Date.now());
        liveActivityPromise = this.pipelineLiveActivityService
          .sendStart(
            payload,
            fcmToken,
            liveActivity.pushToStartToken!,
            liveActivity.instanceId,
          )
          .then((result) => {
            if (!result.success) this.remotelyStartedPipelines.delete(startKey);
            return result;
          });
      } else if (
        payload.object_kind === 'pipeline' &&
        TERMINAL_PIPELINE_STATUSES.has(payload.object_attributes.status)
      ) {
        this.remotelyStartedPipelines.delete(startKey);
      }
      const [result, liveActivityResult] = await Promise.all([
        notificationPromise,
        liveActivityPromise,
      ]);

      if (liveActivityResult && !liveActivityResult.success) {
        this.logger.warn(
          `Live Activity update failed: ${liveActivityResult.error}`,
        );
      }

      if (result.success) {
        this.logger.log(
          `Notification sent successfully for ${payload.object_kind} event`,
        );
      } else {
        this.logger.warn(
          `Notification delivery failed without rejecting the GitLab webhook: ${result.error}`,
        );
      }
    } catch (error) {
      this.logger.error(
        `Failed to send notification: ${error.message}`,
        error.stack,
      );
      throw error;
    }
  }

  private findLiveActivityToken(
    payload: GitLabWebhookEvent,
    headers: LiveActivityHeaders,
  ): string | undefined {
    if (payload.object_kind !== 'pipeline') return undefined;

    if (headers.registrations && headers.registrations.length <= 8192) {
      try {
        const registrations = JSON.parse(headers.registrations) as unknown;
        if (Array.isArray(registrations)) {
          const match = registrations
            .slice(-MAX_LIVE_ACTIVITY_REGISTRATIONS)
            .find(
              (item) =>
                !!item &&
                typeof item === 'object' &&
                (item as { pipelineId?: unknown }).pipelineId ===
                  payload.object_attributes.id,
            ) as
            | {
                pipelineId: number;
                pushToken?: unknown;
                registeredAt?: unknown;
              }
            | undefined;
          if (match) {
            if (
              typeof match.registeredAt === 'number' &&
              Number.isFinite(match.registeredAt) &&
              Math.floor(Date.now() / 1000) - match.registeredAt >=
                MAX_LIVE_ACTIVITY_AGE_SECONDS
            ) {
              this.logger.log(
                `Ignoring an expired Live Activity registration for pipeline ${payload.object_attributes.id}`,
              );
              return undefined;
            }
            if (
              typeof match.pushToken === 'string' &&
              LIVE_ACTIVITY_TOKEN_PATTERN.test(match.pushToken)
            ) {
              return match.pushToken;
            }
          }
        }
      } catch {
        this.logger.warn('Ignoring invalid Live Activity registrations');
      }
    }

    if (
      headers.token &&
      headers.pipelineId === String(payload.object_attributes.id) &&
      LIVE_ACTIVITY_TOKEN_PATTERN.test(headers.token)
    ) {
      return headers.token;
    }
    return undefined;
  }

  private shouldStartLiveActivity(
    payload: Extract<GitLabWebhookEvent, { object_kind: 'pipeline' }>,
    headers: LiveActivityHeaders,
    fcmToken: string,
  ): boolean {
    if (!headers.pushToStartToken) return false;
    if (!LIVE_ACTIVITY_TOKEN_PATTERN.test(headers.pushToStartToken)) {
      this.logger.warn('Ignoring an invalid Live Activity push-to-start token');
      return false;
    }
    if (payload.object_attributes.status !== 'running') return false;

    const key = this.liveActivityStartKey(payload, fcmToken);
    const now = Date.now();
    for (const [storedKey, startedAt] of this.remotelyStartedPipelines) {
      if (now - startedAt >= REMOTE_START_DEDUPLICATION_MS) {
        this.remotelyStartedPipelines.delete(storedKey);
      }
    }
    return !this.remotelyStartedPipelines.has(key);
  }

  private liveActivityStartKey(
    payload:
      | Extract<GitLabWebhookEvent, { object_kind: 'pipeline' }>
      | GitLabWebhookEvent,
    fcmToken: string,
  ): string {
    if (payload.object_kind !== 'pipeline') return fcmToken;
    return `${fcmToken}:${payload.project.id}:${payload.object_attributes.id}`;
  }
}
