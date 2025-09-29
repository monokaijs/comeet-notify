import { IsString, IsNotEmpty, IsOptional } from 'class-validator';
import { GitLabWebhookEvent } from './gitlab-webhook.dto';

export class WebhookRequestDto {
  @IsString()
  @IsNotEmpty()
  fcmToken: string;

  @IsOptional()
  @IsString()
  userId?: string;

  payload: GitLabWebhookEvent;
}
