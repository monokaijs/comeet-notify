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
  completedJobCount: number;
  totalJobCount: number;
}

export interface PipelineLiveActivityContentState {
  status: string;
  stages: PipelineLiveActivityStage[];
  currentStageName?: string;
  failedStageName?: string;
  failedJobName?: string;
  completedStageCount: number;
  totalStageCount: number;
  completedJobCount: number;
  totalJobCount: number;
  updatedAt: number;
}

export interface PipelineLiveActivityUpdate {
  event: 'update' | 'end';
  timestamp: number;
  staleDate?: number;
  dismissalDate?: number;
  contentState: PipelineLiveActivityContentState;
}

export interface PipelineLiveActivityStart {
  event: 'start';
  timestamp: number;
  staleDate: number;
  inputPushToken: 1;
  contentState: PipelineLiveActivityContentState;
  attributesType: 'PipelineActivityAttributes';
  attributes: {
    instanceId?: string;
    projectId: number;
    pipelineId: number;
    pipelineName: string;
    ref: string;
    deepLink: string;
  };
  alert: {
    title: string;
    body: string;
    sound: 'default';
  };
}

export type PipelineLiveActivityMessage =
  | PipelineLiveActivityStart
  | PipelineLiveActivityUpdate;
