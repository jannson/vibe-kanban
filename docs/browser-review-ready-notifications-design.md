# Remote Review-Ready Notifications Design

## Context

The existing Vibe Kanban notification flow is backend-driven and targets the operating system where the server process is running.

This works when Vibe Kanban runs on the same desktop machine as the user, but it breaks down for the remote-server workflow:

- the user SSHes from their laptop or desktop into a remote Linux machine
- Vibe Kanban runs on that remote Linux machine
- backend notifications currently fire on the remote machine, not on the user's local computer

Browser-based audio was considered as a workaround, but it is not reliable enough for the target workflow:

- background browser tabs are heavily constrained
- audio playback from a hidden tab is inconsistent across browsers and OSes
- by the time the user foregrounds the tab, they have already seen the state change

The design direction in this document is therefore:

- keep completion detection in the backend
- deliver the notification event back to the currently connected computer
- let that local computer play the sound and optionally show a local OS notification

## Problem Statement

When a task moves from `In Progress` to `In Review`, the user wants an audible notification on the local computer they are currently using, even though Vibe Kanban is running remotely over SSH.

The desired behavior is:

- the remote server detects task completion as it already does
- the notification event is routed to the user's local machine
- the local machine emits sound immediately
- the mechanism should work even when the browser is hidden or not focused
- the design should fit naturally into the existing backend notification flow

## Goals

- Reuse backend task-completion detection
- Deliver notifications to the SSH client's local computer
- Support local sound playback on the user's actual machine
- Avoid dependence on browser autoplay behavior
- Keep the implementation minimally invasive to existing completion flow
- Support one active computer per SSH session in v1

## Non-Goals

- Building a general-purpose cross-device notification system in v1
- Synching notifications to all machines a user owns
- Replacing all existing OS notification channels immediately
- Solving notification delivery for fully headless, non-SSH access patterns in v1

## Recommended Architecture

The recommended design is:

1. Run a small local notifier program on the user's computer
2. Open an SSH reverse tunnel from the remote server back to the local notifier
3. When the backend detects task completion, it sends an HTTP request to the reverse-tunnel endpoint
4. The request arrives at the user's local notifier
5. The local notifier plays sound and optionally displays a local desktop notification

This keeps the event source in the backend, but relocates the sound output to the correct machine.

## High-Level Flow

```text
User Computer
  local notifier listening on 127.0.0.1:43210
        ^
        | SSH reverse tunnel
        |
Remote Server
  vibe-kanban backend
    -> task finishes
    -> backend calls http://127.0.0.1:<remote-forwarded-port>/notify
    -> SSH reverse port forwards request to local notifier
    -> local notifier plays sound on the user's computer
```

## Why This Direction Is Better Than Browser Audio

Browser audio was rejected as the primary solution because:

- background tab audio is unreliable
- browser autoplay rules vary
- hidden-tab execution is throttled
- the user specifically wants background audible notification

Local notifier delivery is better because:

- sound is played by the local OS, not by a hidden web page
- it does not depend on browser tab focus
- it maps naturally to the "I am currently SSHed in from this machine" workflow

## Core Components

### 1. Remote backend completion detection

This part already exists:

- task completion is detected in backend container/finalization code
- the backend already has a notification service abstraction

This design reuses that trigger point rather than moving completion detection into the browser.

### 2. Local notifier

A small local program running on the user's computer.

Responsibilities:

- listen on a local HTTP port
- authenticate incoming requests
- receive a notification payload
- play a local sound
- optionally show a local desktop notification

Suggested implementation forms:

- Node.js script
- Python script
- small tray app later if needed

For v1, a simple local HTTP server is sufficient.

### 3. SSH reverse tunnel

The SSH client establishes a reverse tunnel when connecting to the server.

Conceptually:

- local notifier listens on `127.0.0.1:43210` on the user's computer
- SSH exposes that local port on the remote side as something like `127.0.0.1:43110`
- the remote backend posts notification events to `127.0.0.1:43110`

This allows the remote backend to reach the local notifier without exposing the local machine publicly.

### 4. Backend remote notifier channel

Extend the backend notification service to support an additional delivery channel:

- local/remote HTTP notifier endpoint

This should be an additive channel, not a full replacement for existing notifications.

## Proposed v1 Behavior

When a task enters `In Review` after completion:

- backend detects completion as it already does
- backend calls the notifier service
- notifier service:
  - keeps existing local-server notification behavior as-is or configurable
  - additionally sends an HTTP POST to the remote notifier endpoint if configured
- local notifier receives the event
- local notifier plays the configured sound on the user's current machine

If the remote notifier is unavailable:

- backend logs the failure
- task completion flow continues normally
- notification failure does not block task finalization

## Configuration Model

The design should distinguish between:

- existing server-local OS notifications
- new remote-to-local notifications via SSH reverse tunnel

The recommended model is not a single remote target. It should be a list of independently configured notifier targets.

### Recommended top-level config shape

At the backend config layer:

```ts
type RemoteNotifierConfig = {
  enabled: boolean;
  targets: RemoteNotifierTarget[];
};
```

Suggested field names in persisted config:

- `remote_notifiers_enabled: bool`
- `remote_notifiers: RemoteNotifierTarget[]`

This allows:

- multiple local computers
- different routing rules per target
- future expansion to non-SSH delivery endpoints

### Recommended `remote_notifiers: []` structure

```ts
type RemoteNotifierTarget = {
  id: string;
  enabled: boolean;
  label: string | null;
  url: string;
  token: string | null;
  projects: 'ALL' | string[];
  title_regex: string | null;
  timeout_ms: number | null;
  sound_enabled: boolean;
  desktop_enabled: boolean;
};
```

