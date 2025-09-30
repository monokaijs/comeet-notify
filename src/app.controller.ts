import { Controller, Get, Post, HttpCode, Logger, Req } from '@nestjs/common';
import { ApiTags, ApiOperation, ApiResponse } from '@nestjs/swagger';
import { Request } from 'express';
import { AppService } from './app.service';

@ApiTags('app')
@Controller()
export class AppController {
  private readonly logger = new Logger(AppController.name);

  constructor(private readonly appService: AppService) {}

  @Get()
  @ApiOperation({ summary: 'Get application status' })
  @ApiResponse({ status: 200, description: 'Returns a hello message', type: String })
  getHello(): string {
    return this.appService.getHello();
  }

  @Post()
  @HttpCode(200)
  @ApiOperation({ summary: 'Handle POST requests to root' })
  @ApiResponse({ status: 200, description: 'Returns a hello message for POST requests', type: String })
  postHello(): string {
    console.log('POST request received');
    return this.appService.getHello();
  }
}
