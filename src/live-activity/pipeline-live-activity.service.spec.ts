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
          { name: 'prepare', status: 'success' },
          { name: 'build', status: 'success' },
          { name: 'test', status: 'failed' },
          { name: 'deploy', status: 'pending' },
        ],
        currentStageName: 'test',
        failedStageName: 'test',
        failedJobName: 'unit tests',
        completedStageCount: 2,
        totalStageCount: 4,
        updatedAt: 1786320000,
      },
    });
  });

  it('marks active updates stale after 90 seconds and limits the visible stepper', () => {
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
    expect(update.staleDate).toBe(1786320090);
    expect(update.dismissalDate).toBeUndefined();
    expect(update.contentState.currentStageName).toBe('six');
    expect(update.contentState.stages).toHaveLength(6);
    expect(update.contentState.stages.map(({ name }) => name)).toContain('six');
  });
});