Suggested persisted semantics:

- `id`
  - stable identifier for logging and UI editing
- `enabled`
  - master on/off switch for this target
- `label`
  - human-readable name, such as `work-macbook` or `office-linux`
- `url`
  - remote-side forwarded URL, e.g. `http://127.0.0.1:43110/notify`
- `token`
  - shared secret or bearer token for local notifier auth
- `projects`
  - `ALL` means all projects
  - string array means only those projects match
- `title_regex`
  - optional regex applied to task title
- `timeout_ms`
  - per-target timeout override
- `sound_enabled`
  - whether this target should cause sound on the local notifier
- `desktop_enabled`
  - whether this target should request a local desktop notification

### Why a list is better than a single target

This model supports:

- one notifier per computer
- one notifier per user context
- targeted routing for project subsets
- different sound/desktop behavior per destination

Example use cases:

- work laptop only receives tasks from `client-a`
- home desktop only receives tasks matching `^urgent:`
- one notifier target receives all tasks, another receives only production-related tasks

### Why filtering belongs primarily on the server

The server should decide whether a target should receive an event.

That is better than sending every event to every local notifier and filtering locally because:

- only relevant events are transmitted
- notifier programs stay smaller and simpler
- routing logic remains centralized and auditable
- fewer duplicate notifications across machines
- less task metadata is sprayed to uninterested targets

### Why separate config is needed

Existing notification config semantics currently mean:

- play sound on the machine running the backend
- show push notification on the machine running the backend

That is a different delivery target from the local notifier path. Reusing the same booleans would blur two distinct behaviors.

## Matching And Routing Algorithm

The backend should evaluate each completion event against every enabled remote notifier target.

### Inputs to matching

For each completion event, the backend has access to:

- task id
- task title
- project id
- project name
- workspace id
- session id
- final task status
- executor
- completion timestamp

For each target, the backend has:

- `enabled`
- `projects`
- `title_regex`
- `url`
- `token`
- `timeout_ms`

### Recommended matching order

For each target in `remote_notifiers`:

1. Skip if global remote notifier feature is disabled
2. Skip if target `enabled` is false
3. Skip if `url` is empty or invalid
4. Match project filter
5. Match title regex filter
6. If both filters pass, send notification request

### Project filter semantics

If `projects === 'ALL'`:

- all projects match

If `projects` is a list:

- the event matches only if the current task's project identifier is in the list

Recommended identifier for matching:

- use internal `project_id`

Optional later enhancement:

- allow matching by project name in addition to `project_id`

### Title regex semantics

If `title_regex === null` or empty:

- title matches by default

If `title_regex` is present:

- compile regex once when config is loaded or validated
- apply it to the task title
- if regex compilation fails, mark target invalid and skip it

Recommended behavior on invalid regex:

- do not fail all notifications
- log configuration error with target `id`
- skip only that target

### Match pseudocode

```ts
for (const target of remote_notifiers) {
  if (!remote_notifiers_enabled) continue;
  if (!target.enabled) continue;
  if (!isValidUrl(target.url)) continue;

  const projectMatched =
    target.projects === 'ALL' ||
    target.projects.includes(task.project_id);

  if (!projectMatched) continue;

  const titleMatched =
    !target.title_regex || regex(target.title_regex).test(task.title);

  if (!titleMatched) continue;

  sendNotification(target, payload);
}
```

### Delivery semantics

Matching is one-to-many:

- one task completion may trigger zero targets
- one task completion may trigger one target
- one task completion may trigger multiple targets

This is intentional. Different computers may subscribe to overlapping project/title rules.

## Backend Config And Rust Type Design

This section defines the recommended backend-side Rust model for the new remote notifier configuration.

### Current config baseline

Today the backend config root is:

- `Config` in `crates/services/src/services/config/versions/v8.rs`

and existing notifications are represented by:

- `NotificationConfig`

which currently model only server-local notification behavior.

The remote notifier design should be added as a new config section rather than overloaded into the current `NotificationConfig`.

### Recommended new config section

Add a new field to `Config`:

```rust
pub remote_notifications: RemoteNotificationsConfig,
```

Recommended placement:

- top-level sibling of `notifications`

Reason:

- existing `notifications` already mean "notify on the server host"
- `remote_notifications` clearly means "deliver to external/local-client notifiers"

### Recommended Rust types

```rust
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct RemoteNotificationsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub targets: Vec<RemoteNotifierTarget>,
    #[serde(default = "default_remote_notification_timeout_ms")]
    pub default_timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct RemoteNotifierTarget {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub label: Option<String>,
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub projects: RemoteNotifierProjectFilter,
    #[serde(default)]
    pub title_regex: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default = "default_true")]
    pub sound_enabled: bool,
    #[serde(default)]
    pub desktop_enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS, Default)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RemoteNotifierProjectFilter {
    #[default]
    All,
    ProjectIds(Vec<String>),
}
```

### Why this shape is recommended

`RemoteNotificationsConfig` contains:

- one global master switch
- one list of independently configured targets
- one default timeout to avoid repeating the same value on every target

`RemoteNotifierTarget` contains:

- stable identity for logs and UI editing
- per-target enable/disable
- per-target auth
- per-target project filter
- per-target title filter
- per-target output preferences

`RemoteNotifierProjectFilter` is an enum instead of a raw union type because:

- Rust models it cleanly
- serde can validate it explicitly
- generated TypeScript can remain precise

