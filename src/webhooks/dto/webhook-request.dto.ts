import { GitLabWebhookEvent } from './gitlab-webhook.dto';

export class WebhookRequestDto {
  payload: GitLabWebhookEvent;
  fcmToken: string; // From X-FCM-Token header
  gitlabEvent?: string; // From X-GitLab-Event header
}
