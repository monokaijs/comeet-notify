import { ApiProperty } from '@nestjs/swagger';

export class GitLabWebhookDto {
  @ApiProperty({ 
    example: 'push', 
    description: 'Type of GitLab event',
    enum: ['push', 'merge_request', 'issue', 'pipeline', 'tag_push']
  })
  object_kind: string;

  @ApiProperty({ example: 'push', description: 'Event name' })
  event_name?: string;

  @ApiProperty({ example: 'John Doe', description: 'User who triggered the event' })
  user_name?: string;

  @ApiProperty({ 
    description: 'Project information',
    example: {
      id: 15,
      name: 'example-project',
      web_url: 'https://gitlab.example.com/group/project'
    }
  })
  project?: {
    id: number;
    name: string;
    web_url: string;
  };

  @ApiProperty({ 
    description: 'Repository information',
    example: {
      name: 'example-project',
      homepage: 'https://gitlab.example.com/group/project'
    }
  })
  repository?: {
    name: string;
    homepage: string;
  };
}