### Alternative JSON shape

If the tagged enum representation is considered too noisy for the config file, a looser JSON shape may be used:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum RemoteNotifierProjectFilter {
    All(String),
    ProjectIds(Vec<String>),
}
```

with `"ALL"` as the serialized sentinel.

Recommendation:

- use the cleaner internal enum design first in the Rust document model
- decide later whether persisted JSON should remain tagged or use a friendlier custom serializer

### Recommended defaults

Recommended defaults:

```rust
impl Default for RemoteNotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            targets: Vec::new(),
            default_timeout_ms: 1500,
        }
    }
}
```

Per-target defaults:

- `enabled = true`
- `projects = All`
- `title_regex = None`
- `timeout_ms = None`
- `sound_enabled = true`
- `desktop_enabled = false`

This yields safe startup behavior:

- remote delivery is fully off until explicitly enabled
- once enabled, targets behave sensibly with minimal required fields

### Config migration strategy

The current config version is `v8`.

Recommended next step:

- introduce `v9`
- add `remote_notifications` to the top-level `Config`
- migrate from `v8` by populating `RemoteNotificationsConfig::default()`

Recommended migration behavior:

- no attempt to infer targets from old config
- all remote notifier functionality starts disabled after migration

That keeps migration low-risk.

### Validation responsibilities

Validation should happen on config load and config save boundaries.

Recommended validation areas:

- target `id` must be non-empty
- target `id` must be unique across `targets`
- target `url` must parse as HTTP or HTTPS
- `timeout_ms`, if present, must be within a reasonable bound
- `title_regex`, if present, must compile successfully
- `projects`, if `ProjectIds`, should not contain duplicates

Recommended validation location:

- config validation helpers in the config module, not inside request sending code

Reason:

- keeps runtime delivery path simple
- invalid targets can be rejected or marked invalid before completion events are processed

### URL validation

Recommended accepted schemes:

- `http`
- `https`

Even though the primary SSH reverse tunnel example uses loopback HTTP, allowing HTTPS keeps the model general for future remote endpoints.

Recommended rejected values:

- empty URL
- malformed URL
- unsupported schemes

### Regex validation

Recommended behavior:

- compile regex once during validation
- if invalid, reject config save or surface validation error

Do not defer regex compilation to notification-send time if it can be avoided.

Reason:

- invalid configuration should fail early
- delivery path should not recompile patterns repeatedly

### Project identifier type

Recommended matching key:

- internal `project_id`

Recommended stored Rust type:

```rust
pub enum RemoteNotifierProjectFilter {
    All,
    ProjectIds(Vec<String>),
}
```

Reason for `String` over `Uuid` in config model:

- config persistence and UI editing stay simpler
- avoids validation coupling at deserialization time
- IDs can still be validated later if desired

If stricter typing becomes important later, this may be evolved to `Vec<Uuid>`.

## Backend Runtime Routing Model

The config model above needs a runtime representation that is cheap to evaluate during task completion.

### Recommended runtime types

Keep the persisted config shape simple, but compile it into a validated runtime shape when loaded.

Suggested runtime model:

```rust
pub struct CompiledRemoteNotifierTarget {
    pub id: String,
    pub enabled: bool,
    pub label: Option<String>,
    pub url: reqwest::Url,
    pub token: Option<String>,
    pub projects: CompiledProjectFilter,
    pub title_regex: Option<regex::Regex>,
    pub timeout_ms: u64,
    pub sound_enabled: bool,
    pub desktop_enabled: bool,
}

