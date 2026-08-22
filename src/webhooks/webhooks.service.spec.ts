import { FcmService } from '../fcm/fcm.service';
import { PipelineLiveActivityService } from '../live-activity/pipeline-live-activity.service';
import {
  GitLabEventParserService,
  GitLabEventType,
} from './gitlab-event-parser.service';
import { GitLabPipelineEvent } from './dto/gitlab-webhook.dto';
import { WebhooksService } from './webhooks.service';

const pipeline = {
  object_kind: 'pipeline',
  object_attributes: { id: 42, status: 'running', ref: 'main', stages: [] },
  project: {
    id: 7,
    name: 'Comeet',
    web_url: 'https://gitlab.example.com/comeet',
  },
  builds: [],
} as unknown as GitLabPipelineEvent;

describe('WebhooksService Live Activity routing', () => {
  const parseEvent = jest.fn().mockReturnValue({
    eventType: GitLabEventType.PIPELINE,
    title: 'Pipeline started',
    message: 'Pipeline is running',
    repositoryName: 'Comeet',
    repositoryUrl: 'https://gitlab.example.com/comeet',
    deepLinkData: { event_type: 'pipeline', project_id: 7, pipeline_id: 42 },
  });
  const sendNotification = jest.fn().mockResolvedValue({ success: true });
  const sendUpdate = jest
    .fn()
    .mockResolvedValue({ success: false, error: 'expired token' });
  const sendStart = jest.fn().mockResolvedValue({ success: true });
  const service = new WebhooksService(
    { parseEvent } as unknown as GitLabEventParserService,
    { sendNotification } as unknown as FcmService,
    { sendUpdate, sendStart } as unknown as PipelineLiveActivityService,
  );

  beforeEach(() => {
    sendNotification.mockClear();
    sendUpdate.mockClear();
    sendStart.mockClear();
  });

  it('routes a matching activity token without failing normal notification delivery', async () => {
    const token = 'a'.repeat(64);

    await expect(
      service.processWebhook(pipeline, 'fcm-token', {
        token,
        pipelineId: '42',
      }),
    ).resolves.toBeUndefined();

    expect(sendNotification).toHaveBeenCalledTimes(1);
    expect(sendUpdate).toHaveBeenCalledWith(pipeline, 'fcm-token', token);
  });

  it('includes the GitLab instance in normal notification navigation data', async () => {
    await service.processWebhook(pipeline, 'instance-fcm-token', {
      instanceId: 'gitlab-work',
    });

    expect(sendNotification).toHaveBeenCalledWith(
      'instance-fcm-token',
      expect.objectContaining({
        data: expect.objectContaining({ instance_id: 'gitlab-work' }),
      }),
    );
  });

  it('acknowledges the webhook when normal FCM delivery fails', async () => {
    sendNotification.mockResolvedValueOnce({
      success: false,
      error: 'expired FCM token',
    });

    await expect(
      service.processWebhook(pipeline, 'expired-fcm-token'),
    ).resolves.toBeUndefined();
  });

  it('does not route a token for another pipeline', async () => {
    await service.processWebhook(pipeline, 'fcm-token', {
      token: 'b'.repeat(64),
      pipelineId: '41',
    });

    expect(sendNotification).toHaveBeenCalledTimes(1);
    expect(sendUpdate).not.toHaveBeenCalled();
  });

  it('starts a running pipeline when only a push-to-start token is registered', async () => {
    const pushToStartToken = 'c'.repeat(64);

    await service.processWebhook(pipeline, 'fcm-token', {
      pushToStartToken,
    });

    expect(sendStart).toHaveBeenCalledWith(
      pipeline,
      'fcm-token',
      pushToStartToken,
      undefined,
    );
    expect(sendUpdate).not.toHaveBeenCalled();
  });

  it('prefers an existing pipeline update token over starting a duplicate activity', async () => {
    await service.processWebhook(pipeline, 'fcm-token', {
      token: 'd'.repeat(64),
      pipelineId: '42',
      pushToStartToken: 'e'.repeat(64),
    });

    expect(sendUpdate).toHaveBeenCalledTimes(1);
    expect(sendStart).not.toHaveBeenCalled();
  });

  it('sends only a Live Activity when live_activity delivery is selected', async () => {
    const token = '4'.repeat(64);

    await service.processWebhook(pipeline, 'live-only-fcm-token', {
      pipelineDeliveryMode: 'live_activity',
      token,
      pipelineId: '42',
    });

    expect(sendNotification).not.toHaveBeenCalled();
    expect(sendUpdate).toHaveBeenCalledWith(
      pipeline,
      'live-only-fcm-token',
      token,
    );
  });

  it('sends only a notification when notification delivery is selected', async () => {
    await service.processWebhook(pipeline, 'notification-only-fcm-token', {
      pipelineDeliveryMode: 'notification',
      token: '5'.repeat(64),
      pipelineId: '42',
      pushToStartToken: '6'.repeat(64),
    });

    expect(sendNotification).toHaveBeenCalledTimes(1);
    expect(sendUpdate).not.toHaveBeenCalled();
    expect(sendStart).not.toHaveBeenCalled();
  });

  it('keeps both delivery channels enabled when both is selected', async () => {
    const token = '7'.repeat(64);

    await service.processWebhook(pipeline, 'both-fcm-token', {
      pipelineDeliveryMode: 'both',
      token,
      pipelineId: '42',
    });

    expect(sendNotification).toHaveBeenCalledTimes(1);
    expect(sendUpdate).toHaveBeenCalledTimes(1);
  });

  it('routes the matching token from multiple pipeline registrations', async () => {
    const token42 = 'f'.repeat(64);
    const token43 = '1'.repeat(64);

    await service.processWebhook(pipeline, 'multi-fcm-token', {
      registrations: JSON.stringify([
        { pipelineId: 43, pushToken: token43 },
        { pipelineId: 42, pushToken: token42 },
      ]),
      pushToStartToken: '2'.repeat(64),
    });

    expect(sendUpdate).toHaveBeenCalledWith(
      pipeline,
      'multi-fcm-token',
      token42,
    );
    expect(sendStart).not.toHaveBeenCalled();
  });

  it('uses the newest eight registrations from older unbounded clients', async () => {
    const registrations = Array.from({ length: 9 }, (_, index) => ({
      pipelineId: index === 0 ? 42 : 100 + index,
      pushToken: String(index + 1).repeat(64),
    }));

    await service.processWebhook(pipeline, 'bounded-fcm-token', {
      registrations: JSON.stringify(registrations),
      pushToStartToken: 'a'.repeat(64),
    });

    expect(sendUpdate).not.toHaveBeenCalled();
    expect(sendStart).toHaveBeenCalledTimes(1);
  });

  it('restarts a running pipeline after its activity registration expires', async () => {
    const expiredToken = 'b'.repeat(64);

    await service.processWebhook(pipeline, 'expired-activity-fcm-token', {
      registrations: JSON.stringify([
        { pipelineId: 42, pushToken: expiredToken, registeredAt: 1 },
      ]),
      token: expiredToken,
      pipelineId: '42',
      pushToStartToken: 'c'.repeat(64),
    });

    expect(sendUpdate).not.toHaveBeenCalled();
    expect(sendStart).toHaveBeenCalledTimes(1);
  });

  it('deduplicates repeated remote starts for the same device and pipeline', async () => {
    const repeatedPipeline = {
      ...pipeline,
      object_attributes: { ...pipeline.object_attributes, id: 99 },
    } as GitLabPipelineEvent;
    const headers = {
      pushToStartToken: '3'.repeat(64),
      instanceId: 'gitlab-1',
    };

    await service.processWebhook(repeatedPipeline, 'dedupe-fcm-token', headers);
    await service.processWebhook(repeatedPipeline, 'dedupe-fcm-token', headers);

    expect(sendStart).toHaveBeenCalledTimes(1);
    expect(sendStart).toHaveBeenCalledWith(
      repeatedPipeline,
      'dedupe-fcm-token',
      headers.pushToStartToken,
      'gitlab-1',
    );
  });
});
