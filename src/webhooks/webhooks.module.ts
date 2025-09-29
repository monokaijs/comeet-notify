import { Module } from '@nestjs/common';
import { WebhooksController } from './webhooks.controller';
import { WebhooksService } from './webhooks.service';
import { GitLabEventParserService } from './gitlab-event-parser.service';
import { FcmModule } from '../fcm/fcm.module';

@Module({
  imports: [FcmModule],
  controllers: [WebhooksController],
  providers: [WebhooksService, GitLabEventParserService],
  exports: [WebhooksService],
})
export class WebhooksModule {}