pub enum CompiledProjectFilter {
    All,
    ProjectIds(std::collections::HashSet<String>),
}
```

### Why a compiled runtime representation helps

- URL parsing happens once
- regex compilation happens once
- project membership checks become cheap
- invalid targets can be omitted from runtime routing cleanly

### Recommended compilation point

Options:

1. Compile targets every time config is read
2. Compile targets once when config is loaded or updated

Recommendation:

- compile once on config load/save/update
- store compiled targets inside the notification service or a config-derived runtime cache

This avoids repeated regex and URL parsing on every completion event.

## NotificationService Integration Design

The current `NotificationService` API is:

```rust
pub async fn notify(&self, title: &str, message: &str)
```

That is insufficient for remote target matching because remote routing needs structured task metadata.

### Recommended API evolution

Keep the current simple API for server-local notifications if needed, but add a structured path for task-completion notifications.

Recommended new input type:

```rust
pub struct ReviewReadyNotificationEvent {
    pub task_id: String,
    pub task_title: String,
    pub project_id: String,
    pub project_name: Option<String>,
    pub workspace_id: String,
    pub session_id: String,
    pub status: String,
    pub branch: Option<String>,
    pub executor: Option<String>,
    pub completed_at: chrono::DateTime<chrono::Utc>,
}
```

Recommended service methods:

```rust
impl NotificationService {
    pub async fn notify_local(&self, title: &str, message: &str);
    pub async fn notify_review_ready(&self, event: &ReviewReadyNotificationEvent);
}
```

### Why a structured event is needed

The server must match by:

- project id
- task title

and must send a structured payload to remote notifiers.

Trying to reconstruct this from `(title, message)` would be brittle and error-prone.

### Recommended send path split

`notify_review_ready(event)` should internally perform:

1. optional existing server-local notifications
2. optional remote notifier routing

Conceptually:

```rust
pub async fn notify_review_ready(&self, event: &ReviewReadyNotificationEvent) {
    self.maybe_send_server_local_notification(event).await;
    self.maybe_send_remote_notifications(event).await;
}
```

This keeps server-local and remote-local delivery cleanly separated.

## Remote Sender Design

### HTTP client behavior

Recommended transport:

- `reqwest` async client

Recommended sender behavior per matching target:

- build JSON payload
- include bearer token if configured
- apply per-target timeout or default timeout
- send POST request
- treat non-2xx as delivery failure
- log target id and HTTP status

### Failure policy

Remote delivery is best-effort:

- no retries in the initial synchronous path
- no task-finalization failure on send error
- log and continue

Optional later enhancement:

- background retry queue

Do not include retry complexity in v1.

## Logging And Observability

Recommended structured logs on remote delivery:

- target id
- target label
- task id
- project id
- match result
- skip reason when skipped
- HTTP status when request fails

Recommended skip reasons:

- global_disabled
- target_disabled
- invalid_target
- project_filter_miss
- title_regex_miss

This will make routing behavior debuggable in production.

## Recommended examples

### Example 1. Receive all projects

```json
{
  "id": "work-macbook",
  "enabled": true,
  "label": "Work MacBook",
  "url": "http://127.0.0.1:43110/notify",
  "token": "secret-1",
  "projects": "ALL",
  "title_regex": null,
  "timeout_ms": 1500,
  "sound_enabled": true,
  "desktop_enabled": true
}
```

### Example 2. Only specific projects

```json
{
  "id": "office-linux",
  "enabled": true,
  "label": "Office Linux",
  "url": "http://127.0.0.1:43111/notify",
  "token": "secret-2",
  "projects": ["project-uuid-a", "project-uuid-b"],
  "title_regex": null,
  "timeout_ms": 1500,
  "sound_enabled": true,
  "desktop_enabled": false
}
```

### Example 3. Only urgent titles

```json
{
  "id": "urgent-only",
  "enabled": true,
  "label": "Urgent Tasks",
  "url": "http://127.0.0.1:43112/notify",
  "token": "secret-3",
  "projects": "ALL",
  "title_regex": "^(urgent|prod|p0):",
  "timeout_ms": 1500,
  "sound_enabled": true,
  "desktop_enabled": true
}
```

## Notification Payload

The remote backend should send a small signed or token-authenticated JSON payload.

Suggested request:

```http
POST /notify HTTP/1.1
Authorization: Bearer <token>
Content-Type: application/json
```

Suggested body:

```json
{
  "event": "task_review_ready",
  "task_id": "uuid",
  "task_title": "Fix flaky CI",
  "project_id": "uuid",
  "project_name": "backend-platform",
  "workspace_id": "uuid",
  "session_id": "uuid",
  "status": "inreview",
  "branch": "vk/fix-flaky-ci",
  "executor": "codex",
  "completed_at": "2026-03-26T12:34:56Z"
}
```

Minimum required fields for v1:

- `event`
- `task_id`
- `task_title`
- `project_name`
- `status`
- `completed_at`

### Recommended versioned payload contract

To keep the local notifier forward-compatible, the payload should include a schema version.

Recommended additional fields:

```json
{
  "schema_version": "v1",
  "event": "task_review_ready",
  "delivery": {
    "target_id": "work-macbook",
    "sound_enabled": true,
    "desktop_enabled": true
  }
}
```

Recommended v1 payload shape:

```json
{
  "schema_version": "v1",
  "event": "task_review_ready",
  "task_id": "uuid",
  "task_title": "Fix flaky CI",
  "project_id": "uuid",
  "project_name": "backend-platform",
  "workspace_id": "uuid",
  "session_id": "uuid",
  "status": "inreview",
  "branch": "vk/fix-flaky-ci",
  "executor": "codex",
  "completed_at": "2026-03-26T12:34:56Z",
  "delivery": {
    "target_id": "work-macbook",
    "sound_enabled": true,
    "desktop_enabled": true
  }
}
```

`delivery` is derived from the matched remote notifier target and allows the local notifier to stay mostly stateless.

## Local Notifier API

### Endpoint

Recommended v1 endpoint:

- `POST /notify`

Additional recommended endpoints:

- `GET /health`
- `POST /test`

These two endpoints make tunnel setup and local validation much easier.

### Authentication

Use a bearer token or shared secret header.

Recommended v1:

- backend sends `Authorization: Bearer <remote_notifier_token>`
- local notifier rejects requests without a matching token

This is sufficient because:

- the port is only reachable through SSH reverse tunnel
- the token protects against accidental or misrouted traffic

### Token verification rules

Recommended v1 rules:

- require token auth on `POST /notify`
- require token auth on `POST /test`
- `GET /health` may be unauthenticated if it only returns process liveness
- use constant-time token comparison
- if token is missing or invalid, return `401 Unauthorized`

If `token` is unset for a target:

- backend sends no auth header
- local notifier may be configured to allow unauthenticated requests on loopback-only mode

Recommendation:

- prefer token-required mode by default
- only allow unauthenticated mode as an explicit local development option

### Expected response

Successful:

```json
{
  "ok": true
}
```

Failure:

```json
{
  "ok": false,
  "error": "unauthorized"
}
```

### Full request and response contract

#### `POST /notify`

Purpose:

- receive a real review-ready notification event from the backend

Request headers:

- `Authorization: Bearer <token>` if configured
- `Content-Type: application/json`

Request body:

- `RemoteNotifierEventV1`

Success response:

```json
{
  "ok": true,
  "event": "task_review_ready",
  "action": {
    "sound_attempted": true,
    "desktop_attempted": true
  }
}
```

Failure responses:

```json
{
  "ok": false,
  "error": "unauthorized"
}
```

```json
{
  "ok": false,
  "error": "invalid_payload"
}
```

```json
{
  "ok": false,
  "error": "internal_error"
}
```

#### `GET /health`

Purpose:

- indicate that the notifier process is alive and listening

Recommended response:

```json
{
  "ok": true,
  "service": "vk-notifier",
  "version": "0.1.0"
}
```

This endpoint is for process liveness only. It does not guarantee that sound output is currently working.

#### `POST /test`

Purpose:

- explicitly trigger a local sound/desktop notification without waiting for a real task completion

Request headers:

- `Authorization: Bearer <token>` if configured
- `Content-Type: application/json`

Recommended request body:

```json
{
  "sound_enabled": true,
  "desktop_enabled": true,
  "title": "Test Notification",
  "message": "Vibe Kanban local notifier is reachable"
}
```

Recommended success response:

```json
{
  "ok": true,
  "action": {
    "sound_attempted": true,
    "desktop_attempted": true
  }
}
```

`POST /test` is operationally important because it lets the user verify:

- notifier is running
- SSH reverse tunnel is working
- auth token matches
- local sound path is functioning

## SSH Reverse Tunnel Design

### Example topology

Local computer:

- notifier listens on `127.0.0.1:43210`

SSH connection:

- `ssh -R 43110:127.0.0.1:43210 user@server`

Remote server:

- backend posts to `http://127.0.0.1:43110/notify`

