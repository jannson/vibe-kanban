package main

import (
	"bytes"
	"encoding/json"
	"io"
	"log"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func testServer() *server {
	return newServer("secret", localOutput{}, log.New(io.Discard, "", 0))
}

func TestHealthEndpoint(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/health", nil)
	rec := httptest.NewRecorder()

	testServer().routes("0.1.0").ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
}

func TestNotifyRequiresAuth(t *testing.T) {
	req := httptest.NewRequest(http.MethodPost, "/notify", bytes.NewReader([]byte(`{}`)))
	rec := httptest.NewRecorder()

	testServer().routes("0.1.0").ServeHTTP(rec, req)

	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
}

func TestNotifyAcceptsValidPayload(t *testing.T) {
	payload, err := json.Marshal(RemoteNotifierEventV1{
		SchemaVersion: "v1",
		Event:         "task_review_ready",
		TaskID:        "task-1",
		TaskTitle:     "urgent: fix prod",
		ProjectID:     "project-1",
		WorkspaceID:   "workspace-1",
		SessionID:     "session-1",
		Status:        "inreview",
		Delivery: RemoteNotifierDelivery{
			TargetID:       "target-1",
			SoundEnabled:   false,
			DesktopEnabled: false,
		},
	})
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}

	req := httptest.NewRequest(http.MethodPost, "/notify", bytes.NewReader(payload))
	req.Header.Set("Authorization", "Bearer secret")
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()

	testServer().routes("0.1.0").ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
}

func TestTestEndpointRequiresAuth(t *testing.T) {
	req := httptest.NewRequest(http.MethodPost, "/test", bytes.NewReader([]byte(`{}`)))
	rec := httptest.NewRecorder()

	testServer().routes("0.1.0").ServeHTTP(rec, req)

	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
}

func TestShouldDispatchDeduplicatesIdenticalEvents(t *testing.T) {
	srv := testServer()
	event := RemoteNotifierEventV1{
		SchemaVersion: "v1",
		Event:         "task_review_ready",
		TaskID:        "task-1",
		TaskTitle:     "urgent: fix prod",
		ProjectID:     "project-1",
		WorkspaceID:   "workspace-1",
		SessionID:     "session-1",
		Status:        "inreview",
		CompletedAt:   "2026-03-26T15:00:00Z",
		Delivery: RemoteNotifierDelivery{
			TargetID:       "target-1",
			SoundEnabled:   true,
			DesktopEnabled: false,
		},
	}

	if !srv.shouldDispatch(event) {
		t.Fatal("expected first event to dispatch")
	}

	if srv.shouldDispatch(event) {
		t.Fatal("expected duplicate event to be deduplicated")
	}
}

func TestShouldPlaySoundThrottlesBurstEvents(t *testing.T) {
	srv := testServer()
	srv.soundWindow = 5 * time.Second

	if !srv.shouldPlaySound() {
		t.Fatal("expected first sound to play")
	}

	if srv.shouldPlaySound() {
		t.Fatal("expected second sound within window to be throttled")
	}

	srv.lastSoundAt = time.Now().Add(-6 * time.Second)
	if !srv.shouldPlaySound() {
		t.Fatal("expected sound after window to play again")
	}
}
