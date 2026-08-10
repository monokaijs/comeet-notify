export interface FcmNotificationData {
  title: string;
  body: string;
  data?: Record<string, string>;
}

export interface FcmResponse {
  success: boolean;
  messageId?: string;
  error?: string;
}

export type PipelineLiveActivityStageStatus =
  | 'success'
  | 'failed'
  | 'running'
  | 'pending'
  | 'canceled'
  | 'skipped'
  | 'manual';

export interface PipelineLiveActivityStage {
  name: string;
  status: PipelineLiveActivityStageStatus;
}

export interface PipelineLiveActivityContentState {
  status: string;
  stages: PipelineLiveActivityStage[];
  currentStageName?: string;
  failedStageName?: string;
  failedJobName?: string;
  completedStageCount: number;
  totalStageCount: number;
  updatedAt: number;
}

export interface PipelineLiveActivityUpdate {
  event: 'update' | 'end';
  timestamp: number;
  staleDate?: number;
  dismissalDate?: number;
  contentState: PipelineLiveActivityContentState;
}
