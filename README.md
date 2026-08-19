# Comeet Notify

Self-hosted notification relay for the Comeet mobile app. Comeet Notify receives
GitLab webhook events, turns them into concise mobile notifications, and delivers
them through Firebase Cloud Messaging (FCM). Pipeline webhooks can remotely
start, update, and end iOS Live Activities.

```mermaid
flowchart LR
    GitLab["GitLab project"] -->|"Webhook + device headers"| Relay["Comeet Notify"]
    Relay -->|"Firebase Admin SDK"| FCM["Firebase Cloud Messaging"]
    FCM -->|"Push notification + deep-link data"| App["Comeet mobile app"]
    FCM -->|"ActivityKit update / end"| Activity["iOS Live Activity"]
```

## Features

- Runs entirely in your own infrastructure
- Supports push, merge request, issue, pipeline, and tag events
- Sends native Android and iOS notifications through FCM
- Includes project and event metadata for in-app deep links
- Updates and ends pipeline Live Activities with status snapshots and available stage details
- Checks required delivery headers and provides structured errors and request logs
- Ships with a multi-stage Docker image and PM2 runtime
- Exposes interactive OpenAPI documentation with Swagger UI

## Supported GitLab events

| GitLab trigger       | Event type      | Deep-link metadata               |
| -------------------- | --------------- | -------------------------------- |
| Push events          | `push`          | Project ID and commit SHA        |
| Merge request events | `merge_request` | Project ID and merge request IID |
| Issues events        | `issue`         | Project ID and issue IID         |
| Pipeline events      | `pipeline`      | Project ID and pipeline ID       |
| Tag push events      | `tag_push`      | Project ID                       |

Unsupported webhook event types are ignored without sending a notification.

## Requirements

- Node.js 20 or later
- npm
- A Firebase project with Cloud Messaging enabled
- A Firebase service-account key
- An FCM registration token from the Comeet mobile app
- A GitLab project where you can configure webhooks

## Quick start

1. Clone the repository and install dependencies:

   ```bash
   git clone https://github.com/monokaijs/comeet-notify.git
   cd comeet-notify
   npm ci
   ```

2. Create your local environment file:

   ```bash
   cp .env.example .env
   ```

3. Add your Firebase service-account credentials to `.env`:

   ```dotenv
   PORT=3000
   NODE_ENV=development
   LOG_LEVEL=info

   FIREBASE_PROJECT_ID=your-firebase-project-id
   FIREBASE_PRIVATE_KEY="-----BEGIN PRIVATE KEY-----\nYOUR_PRIVATE_KEY_HERE\n-----END PRIVATE KEY-----\n"
   FIREBASE_CLIENT_EMAIL=firebase-adminsdk-xxxxx@your-project.iam.gserviceaccount.com
   ```

4. Start the development server:

   ```bash
   npm run start:dev
   ```

The relay is available at `http://localhost:3000`, and the Swagger UI is
available at `http://localhost:3000/docs`.

## Firebase configuration

Create or select a Firebase project, then generate a service-account key from
**Project settings → Service accounts → Generate new private key**. Copy these
fields from the downloaded JSON file into `.env`:

| JSON field     | Environment variable    |
| -------------- | ----------------------- |
| `project_id`   | `FIREBASE_PROJECT_ID`   |
| `private_key`  | `FIREBASE_PRIVATE_KEY`  |
| `client_email` | `FIREBASE_CLIENT_EMAIL` |

Keep the private key on one quoted line and represent line breaks as `\n`, as
shown in `.env.example`. Never commit the service-account JSON file or a
populated `.env` file.

On Android, notifications target the `gitlab_notifications` channel. The Comeet
app must create this notification channel before messages arrive.

## GitLab webhook setup

In your GitLab project, open **Settings → Webhooks** and configure:

| Setting          | Value                                                               |
| ---------------- | ------------------------------------------------------------------- |
| URL              | `https://notify.example.com/webhooks/gitlab`                        |
| Custom header    | `X-FCM-Token: <comeet-device-token>`                                |
| Triggers         | Push, tag push, issue, merge request, and pipeline events as needed |
| SSL verification | Enabled                                                             |

The `X-GitLab-Event` header is read when GitLab includes it, but event handling
is determined from the webhook payload. Each request must include exactly one
target device token in `X-FCM-Token`.

Comeet stores the active pipeline registrations on the existing project webhook:

| Header                                | Purpose                                                   |
| ------------------------------------- | --------------------------------------------------------- |
| `X-Comeet-Instance-ID`                | GitLab instance used for notification and activity links  |
| `X-Pipeline-Delivery-Mode`            | `live_activity`, `notification`, or `both`                 |
| `X-Live-Activity-Registrations`       | JSON list of pipeline IDs and ActivityKit update tokens   |
| `X-Live-Activity-Token`               | Legacy single-activity token used during rolling upgrades |
| `X-Live-Activity-Pipeline-ID`         | Pipeline associated with the legacy token                 |
| `X-Live-Activity-Push-To-Start-Token` | ActivityKit token used to remotely start new activities   |

The relay sends a Live Activity update only for a pipeline event whose ID
exactly matches a registration. Tokens are validated and never logged. The
activity-scoped routing data remains on the GitLab webhook. A short-lived,
in-memory process guard also deduplicates repeated remote-start events.
Pipeline events must be enabled for the project's Comeet notification
subscription or GitLab will not deliver the updates to the relay.
The pipeline delivery mode independently controls regular notification and
Live Activity delivery. Missing or invalid mode headers default to `both` for
compatibility with older Comeet clients.

