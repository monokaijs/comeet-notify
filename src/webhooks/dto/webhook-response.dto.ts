import { ApiProperty } from '@nestjs/swagger';

export class WebhookSuccessResponseDto {
  @ApiProperty({ example: true, description: 'Indicates if the webhook was processed successfully' })
  success: boolean;

  @ApiProperty({ example: 'Notification sent successfully', description: 'Success message' })
  message: string;
}

export class WebhookErrorResponseDto {
  @ApiProperty({ example: 400, description: 'HTTP status code' })
  statusCode: number;

  @ApiProperty({ example: 'Missing FCM token in X-FCM-Token header', description: 'Error message' })
  message: string;

  @ApiProperty({ example: 'Bad Request', description: 'Error type' })
  error: string;
}