Recommended additional endpoint checks:

- remote shell can run `curl http://127.0.0.1:43110/health`
- backend config test action can call `POST http://127.0.0.1:43110/test`

### Why reverse tunnel is the right fit

- the SSH client already controls the connection
- no inbound port needs to be opened on the user's computer
- the notifier remains local-only
- the server does not need to know the user's home IP address

### Connection ownership assumption

For v1, the system assumes:

- one active SSH-connected machine is the intended notification target

If the user reconnects from another machine, they can establish a new reverse tunnel and update the backend endpoint config for that session.

### Recommended SSH invocation patterns

Manual example:

```bash
ssh -N -R 43110:127.0.0.1:43210 user@server
```

Recommended production-like local workflow:

```bash
ssh \
  -o ExitOnForwardFailure=yes \
  -o ServerAliveInterval=30 \
  -o ServerAliveCountMax=3 \
  -N \
  -R 43110:127.0.0.1:43210 \
  user@server
```

Why these flags:

- `ExitOnForwardFailure=yes`
  - fail fast if reverse port setup does not work
- `ServerAliveInterval`
  - keep the connection alive
- `ServerAliveCountMax`
  - drop dead sessions instead of hanging forever

Optional later improvement:

- support `autossh` or a bundled helper command that supervises the tunnel

### Recommended remote binding mode

Prefer loopback-only remote binding if SSH server configuration supports it.

Goal:

- the forwarded remote port should be reachable only from the remote host itself

That keeps the notifier endpoint private to the remote machine and reduces accidental exposure.

### Tunnel health expectations

Healthy tunnel means:

- local notifier responds on `127.0.0.1:43210`
- remote side can reach forwarded port on `127.0.0.1:43110`
- auth token, if configured, is accepted

Unhealthy tunnel examples:

- reverse port not established
- local notifier not running
- local notifier bound to wrong interface/port
- stale SSH session
- token mismatch

## Session Binding Options

There are two possible ways to bind a notifier target.

### Option A. Global process-level target

One backend instance has one configured remote notifier URL.

Pros:

- simplest implementation
- minimal backend changes

Cons:

- poor fit if multiple users share one server
- poor fit if one user has multiple active computers

### Option B. Session-scoped target

Each SSH-connected environment registers its own notifier target for the current user session.

Pros:

- better multi-user behavior
- better mapping to "current connected computer"

Cons:

- more moving parts
- requires a registration protocol

### Recommendation

For v1, implement a process-level target first if the deployment is effectively single-user.

If multi-user support matters soon, move directly to session-scoped registration.

## Recommended Registration Model

Even if v1 starts simple, the long-term cleaner model is:

- local notifier starts
- SSH tunnel starts
- a small CLI or API call registers the remote forwarded URL with the backend
- backend stores notifier target for the current user/session

Suggested registration API later:

```http
POST /api/notifications/remote-target
{
  "url": "http://127.0.0.1:43110/notify",
  "token": "secret",
  "label": "alice-macbook"
}
```

For the immediate design phase, we do not need to commit to the registration endpoint yet, but the architecture should leave room for it.

## Backend Change Scope

### Primary backend files

Likely change areas:

- notification service
- config types and migrations
- config routes/types exposed to frontend or CLI if needed
- task completion call sites if they need extra context fields in payload

Concrete likely files:

- `crates/services/src/services/notification.rs`
- `crates/services/src/services/config/...`
- `crates/server/src/routes/config.rs`
- generated shared types if config is surfaced through the UI

### Backend responsibilities

Add a remote notifier send path that:

- builds a payload from completion context
- sends an HTTP POST to configured notifier URL
- applies timeout
- logs failures
- does not block or fail task completion on notifier delivery failure

## Local Notifier Responsibilities

The local notifier should:

- bind only to `127.0.0.1`
- validate auth token
- parse JSON payload
- play configured sound locally
- optionally show a desktop notification
- return quickly to the backend

The notifier should avoid:

- long-running synchronous playback before responding
- exposing a public network interface
- storing unnecessary task history in v1

### Minimal responsibility boundary

The local notifier should be intentionally small.

Its primary responsibilities are:

- receive authenticated notification events
- perform local output actions
- return delivery status

