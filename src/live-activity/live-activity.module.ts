import { Module } from '@nestjs/common';
import { FcmModule } from '../fcm/fcm.module';
import { PipelineLiveActivityService } from './pipeline-live-activity.service';

@Module({
  imports: [FcmModule],
  providers: [PipelineLiveActivityService],
  exports: [PipelineLiveActivityService],
})
export class LiveActivityModule {}
