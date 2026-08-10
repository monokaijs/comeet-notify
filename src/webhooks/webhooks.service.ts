import { Injectable, Logger } from '@nestjs/common';
import { GitLabWebhookEvent } from './dto/gitlab-webhook.dto';
import { GitLabEventParserService } from './gitlab-event-parser.service';
import { FcmService } from '../fcm/fcm.service';
import { PipelineLiveActivityService } from '../live-activity/pipeline-live-activity.service';

interface LiveActivityHeaders {
  token?: string;
  pipelineId?: string;
  pushToStartToken?: string;
}

const LIVE_ACTIVITY_TOKEN_PATTERN = /^[a-fA-F0-9]{32,512}$/;

@Injectable()
export class WebhooksService {
  private readonly logger = new Logger(WebhooksService.name);

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

      const notificationPromise = this.fcmService.sendNotification(fcmToken, {
        title: notificationData.title,
        body: notificationData.message,
        data: fcmData,
      });
      let liveActivityPromise = Promise.resolve<Awaited<
        ReturnType<PipelineLiveActivityService['sendUpdate']>
      > | null>(null);
      if (this.shouldUpdateLiveActivity(payload, liveActivity)) {
        liveActivityPromise = this.pipelineLiveActivityService.sendUpdate(
          payload,
          fcmToken,
          liveActivity.token!,
        );
      } else if (this.shouldStartLiveActivity(payload, liveActivity)) {
        liveActivityPromise = this.pipelineLiveActivityService.sendStart(
          payload,
          fcmToken,
          liveActivity.pushToStartToken!,
        );
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
        this.logger.error(`Failed to send notification: ${result.error}`);
        throw new Error(result.error || 'Unknown FCM error');
      }
    } catch (error) {
      this.logger.error(
        `Failed to send notification: ${error.message}`,
        error.stack,
      );
      throw error;
    }
  }

  private shouldUpdateLiveActivity(
    payload: GitLabWebhookEvent,
    headers: LiveActivityHeaders,
  ): payload is Extract<GitLabWebhookEvent, { object_kind: 'pipeline' }> {
    if (
      payload.object_kind !== 'pipeline' ||
      !headers.token ||
      !headers.pipelineId
    )
      return false;
    if (!LIVE_ACTIVITY_TOKEN_PATTERN.test(headers.token)) {
      this.logger.warn('Ignoring an invalid Live Activity token');
      return false;
    }
    return String(payload.object_attributes.id) === headers.pipelineId;
  }

  private shouldStartLiveActivity(
    payload: GitLabWebhookEvent,
    headers: LiveActivityHeaders,
  ): payload is Extract<GitLabWebhookEvent, { object_kind: 'pipeline' }> {
    if (payload.object_kind !== 'pipeline' || !headers.pushToStartToken)
      return false;
    if (!LIVE_ACTIVITY_TOKEN_PATTERN.test(headers.pushToStartToken)) {
      this.logger.warn('Ignoring an invalid Live Activity push-to-start token');
      return false;
    }
    return payload.object_attributes.status === 'running';
  }
}
