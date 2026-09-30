# Comeet Notify

Self-hosted notification relay for the Comeet mobile app. Comeet Notify receives
GitLab webhook events, turns them into concise mobile notifications, and delivers
them through Firebase Cloud Messaging (FCM). Pipeline webhooks can remotely
start, update, and end iOS Live Activities.

## Features

- Runs entirely in your own infrastructure
- Supports push, merge request, issue, pipeline, and tag events
- Sends native Android and iOS notifications through FCM
- Includes project and event metadata for in-app deep links
- Updates and ends pipeline Live Activities with status snapshots and available stage details
- Checks required delivery headers and provides structured errors and request logs
- Runs as a single memory-efficient Axum/Tokio binary
- Ships with a multi-stage, non-root Debian container image
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

- Rust 1.93 or later
- Cargo
- A Firebase project with Cloud Messaging enabled
- A Firebase service-account key
- An FCM registration token from the Comeet mobile app
- A GitLab project where you can configure webhooks

## Quick start

1. Clone the repository:

   ```bash
   git clone https://github.com/monokaijs/comeet-notify.git
   cd comeet-notify
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
   cargo run
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
      "id": 15,
      "name": "example-project",
      "web_url": "https://gitlab.example.com/group/example-project"
    }
  }'
```

A valid, acknowledged webhook returns this response even when best-effort FCM
delivery fails:

```json
{
  "success": true,
  "message": "Webhook processed successfully"
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
| `LOG_LEVEL`             | No       | `info`        | Tracing filter, such as `info`, `debug`, or a module directive   |
| `FIREBASE_PROJECT_ID`   | Yes      | —             | Firebase project ID                                             |
| `FIREBASE_PRIVATE_KEY`  | Yes      | —             | Firebase service-account private key                            |
| `FIREBASE_CLIENT_EMAIL` | Yes      | —             | Firebase service-account client email                           |

If any Firebase credential is missing or the private key is invalid, the API
starts with FCM disabled. Valid GitLab webhooks are still acknowledged because
notification delivery is best effort.

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

Images are built for `linux/amd64`. Default-branch images are published to
GitHub Container Registry:

```bash
docker pull ghcr.io/monokaijs/comeet-notify:latest
```

The container runs one non-root Rust process with two Tokio worker threads, an
internal health check, and graceful SIGTERM handling. The service does not
require PostgreSQL, another database, or a persistent volume because all routing
state remains on the GitLab webhook and the only server-side guard is temporary
remote-start deduplication.
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

## Automated deployment

Pushes to `master` run formatting, Clippy, and Rust tests, then build and publish
one AMD64 image and deploy that image by its immutable digest. The Docker build
compiles the release binary; CI does not build a separate smoke-test image.
Production deployment uses the existing SSH key and pinned known-hosts secrets,
checks container health, and restores the previous image if startup fails.

## Development

```bash
cargo run                                      # Start the service
cargo fmt --all -- --check                     # Check formatting
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
cargo build --release --locked                 # Optimized production binary
target/release/comeet-notify healthcheck       # Probe a running local service
```

Project layout:

```text
src/
├── app.rs            # Axum routes, middleware, errors, and OpenAPI
├── config.rs         # Environment-backed configuration
├── fcm.rs            # OAuth token cache and FCM HTTP v1 delivery
├── live_activity.rs  # ActivityKit pipeline state builder
├── models.rs         # GitLab and response models
├── parser.rs         # GitLab notification parser
├── webhooks.rs       # Delivery routing and remote-start deduplication
├── lib.rs
└── main.rs           # Two-thread Tokio runtime and healthcheck command

tests/
├── fcm_contract.rs
├── http_contract.rs
└── routing_contract.rs
```

## Troubleshooting

**`Firebase configuration is incomplete`**

Confirm that all three `FIREBASE_*` variables are present. If using Docker,
verify that the container receives the intended environment file.

**`FCM is disabled`**

Check the service-account values and container logs. In particular, make sure
the private key contains escaped `\n` line breaks and remains wrapped in quotes.

**`Invalid FCM token`**

The registration token is expired, revoked, or belongs to a different Firebase
project. Obtain a current token from the Comeet app and update the GitLab custom
header.

**Webhook returns `400 Bad Request`**

Ensure `X-FCM-Token` is set and the JSON body is valid. Unsupported GitLab event
types are intentionally acknowledged with `201` and do not send a message.

## License

Comeet Notify is available under the [MIT License](LICENSE).