Its non-responsibilities in v1 should be:

- deciding which projects should notify
- deciding which task titles should notify
- maintaining complex routing rules
- becoming the primary rules engine

### Why local filtering should not be primary

A design where the server sends every task event to every notifier and the notifier filters locally is inferior as the main architecture because:

- it pushes routing complexity to every machine
- it duplicates configuration across devices
- it increases metadata exposure
- it makes debugging much harder
- it causes noisy and unnecessary network traffic

### Acceptable local secondary filtering

Very small local secondary filtering is acceptable later, but only as a safety valve.

Examples:

- local quiet hours
- local temporary mute
- local blacklist override

These should be optional secondary controls, not the primary project/title matching mechanism.

## Local Notifier Process Design

### Recommended implementation language

Use Golang for the local notifier.

Reasoning:

- easy to ship as a single static binary
- good HTTP server support in the standard library
- easy cross-platform distribution
- fits the team's prior experience with a Go-based gateway program
- simple concurrency model for sound/desktop side effects

Recommendation:

- keep the first notifier as a tiny standalone Go binary
- do not couple it to the main Rust backend codebase

### Recommended process shape

Suggested binary name:

- `vk-notifier`

Suggested internal modules:

- config loading
- HTTP server
- auth middleware
- sound output
- desktop notification output
- logging

Recommended startup behavior:

- read config or CLI flags
- bind to `127.0.0.1:<port>`
- print startup summary
- expose `/health`
- start serving requests

### Minimal command set

Recommended v1 CLI:

```text
vk-notifier serve
vk-notifier test
vk-notifier version
```

#### `vk-notifier serve`

Starts the local HTTP notifier service.

Recommended flags:

- `--listen 127.0.0.1:43210`
- `--token <secret>`
- `--sound-file <path-or-name>`
- `--desktop-enabled`
- `--log-level info|debug`

#### `vk-notifier test`

Runs a local self-test without needing the backend.

Recommended behavior:

- play sound locally
- optionally show desktop notification
- print success/failure to stdout

This is useful before even opening the SSH tunnel.

#### `vk-notifier version`

Print binary version and build info.

### Recommended request handling model

For `POST /notify`:

- parse payload
- validate token
- validate `schema_version`
- queue local output work
- return response quickly

Do not block the HTTP response on long-running sound playback if avoidable.

Recommended model:

- trigger local output asynchronously
- return 200 once the request is accepted for execution

### Recommended sound implementation behavior

The Go notifier should abstract sound playback behind a small interface:

```go
type SoundPlayer interface {
    Play(sound SoundSpec) error
}
```

Similarly for desktop notification:

```go
type DesktopNotifier interface {
    Notify(title, message string) error
}
```

This keeps OS-specific behavior isolated.

## Health Check And Test Notification Design

### `GET /health`

Purpose:

- verify that the Go notifier process is alive
- verify that SSH reverse forwarding reaches the process

It should not attempt sound playback.

Recommended use:

- manual curl on the local machine
- manual curl from the remote server via forwarded port
- automated readiness checks by a future helper script

### `POST /test`

Purpose:

- verify end-to-end notification delivery and local output

Recommended test sequence:

1. Run `vk-notifier serve` locally
2. Call `curl http://127.0.0.1:43210/health`
3. Call `POST /test` locally
4. Open SSH reverse tunnel
5. From remote server call `curl http://127.0.0.1:43110/health`
6. From remote server call `POST /test`
7. Confirm local machine plays sound

### Backend-side test action

Later, the backend may expose a configuration-level "test notifier target" action.

Recommended behavior:

- backend selects one configured target
- backend sends `POST /test` to that target's URL
- UI or CLI reports success/failure

This is better than waiting for a real task completion to validate tunnel setup.

## Implementation Work Breakdown

This section converts the design into an implementation-oriented work split across the Rust backend, the Go notifier, and configuration entry points.

## Rust Backend Scope

### 1. Config types and migration

Add the new remote notification config model to the Rust config system.

Expected work:

- introduce `v9` config
- add `remote_notifications` to top-level `Config`
- define:
  - `RemoteNotificationsConfig`
  - `RemoteNotifierTarget`
  - `RemoteNotifierProjectFilter`
- provide defaults
- migrate from `v8` by adding empty disabled remote config

Likely files:

- `crates/services/src/services/config/versions/v8.rs`
- new `crates/services/src/services/config/versions/v9.rs`
- `crates/services/src/services/config/mod.rs`

### 2. Config validation

Add validation helpers for remote notification targets.

Expected work:

- unique target `id`
- valid URL
- valid regex
- sane timeout bounds
- project list normalization/deduplication

Likely files:

- config version module where validation is implemented
- possibly shared validation helpers under `crates/services/src/services/config/`

### 3. Shared type exposure

If remote notification config is exposed to the frontend or any generated API types:

- update TS type generation inputs
- regenerate shared types

Likely files:

- `crates/server/src/bin/generate_types.rs`
- `shared/types.ts` via generation

### 4. Notification event model

Introduce a structured event type for review-ready notifications.

Expected work:

- define `ReviewReadyNotificationEvent`
- populate it from existing task completion context
- stop relying on plain `(title, message)` for remote routing

Likely files:

- `crates/services/src/services/notification.rs`
- completion call sites in container/finalization services

### 5. Notification service split

Refactor `NotificationService` into separate delivery paths.

Expected work:

- keep existing server-local notification path
- add remote notifier routing path
- add structured matching against `remote_notifications.targets`
- add HTTP POST delivery to matched targets

