import { ExceptionFilter, Catch, ArgumentsHost, HttpException, Logger } from '@nestjs/common';
import { Request, Response } from 'express';

@Catch(HttpException)
export class WebhookExceptionFilter implements ExceptionFilter {
  private readonly logger = new Logger(WebhookExceptionFilter.name);

  catch(exception: HttpException, host: ArgumentsHost) {
    const ctx = host.switchToHttp();
    const response = ctx.getResponse<Response>();
    const request = ctx.getRequest<Request>();
    const status = exception.getStatus();

    const errorResponse = {
      statusCode: status,
      timestamp: new Date().toISOString(),
      path: request.url,
      method: request.method,
      message: exception.message,
    };

    this.logger.error(
      `Webhook error: ${request.method} ${request.url} - ${status} - ${exception.message}`,
      exception.stack,
    );

    response.status(status).json(errorResponse);
  }
}
