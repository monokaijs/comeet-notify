import { Injectable } from '@nestjs/common';
import {
  PipelineLiveActivityStage,
  PipelineLiveActivityStageStatus,
  PipelineLiveActivityStart,
  PipelineLiveActivityUpdate,
} from '../fcm/dto/fcm-notification.dto';
import { FcmService } from '../fcm/fcm.service';
import {
  GitLabPipelineBuild,
  GitLabPipelineEvent,
} from '../webhooks/dto/gitlab-webhook.dto';

const TERMINAL_STATUSES = new Set(['success', 'failed', 'canceled', 'skipped']);
const WAITING_STATUSES = new Set([
  'created',
  'waiting_for_resource',
  'preparing',
  'pending',
  'scheduled',
]);
const COMPLETED_STAGE_STATUSES = new Set<PipelineLiveActivityStageStatus>([
  'success',
  'skipped',
]);
const MAX_VISIBLE_STAGES = 6;

function truncate(value: string, maxLength: number): string {
  return value.length > maxLength ? `${value.slice(0, maxLength - 1)}…` : value;
}

function normalizeBuildStatus(status: string): PipelineLiveActivityStageStatus {
  if (WAITING_STATUSES.has(status)) return 'pending';
  if (
    status === 'success' ||
    status === 'failed' ||
    status === 'running' ||
    status === 'canceled' ||
    status === 'skipped' ||
    status === 'manual'
  ) {
    return status;
  }
  return 'pending';
}

function aggregateStageStatus(
  builds: GitLabPipelineBuild[],
): PipelineLiveActivityStageStatus {
  const statuses = builds.map(({ status }) => normalizeBuildStatus(status));
  if (statuses.includes('failed')) return 'failed';
  if (statuses.includes('running')) return 'running';
  if (statuses.includes('pending')) return 'pending';
  if (statuses.includes('manual')) return 'manual';
  if (statuses.includes('canceled')) return 'canceled';
  if (statuses.length > 0 && statuses.every((status) => status === 'skipped'))
    return 'skipped';
  if (statuses.every((status) => status === 'success' || status === 'skipped'))
    return 'success';
  return 'pending';
}

function buildStages(
  payload: GitLabPipelineEvent,
): PipelineLiveActivityStage[] {
  const grouped = new Map<string, GitLabPipelineBuild[]>();
  const orderedStageNames: string[] = [];

  for (const stageName of payload.object_attributes.stages ?? []) {
    if (!grouped.has(stageName)) {
      grouped.set(stageName, []);
      orderedStageNames.push(stageName);
    }
  }

  for (const build of [...(payload.builds ?? [])].sort((a, b) => a.id - b.id)) {
    const stageName = build.stage || 'Build';
    if (!grouped.has(stageName)) {
      grouped.set(stageName, []);
      orderedStageNames.push(stageName);
    }
    grouped.get(stageName)!.push(build);
  }

  return orderedStageNames
    .filter((stageName) => grouped.get(stageName)!.length > 0)
    .map((stageName) => ({
      name: truncate(stageName, 32),
      status: aggregateStageStatus(grouped.get(stageName)!),
    }));
}

function selectVisibleStages(
  stages: PipelineLiveActivityStage[],
  focusIndex: number,
): PipelineLiveActivityStage[] {
  if (stages.length <= MAX_VISIBLE_STAGES) return stages;
  const maxStart = stages.length - MAX_VISIBLE_STAGES;
  const start = Math.max(0, Math.min(maxStart, focusIndex - 2));
  return stages.slice(start, start + MAX_VISIBLE_STAGES);
}

@Injectable()
export class PipelineLiveActivityService {
  constructor(private readonly fcmService: FcmService) {}

  buildUpdate(
    payload: GitLabPipelineEvent,
    now = new Date(),
  ): PipelineLiveActivityUpdate {
    const stages = buildStages(payload);
    const failedStageIndex = stages.findIndex(
      ({ status }) => status === 'failed',
    );
    const activeStageIndex = stages.findIndex(({ status }) =>
      ['running', 'pending', 'manual', 'canceled'].includes(status),
    );
    const focusIndex =
      failedStageIndex >= 0
        ? failedStageIndex
        : activeStageIndex >= 0
          ? activeStageIndex
          : Math.max(0, stages.length - 1);
    const focusedStage = stages[focusIndex];
    const failedBuild = [...(payload.builds ?? [])]
      .sort((a, b) => a.id - b.id)
      .find(
        ({ status, stage }) =>
          status === 'failed' &&
          truncate(stage || 'Build', 32) === focusedStage?.name,
      );
    const status = payload.object_attributes.status;
    const timestamp = Math.floor(now.getTime() / 1000);
    const isTerminal = TERMINAL_STATUSES.has(status);

    return {
      event: isTerminal ? 'end' : 'update',
      timestamp,
      staleDate: isTerminal ? undefined : timestamp + 90,
      dismissalDate: isTerminal
        ? timestamp + (status === 'failed' ? 60 * 60 : 15 * 60)
        : undefined,
      contentState: {
        status,
        stages: selectVisibleStages(stages, focusIndex),
        currentStageName: focusedStage?.name,
        failedStageName:
          failedStageIndex >= 0 ? stages[failedStageIndex].name : undefined,
        failedJobName: failedBuild
          ? truncate(failedBuild.name, 100)
          : undefined,
        completedStageCount: stages.filter(({ status: stageStatus }) =>
          COMPLETED_STAGE_STATUSES.has(stageStatus),
        ).length,
        totalStageCount: stages.length,
        updatedAt: timestamp,
      },
    };
  }

  buildStart(
    payload: GitLabPipelineEvent,
    now = new Date(),
  ): PipelineLiveActivityStart {
    const update = this.buildUpdate(payload, now);
    const pipelineId = payload.object_attributes.id;
    const projectId = payload.project.id;
    return {
      event: 'start',
      timestamp: update.timestamp,
      staleDate: update.staleDate ?? update.timestamp + 90,
      contentState: update.contentState,
      attributesType: 'PipelineActivityAttributes',
      attributes: {
        projectId,
        pipelineId,
        pipelineName: truncate(
          payload.project.name || `Pipeline #${pipelineId}`,
          80,
        ),
        ref: truncate(payload.object_attributes.ref || 'detached', 80),
        deepLink: `comeet:///PipelineDetails?projectId=${projectId}&pipelineId=${pipelineId}`,
      },
      alert: {
        title: `${payload.project.name} build started`,
        body: `${payload.object_attributes.ref} · Pipeline #${pipelineId}`,
        sound: 'default',
      },
    };
  }

  async sendUpdate(
    payload: GitLabPipelineEvent,
    fcmToken: string,
    liveActivityToken: string,
  ) {
    return this.fcmService.sendLiveActivity(
      fcmToken,
      liveActivityToken,
      this.buildUpdate(payload),
    );
  }

  async sendStart(
    payload: GitLabPipelineEvent,
    fcmToken: string,
    pushToStartToken: string,
  ) {
    const collapseId = `comeet-${payload.project.id}-${payload.object_attributes.id}`;
    return this.fcmService.sendLiveActivity(
      fcmToken,
      pushToStartToken,
      this.buildStart(payload),
      collapseId,
    );
  }
}