Likely files:

- `crates/services/src/services/notification.rs`

Suggested internal split:

- local notification helpers
- target matching helpers
- remote HTTP sender helpers
- payload builder helpers

### 6. HTTP client integration

Add remote send behavior using `reqwest`.

Expected work:

- build JSON payload
- apply bearer token if configured
- apply timeout
- treat non-2xx as failure
- log result without failing task completion

Potential dependency impact:

- ensure `reqwest` is available in the relevant crate

### 7. Completion flow wiring

Wire remote notifications into task completion flow.

Expected work:

- call `notify_review_ready(event)` at the same point current completion notifications fire
- ensure manual completion/failure paths produce consistent event data
- ensure notification failure does not block finalization

Likely files:

- `crates/services/src/services/container.rs`
- `crates/local-deployment/src/container.rs`

### 8. Backend health/test hooks

Optional for this phase, but strongly recommended:

- add backend-side “test notifier target” action later
- do not block MVP on server-side test route if local manual curl is sufficient

## Go Notifier Scope

### Recommended binary

- `vk-notifier`

### 1. Project structure

Recommended package/module split:

- `cmd/vk-notifier`
  - CLI entrypoint
- `internal/server`
  - HTTP server and routing
- `internal/auth`
  - bearer token validation
- `internal/protocol`
  - request/response structs
- `internal/sound`
  - local sound playback abstraction
- `internal/desktop`
  - desktop notification abstraction
- `internal/config`
  - CLI/config parsing
- `internal/logging`
  - log setup

This keeps the first notifier small but still maintainable.

### 2. Protocol types

Implement Go structs for:

- `RemoteNotifierEventV1`
- `/notify` response
- `/health` response
- `/test` request/response

## Post-Implementation Notes

This section records implementation findings that should inform future optimization work.

### Review-Ready Notification Scope

The current implementation routes review-ready completion notifications through `remote_notifications`.

This is correct for the remote-server workflow that motivated this feature, but it also changes prior behavior:

- users without any remote notifier target configured will no longer receive the old server-local completion notification for review-ready events
- this is acceptable for the current deployment target, but should be revisited before treating the feature as a general replacement for existing notifications

Future optimization:

- make review-ready notification strategy explicit rather than implicitly remote-only
- recommended future config shape:
  - `local_only`
  - `remote_only`
  - `both`

This would preserve backward compatibility for single-machine/local-server users while still supporting the remote SSH workflow cleanly.

### Duplicate Finalize Root Cause

During implementation, a duplicate notification bug was traced to the local container completion flow.

Root cause:

- under the "no changes made, skip cleanup script" path, the code manually called `finalize_task(...)`
- the same execution then still entered the generic `should_finalize()` branch and called `finalize_task(...)` again
- each `finalize_task(...)` call produced a fresh event timestamp
- notifier-side payload deduplication therefore did not catch the duplicate

The bug was fixed in the local container flow by marking the execution as already finalized when the manual finalize path is taken and skipping the generic finalize branch afterward.

This fix should be preserved if the completion pipeline is refactored later.

### Local Desktop Notification Reality

The `Desktop Notification` target flag only expresses what the backend asks the local notifier to do.

It does not guarantee that a local desktop notification will appear.

Additional local requirements still apply:

- the local notifier must be started with `--desktop-enabled`
- the local operating system must allow the host terminal/application to emit notifications
- on macOS specifically, the underlying `osascript display notification ...` path must work in the local user session

This means the Settings UI should eventually explain that `Desktop Notification` is a request flag, not a self-sufficient guarantee.

Recommended future UX improvement:

- show helper text near the checkbox:
  - `Requires the local vk-notifier process to be started with --desktop-enabled`

### Local Notifier Sound Throttling

The Go notifier currently has event-level deduplication for identical payloads within a short window.

This is useful, but it is narrower than the desired user-facing behavior.

Desired future behavior:

- even if multiple different review-ready events arrive within a short period
- the local notifier should emit at most one audible sound within a configurable time window

Recommended default:

- sound throttle window: `5s`

Recommended semantics:

- sound output is rate-limited globally per notifier process
- desktop notifications may still be shown individually
- if multiple tasks complete within the window, they may still all be logged and surfaced, but only one sound should play

This is separate from payload deduplication:

- payload dedupe prevents the exact same event from sounding twice
- sound throttling prevents a burst of different events from producing an unpleasant rapid series of sounds

Recommended future implementation direction in `vk-notifier`:

- keep the existing identical-event dedupe
- add a second layer of sound throttling based on last sound emission time
- do not suppress delivery logging or response success when sound is skipped due to throttling

### Optimization Backlog

The highest-value follow-up items are:

1. Add an explicit notification strategy for review-ready events instead of hard-coding remote-only behavior.
2. Add helper text in Settings explaining the local `--desktop-enabled` requirement.
3. Add notifier-level sound throttling so that at most one sound is emitted every 5 seconds, even across different tasks.

### 3. HTTP server

Expose:

- `GET /health`
- `POST /notify`
- `POST /test`

Expected behavior:

- bind to `127.0.0.1` only by default
- parse JSON
- authenticate protected routes
- return small JSON responses

### 4. Auth module

Expected work:

- parse `Authorization: Bearer ...`
- constant-time compare with configured token
- support explicit no-token local-dev mode only if needed

### 5. Sound module

Expected work:

- define cross-platform `SoundPlayer` interface
- implement best-effort playback on:
  - macOS
  - Linux desktop
  - Windows
- keep sound playback independent from HTTP request parsing

### 6. Desktop notification module

