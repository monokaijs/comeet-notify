import { Controller, Post, Body, Headers, BadRequestException, Logger, Query } from '@nestjs/common';
import { WebhooksService } from './webhooks.service';
import { GitLabWebhookEvent } from './dto/gitlab-webhook.dto';

@Controller('webhooks')
export class WebhooksController {
  private readonly logger = new Logger(WebhooksController.name);

  constructor(private readonly webhooksService: WebhooksService) {}

  @Post('gitlab')
  async handleGitLabWebhook(
    @Body() payload: GitLabWebhookEvent,
    @Headers('x-gitlab-token') gitlabToken: string,
    @Headers('x-gitlab-event') gitlabEvent: string,
    @Query('fcmToken') fcmToken: string,
  ) {
    this.logger.log(`Received GitLab webhook: ${gitlabEvent}`);

    if (!fcmToken) {
      throw new BadRequestException('Missing FCM token in query parameters');
    }

    if (!gitlabEvent) {
      throw new BadRequestException('Missing GitLab event header');
    }

    try {
      await this.webhooksService.processWebhook(payload, fcmToken);
      return { success: true, message: 'Notification sent successfully' };
    } catch (error) {
      this.logger.error(`Error processing webhook: ${error.message}`, error.stack);
      throw new BadRequestException('Failed to process webhook');
    }
  }
}
