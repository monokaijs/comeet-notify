import { FcmService } from '../fcm/fcm.service';
import { GitLabPipelineEvent } from '../webhooks/dto/gitlab-webhook.dto';
import { PipelineLiveActivityService } from './pipeline-live-activity.service';

const makePipeline = (): GitLabPipelineEvent =>
  ({
    object_kind: 'pipeline',
    object_attributes: {
      id: 42,
      ref: 'main',
      status: 'failed',
      stages: ['prepare', 'build', 'test', 'deploy'],
    },
    project: { id: 7, name: 'Comeet' },
    builds: [
      { id: 1, name: 'install', stage: 'prepare', status: 'success' },
      { id: 2, name: 'compile', stage: 'build', status: 'success' },
      { id: 3, name: 'unit tests', stage: 'test', status: 'failed' },
      { id: 4, name: 'release', stage: 'deploy', status: 'created' },
    ],
  }) as GitLabPipelineEvent;

describe('PipelineLiveActivityService', () => {
  const sendLiveActivity = jest.fn();
  const service = new PipelineLiveActivityService({
    sendLiveActivity,
  } as unknown as FcmService);

  beforeEach(() => sendLiveActivity.mockReset());

  it('builds a terminal failure state with the failed stage and job', () => {
    const update = service.buildUpdate(
      makePipeline(),
      new Date('2026-08-10T00:00:00Z'),
    );

    expect(update).toEqual({
      event: 'end',
      timestamp: 1786320000,
      dismissalDate: 1786323600,
      contentState: {
        status: 'failed',
        stages: [
          {
            name: 'prepare',
            status: 'success',
            completedJobCount: 1,
            totalJobCount: 1,
          },
          {
            name: 'build',
            status: 'success',
            completedJobCount: 1,
            totalJobCount: 1,
          },
          {
            name: 'test',
            status: 'failed',
            completedJobCount: 1,
            totalJobCount: 1,
          },
          {
            name: 'deploy',
            status: 'pending',
            completedJobCount: 0,
            totalJobCount: 1,
          },
        ],
        currentStageName: 'test',
        failedStageName: 'test',
        failedJobName: 'unit tests',
        completedStageCount: 2,
        totalStageCount: 4,
        completedJobCount: 3,
        totalJobCount: 4,
        updatedAt: 1786320000,
      },
    });
  });

  it('reports job-level progress for parallel jobs within each stage', () => {
    const payload = makePipeline();
    payload.object_attributes.status = 'running';
    payload.builds = [
      { id: 1, name: 'web', stage: 'build', status: 'success' },
      { id: 2, name: 'ios', stage: 'build', status: 'running' },
      { id: 3, name: 'android', stage: 'build', status: 'pending' },
      { id: 4, name: 'unit', stage: 'test', status: 'pending' },
      { id: 5, name: 'e2e', stage: 'test', status: 'pending' },
    ];

    const update = service.buildUpdate(payload);

    expect(update.contentState.stages).toEqual([
      {
        name: 'build',
        status: 'running',
        completedJobCount: 1,
        totalJobCount: 3,
      },
      {
        name: 'test',
        status: 'pending',
        completedJobCount: 0,
        totalJobCount: 2,
      },
    ]);
    expect(update.contentState.completedJobCount).toBe(1);
    expect(update.contentState.totalJobCount).toBe(5);
  });

  it('shows a successful terminal pipeline as fully complete even with optional manual jobs', () => {
    const payload = makePipeline();
    payload.object_attributes.status = 'success';
    payload.object_attributes.stages = ['build', 'deploy'];
    payload.builds = [
      { id: 1, name: 'compile', stage: 'build', status: 'success' },
      { id: 2, name: 'production', stage: 'deploy', status: 'manual' },
    ];

    const update = service.buildUpdate(payload);

    expect(update.event).toBe('end');
    expect(update.contentState.stages).toEqual([
      {
        name: 'build',
        status: 'success',
        completedJobCount: 1,
        totalJobCount: 1,
      },
      {
        name: 'deploy',
        status: 'skipped',
        completedJobCount: 1,
        totalJobCount: 1,
      },
    ]);
    expect(update.contentState.completedJobCount).toBe(2);
    expect(update.contentState.totalJobCount).toBe(2);
    expect(update.contentState.completedStageCount).toBe(2);
  });

  it('keeps progress focused on running work after an allowed job failure', () => {
    const payload = makePipeline();
    payload.object_attributes.status = 'running';
    payload.object_attributes.stages = ['test', 'deploy'];
    payload.builds = [
      {
        id: 1,
        name: 'optional lint',
        stage: 'test',
        status: 'failed',
        allow_failure: true,
      },
      { id: 2, name: 'release', stage: 'deploy', status: 'running' },
    ];

    const update = service.buildUpdate(payload);

    expect(update.contentState.currentStageName).toBe('deploy');
    expect(update.contentState.failedStageName).toBeUndefined();
    expect(update.contentState.failedJobName).toBeUndefined();
    expect(update.contentState.stages[0]).toMatchObject({
      name: 'test',
      status: 'success',
      completedJobCount: 1,
    });
  });

  it('marks active updates stale after 15 minutes and limits the visible stepper', () => {
    const payload = makePipeline();
    payload.object_attributes.status = 'running';
    payload.object_attributes.stages = [
      'one',
      'two',
      'three',
      'four',
      'five',
      'six',
      'seven',
    ];
    payload.builds = payload.object_attributes.stages.map((stage, index) => ({
      id: index,
      name: stage,
      stage,
      status: stage === 'six' ? 'running' : index < 5 ? 'success' : 'created',
    }));

    const update = service.buildUpdate(
      payload,
      new Date('2026-08-10T00:00:00Z'),
    );

    expect(update.event).toBe('update');
    expect(update.staleDate).toBe(1786320900);
    expect(update.dismissalDate).toBeUndefined();
    expect(update.contentState.currentStageName).toBe('six');
    expect(update.contentState.stages).toHaveLength(6);
    expect(update.contentState.stages.map(({ name }) => name)).toContain('six');
  });

  it('builds the ActivityKit remote-start attributes and initial state', () => {
    const payload = makePipeline();
    payload.object_attributes.status = 'running';

    const start = service.buildStart(payload, new Date('2026-08-10T00:00:00Z'));

    expect(start.event).toBe('start');
    expect(start.inputPushToken).toBe(1);
    expect(start.attributesType).toBe('PipelineActivityAttributes');
    expect(start.attributes).toEqual({
      projectId: 7,
      pipelineId: 42,
      pipelineName: 'Comeet',
      ref: 'main',
      deepLink:
        'comeet:///PipelineDetails?projectId=7&pipelineId=42&notificationSource=liveActivity',
    });
    expect(start.contentState.status).toBe('running');
    expect(start.alert.title).toBe('Comeet build started');
  });

  it('includes the GitLab instance in remotely started activities', () => {
    const payload = makePipeline();
    payload.object_attributes.status = 'running';

    const start = service.buildStart(
      payload,
      new Date('2026-08-10T00:00:00Z'),
      'gitlab-work',
    );

    expect(start.attributes.instanceId).toBe('gitlab-work');
    expect(start.attributes.deepLink).toBe(
      'comeet:///PipelineDetails?projectId=7&pipelineId=42&instanceId=gitlab-work&notificationSource=liveActivity',
    );
  });

  it.each(['manual', 'scheduled', 'canceling', 'waiting_for_callback'])(
    'keeps a pipeline in %s state active',
    (status) => {
      const payload = makePipeline();
      payload.object_attributes.status = status;

      expect(service.buildUpdate(payload).event).toBe('update');
    },
  );

  it.each(['success', 'failed', 'canceled', 'skipped'])(
    'ends a pipeline in completed %s state',
    (status) => {
      const payload = makePipeline();
      payload.object_attributes.status = status;

      expect(service.buildUpdate(payload).event).toBe('end');
    },
  );

  it('delivers terminal content through the pipeline collapse channel', async () => {
    const payload = makePipeline();

    await service.sendUpdate(payload, 'fcm-token', 'activity-token');

    expect(sendLiveActivity).toHaveBeenCalledWith(
      'fcm-token',
      'activity-token',
      expect.objectContaining({
        event: 'end',
        contentState: expect.objectContaining({ status: 'failed' }),
      }),
      'comeet-7-42',
    );
  });
});