Expected work:

- define `DesktopNotifier` interface
- use local OS notification mechanisms where available
- make this optional and driven by request/config flags

### 7. CLI commands

Implement:

- `vk-notifier serve`
- `vk-notifier test`
- `vk-notifier version`

Recommended v1 flags:

- `--listen`
- `--token`
- `--desktop-enabled`
- `--sound-file`
- `--log-level`

### 8. Operational logging

Recommended logs:

- startup bind address
- request accepted/rejected
- unauthorized attempts
- sound playback attempted/failed
- desktop notification attempted/failed

## Configuration Entry Strategy

### Recommendation for this phase

Do not build full UI support in this phase.

Recommended phase boundary:

- support configuration file first
- optionally support CLI/helper registration second
- defer Settings UI until the protocol and target model are validated in real usage

### Why UI should be deferred

The remote notifier design still has operational questions:

- target ownership model
- SSH workflow ergonomics
- exact timeout defaults
- how often users will edit regex/project filters

Shipping UI too early would lock in a model before the workflow is proven.

### Phase 1 configuration entry points

Recommended initial entry points:

1. Config file editing
2. Local notifier CLI flags
3. Manual SSH command

Optional helper later:

4. A small registration CLI that writes/patches backend config or calls an API

### Config file scope for MVP

Backend config should support:

- enabling remote notifications globally
- defining one or more targets
- storing per-target auth/filtering settings

Local notifier config should support:

- listen address
- auth token
- local sound configuration
- desktop notification toggle

### UI recommendation

For this release:

- backend Settings UI: no
- frontend Settings UI: no
- local notifier GUI: no

Rationale:

- the feature is infrastructure-heavy
- manual operators can validate it faster via config and CLI
- UI can follow after the protocol and workflow are stable

## Recommended MVP Delivery Order

### Step 1

Rust backend:

- add config types
- add migration
- add validation

### Step 2

Rust backend:

- add structured notification event
- add remote target matching
- add HTTP sender

### Step 3

Go notifier:

- implement `serve`
- implement `/health`
- implement `/notify`
- implement local sound playback

### Step 4

Go notifier:

- implement `/test`
- implement `vk-notifier test`
- add optional desktop notifications

### Step 5

Operations:

- document SSH reverse tunnel startup
- document config examples
- validate end-to-end manually

### Step 6

Only after real usage:

- consider helper CLI
- consider UI support

## Explicit Recommendation

This phase should be implemented as:

- Rust backend changes for config, matching, and delivery
- standalone Go notifier binary
- config-file and CLI driven workflow
- no Settings UI in the first release

That is the smallest path that proves the architecture without spending time on UI before the transport and routing model are validated.

## Sound Behavior

The sound should be played by the local machine, not by the browser.

This means:

- macOS: local helper can call native sound tools or APIs
- Linux desktop: local helper can call local desktop sound tools
- Windows: local helper can use local sound APIs or PowerShell

Because the sound is local, it is no longer limited by browser background-tab behavior.

## Failure Handling

The backend remote notification path must be best-effort.

Failure scenarios:

- SSH tunnel is not active
- local notifier is not running
- token mismatch
- local machine sleeps or disconnects

Backend behavior:

- log warning/error
- continue normal task finalization
- never fail the task because remote notification failed

Optional later enhancements:

- surface a health indicator
- add a test-notification action
- expose last remote notifier delivery result

## Security Considerations

### Tunnel exposure

The local notifier must not listen on a public interface.

Recommended:

- local notifier listens on `127.0.0.1` only
- reverse tunnel exposes only a loopback-bound remote port if possible

### Authentication

The bearer token is required even with SSH tunneling.

Reasons:

- protects against accidental local misuse on the remote host
- avoids unauthenticated notification injection
- keeps the protocol clean if the design later grows beyond SSH-only operation

### Payload sensitivity

Do not send unnecessary sensitive task content.

For v1, avoid:

- full task description
- diffs
- logs

Send only notification metadata.

## Design Decision Summary

### Chosen direction

- backend completion detection remains the source of truth
- browser audio is not the primary path
- notifications are routed back to the local machine through SSH reverse port forwarding
- the local machine runs a small notifier service that emits sound locally

### Why

- reliable background sound
- no dependence on browser autoplay
- fits existing backend-triggered completion flow
- works naturally with SSH-based remote development

## Implementation Phases

### Phase 1. Design-only

- define notifier payload
- define local notifier API
- define config shape
- define SSH reverse tunnel usage model

### Phase 2. Minimum viable implementation

- backend remote notifier HTTP sender
- local notifier prototype
- manual SSH reverse tunnel setup
- test path for task completion -> local sound

### Phase 3. Operational improvements

- notifier health check
- test notification command
- registration helper CLI
- optional desktop notifications in addition to sound

## Open Questions

These need explicit product/engineering decisions before implementation:

1. Is the target deployment single-user or multi-user?
2. Should notifier target be global, per user, or per session?
3. Should existing `notifications.*` settings be split into:
   - server-local notifications
   - remote-local notifications
4. Do we want the local notifier to support:
   - sound only
   - sound + desktop notification
5. Do we need a companion CLI command to establish the reverse tunnel and register the endpoint automatically?

## Recommended Next Step

Before writing code, refine this design into an implementation plan for:

- the exact backend config schema
- the remote notifier HTTP client contract
- the local notifier process contract
- the SSH command and operational setup for users

The most important architectural decision to settle next is whether the notifier target is:

- a single global backend setting
- or a per-user/per-session registration target

That choice determines most of the backend state model.
