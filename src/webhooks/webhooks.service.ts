import { Injectable, Logger } from '@nestjs/common';
import { GitLabWebhookEvent } from './dto/gitlab-webhook.dto';
import { GitLabEventParserService } from './gitlab-event-parser.service';
import { FcmService } from '../fcm/fcm.service';

@Injectable()
export class WebhooksService {
  private readonly logger = new Logger(WebhooksService.name);

  constructor(
    private gitlabEventParser: GitLabEventParserService,
    private fcmService: FcmService,
  ) {}

  async processWebhook(payload: GitLabWebhookEvent, fcmToken: string): Promise<void> {
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
        fcmData.merge_request_iid = String(notificationData.deepLinkData.merge_request_iid);
      }
      if (notificationData.deepLinkData.pipeline_id !== undefined) {
        fcmData.pipeline_id = String(notificationData.deepLinkData.pipeline_id);
      }

      const result = await this.fcmService.sendNotification(fcmToken, {
        title: notificationData.title,
        body: notificationData.message,
        data: fcmData,
      });

      if (result.success) {
        this.logger.log(`Notification sent successfully for ${payload.object_kind} event`);
      } else {
        this.logger.error(`Failed to send notification: ${result.error}`);
        throw new Error(result.error || 'Unknown FCM error');
      }
    } catch (error) {
      this.logger.error(`Failed to send notification: ${error.message}`, error.stack);
      throw error;
    }
  }
}
