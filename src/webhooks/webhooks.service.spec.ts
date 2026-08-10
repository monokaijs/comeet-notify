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
  const service = new WebhooksService(
    { parseEvent } as unknown as GitLabEventParserService,
    { sendNotification } as unknown as FcmService,
    { sendUpdate } as unknown as PipelineLiveActivityService,
  );

  beforeEach(() => {
    sendNotification.mockClear();
    sendUpdate.mockClear();
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

  it('does not route a token for another pipeline', async () => {
    await service.processWebhook(pipeline, 'fcm-token', {
      token: 'b'.repeat(64),
      pipelineId: '41',
    });

    expect(sendNotification).toHaveBeenCalledTimes(1);
    expect(sendUpdate).not.toHaveBeenCalled();
  });
});
