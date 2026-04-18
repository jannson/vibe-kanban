#!/bin/bash

curl -sS -X POST http://127.0.0.1:43110/notify     -H 'Authorization: Bearer 123456'     -H 'Content-Type: application/json'     -d '{
      "schema_version": "v1",
      "event": "task_review_ready",
      "task_id": "task-desktop-test-1",
      "task_title": "desktop notification test",
      "project_id": "project-desktop-test",
      "project_name": "Desktop Test Project",
      "workspace_id": "workspace-desktop-test",
      "session_id": "session-desktop-test",
      "status": "inreview",
      "branch": "vk/desktop-test",
      "executor": "codex",
      "completed_at": "2026-03-26T15:30:00Z",
      "delivery": {
        "target_id": "remote-target-1774535469014-t9ku9dpq",
        "sound_enabled": true,
        "desktop_enabled": false
      }
    }'
