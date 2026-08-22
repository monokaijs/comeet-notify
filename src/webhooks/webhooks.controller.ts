import {
  Controller,
  Post,
  Body,
  Headers,
  BadRequestException,
  Logger,
  Query,
} from '@nestjs/common';
import {
  ApiTags,
  ApiOperation,
  ApiResponse,
  ApiBody,
  ApiHeader,
} from '@nestjs/swagger';
import { WebhooksService } from './webhooks.service';
import { GitLabWebhookEvent } from './dto/gitlab-webhook.dto';
import { GitLabWebhookDto } from './dto/gitlab-webhook-swagger.dto';
import {
  WebhookSuccessResponseDto,
  WebhookErrorResponseDto,
} from './dto/webhook-response.dto';

@ApiTags('webhooks')
@Controller('webhooks')
export class WebhooksController {
  private readonly logger = new Logger(WebhooksController.name);

  constructor(private readonly webhooksService: WebhooksService) {}

  @Post('gitlab')
  @ApiOperation({
    summary: 'Handle GitLab webhook events',
    description:
      'Processes GitLab webhook events and sends push notifications to the specified FCM token',
  })
  @ApiHeader({
    name: 'x-gitlab-event',
    description: 'GitLab event type (e.g., Push Hook, Merge Request Hook)',
    required: false,
  })
  @ApiHeader({
    name: 'x-live-activity-token',
    description: 'Optional ActivityKit push token for remote pipeline updates',
    required: false,
  })
  @ApiHeader({
    name: 'x-live-activity-pipeline-id',
    description: 'Pipeline ID associated with the ActivityKit push token',
    required: false,
  })
  @ApiHeader({
    name: 'x-live-activity-registrations',
    description: 'JSON list of pipeline IDs and ActivityKit update tokens',
    required: false,
  })
  @ApiHeader({
    name: 'x-live-activity-push-to-start-token',
    description:
      'Optional ActivityKit token for starting pipeline Live Activities',
    required: false,
  })
  @ApiHeader({
    name: 'x-pipeline-delivery-mode',
    description:
      'Pipeline delivery channel: live_activity, notification, or both',
    required: false,
  })
  @ApiHeader({
    name: 'x-fcm-token',
    description: 'Firebase Cloud Messaging token for push notifications',
    required: true,
  })
  @ApiHeader({
    name: 'x-comeet-instance-id',
    description: 'Comeet GitLab instance identifier used for deep links',
    required: false,
  })
  @ApiBody({
    description: 'GitLab webhook payload',
    type: GitLabWebhookDto,
    examples: {
      pushEvent: {
        summary: 'Push Event',
        value: {
          object_kind: 'push',
          event_name: 'push',
          before: '95790bf891e76fee5e1747ab589903a6a1f80f22',
          after: 'da1560886d4f094c3e6c9ef40349f7d38b5d27d7',
          ref: 'refs/heads/master',
          user_name: 'John Doe',
          project: {
            id: 15,
            name: 'example-project',
            web_url: 'https://gitlab.example.com/group/project',
          },
        },
      },
      mergeRequestEvent: {
        summary: 'Merge Request Event',
        value: {
          object_kind: 'merge_request',
          user: {
            name: 'John Doe',
            username: 'johndoe',
          },
          project: {
            id: 15,
            name: 'example-project',
            web_url: 'https://gitlab.example.com/group/project',
          },
          object_attributes: {
            id: 99,
            title: 'MS-Viewport',
            state: 'opened',
            action: 'open',
          },
        },
      },
    },
  })
  @ApiResponse({
    status: 200,
    description: 'Webhook processed successfully',
    type: WebhookSuccessResponseDto,
  })
  @ApiResponse({
    status: 400,
    description: 'Bad request - Missing FCM token or invalid payload',
    type: WebhookErrorResponseDto,
  })
  async handleGitLabWebhook(
    @Body() payload: GitLabWebhookEvent,
    @Headers('x-gitlab-event') gitlabEvent: string,
    @Headers('x-fcm-token') fcmToken: string,
    @Headers('x-live-activity-token') liveActivityToken?: string,
    @Headers('x-live-activity-pipeline-id') liveActivityPipelineId?: string,
    @Headers('x-live-activity-registrations')
    liveActivityRegistrations?: string,
    @Headers('x-live-activity-push-to-start-token')
    liveActivityPushToStartToken?: string,
    @Headers('x-comeet-instance-id') instanceId?: string,
    @Headers('x-pipeline-delivery-mode') pipelineDeliveryMode?: string,
  ) {
    this.logger.log(
      `Received GitLab webhook: ${gitlabEvent || payload.object_kind}`,
    );

    if (!fcmToken) {
      throw new BadRequestException('Missing FCM token in X-FCM-Token header');
    }

    try {
      await this.webhooksService.processWebhook(payload, fcmToken, {
        token: liveActivityToken,
        pipelineId: liveActivityPipelineId,
        registrations: liveActivityRegistrations,
        pushToStartToken: liveActivityPushToStartToken,
        instanceId,
        pipelineDeliveryMode,
      });
      return { success: true, message: 'Webhook processed successfully' };
    } catch (error) {
      this.logger.error(
        `Error processing webhook: ${error.message}`,
        error.stack,
      );
      throw new BadRequestException('Failed to process webhook');
    }
  }
}
