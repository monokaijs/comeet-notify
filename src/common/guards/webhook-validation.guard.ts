import { Injectable, CanActivate, ExecutionContext, UnauthorizedException, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import * as crypto from 'crypto';

@Injectable()
export class WebhookValidationGuard implements CanActivate {
  private readonly logger = new Logger(WebhookValidationGuard.name);

  constructor(private configService: ConfigService) {}

  canActivate(context: ExecutionContext): boolean {
    const request = context.switchToHttp().getRequest();
    const gitlabToken = request.headers['x-gitlab-token'];
    const gitlabEvent = request.headers['x-gitlab-event'];

    if (!gitlabToken) {
      this.logger.warn('Missing GitLab token header');
      throw new UnauthorizedException('Missing GitLab token header');
    }

    if (!gitlabEvent) {
      this.logger.warn('Missing GitLab event header');
      throw new UnauthorizedException('Missing GitLab event header');
    }

    const isValidRequest = this.validateGitLabRequest(request);
    if (!isValidRequest) {
      this.logger.warn('Invalid GitLab webhook request');
      throw new UnauthorizedException('Invalid webhook request');
    }

    return true;
  }

  private validateGitLabRequest(request: any): boolean {
    const signature = request.headers['x-gitlab-token'];
    const payload = JSON.stringify(request.body);
    const secret = this.configService.get<string>('gitlab.webhookSecret');

    if (!secret) {
      this.logger.warn('GitLab webhook secret not configured');
      return false;
    }

    if (!signature) {
      return false;
    }

    const expectedSignature = crypto
      .createHmac('sha256', secret)
      .update(payload, 'utf8')
      .digest('hex');

    return crypto.timingSafeEqual(
      Buffer.from(signature, 'hex'),
      Buffer.from(expectedSignature, 'hex')
    );
  }
}