You can test the endpoint outside GitLab with a representative payload:

```bash
curl --request POST http://localhost:3000/webhooks/gitlab \
  --header "Content-Type: application/json" \
  --header "X-GitLab-Event: Push Hook" \
  --header "X-FCM-Token: YOUR_FCM_REGISTRATION_TOKEN" \
  --data '{
    "object_kind": "push",
    "ref": "refs/heads/main",
    "checkout_sha": "da1560886d4f094c3e6c9ef40349f7d38b5d27d7",
    "user_name": "Jane Developer",
    "project_id": 15,
    "total_commits_count": 1,
    "project": {
      "name": "example-project",
      "web_url": "https://gitlab.example.com/group/example-project"
    }
  }'
```

A successfully delivered notification returns:

```json
{
  "success": true,
  "message": "Notification sent successfully"
}
```

## API

| Method | Path               | Description                                                   |
| ------ | ------------------ | ------------------------------------------------------------- |
| `GET`  | `/`                | Basic liveness response                                       |
| `POST` | `/`                | Basic liveness response                                       |
| `POST` | `/webhooks/gitlab` | Send an FCM notification and an eligible Live Activity update |
| `GET`  | `/docs`            | Swagger UI and interactive API reference                      |

Notification data sent to the mobile app includes:

```json
{
  "eventType": "merge_request",
  "event_type": "merge_request",
  "repositoryName": "example-project",
  "repositoryUrl": "https://gitlab.example.com/group/example-project",
  "project_id": "15",
  "merge_request_iid": "42"
}
```

Event-specific identifiers are included only when applicable. All FCM data
values are serialized as strings.

## Configuration

| Variable                | Required | Default       | Description                                                     |
| ----------------------- | -------- | ------------- | --------------------------------------------------------------- |
| `PORT`                  | No       | `3000`        | HTTP port used by the service                                   |
| `NODE_ENV`              | No       | `development` | Application environment                                         |
| `LOG_LEVEL`             | No       | `info`        | Loaded log-level value; logger filtering is not yet wired to it |
| `FIREBASE_PROJECT_ID`   | Yes      | —             | Firebase project ID                                             |
| `FIREBASE_PRIVATE_KEY`  | Yes      | —             | Firebase service-account private key                            |
| `FIREBASE_CLIENT_EMAIL` | Yes      | —             | Firebase service-account client email                           |

If any Firebase credential is missing, the API starts but FCM delivery remains
unavailable and webhook requests fail.

## Docker

Build and run the image locally:

```bash
docker build --tag comeet-notify .
docker run --detach \
  --name comeet-notify \
  --restart unless-stopped \
  --env-file .env \
  --publish 3000:3000 \
  comeet-notify
```

Images built from the default branch are also published to GitHub Container
Registry:

```bash
docker pull ghcr.io/monokaijs/comeet-notify:latest
```

The container uses one PM2 worker so its remote-start deduplication guard remains
consistent. The service does not require a database or persistent volume.
Deployments that run multiple relay replicas must add a shared deduplication
store before enabling remote starts. Webhook delivery is synchronous; the relay
does not currently maintain a queue or its own retry state.

FCM notification and Live Activity delivery are best effort and do not reject an
otherwise valid GitLab webhook. GitLab pipeline webhooks describe status changes,
not every job transition, so remote activities contain snapshots rather than a
job-level event stream. Active states become stale after 15 minutes without
another pipeline event. Completed activities remain visible for 15 minutes;
failures remain for one hour so the failed stage and job can be inspected.
`manual` and `scheduled` pipelines remain active because GitLab can resume them.
The Comeet app reconciles active activities whenever it opens or returns to the
foreground, removes duplicates, and ends activities whose pipeline has finished.

## Production guidance

- Put the relay behind an HTTPS reverse proxy.
- Restrict access to `/webhooks/gitlab` by source network or at the proxy layer.
- Add rate limiting and request-size limits at the ingress.
- Treat Firebase credentials and FCM registration tokens as secrets.
- Restrict or disable public access to `/docs` if the API reference is not
  intended to be public.
- Update the GitLab custom header when the Comeet app issues a new device token.

> [!IMPORTANT]
> The current application does not validate GitLab's webhook secret-token
> header. Do not expose the relay directly to an untrusted network without
> compensating controls.

## Development

```bash
npm run start:dev   # Start with file watching
npm run build       # Compile the application
npm run start:prod  # Run the compiled application
npm run lint        # Lint and apply safe fixes
npm test            # Run unit tests
```

Project layout:

```text
src/
├── common/       # Logging and exception handling
├── config/       # Environment-backed configuration
├── fcm/          # Firebase initialization and message delivery
├── webhooks/     # GitLab endpoint, event parser, and DTOs
├── app.module.ts
└── main.ts
```

## Troubleshooting

**`Firebase configuration is incomplete`**

Confirm that all three `FIREBASE_*` variables are present. If using Docker,
verify that the container receives the intended environment file.

**`Firebase not initialized`**

Check the service-account values and container logs. In particular, make sure
the private key contains escaped `\n` line breaks and remains wrapped in quotes.

**`Invalid FCM token`**

The registration token is expired, revoked, or belongs to a different Firebase
project. Obtain a current token from the Comeet app and update the GitLab custom
header.

**Webhook returns `400 Bad Request`**

Ensure `X-FCM-Token` is set and the selected GitLab event is supported. Review
the application logs for the Firebase error reported during delivery.

## License

Comeet Notify is available under the [MIT License](LICENSE).
