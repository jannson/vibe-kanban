package main

import (
	"crypto/subtle"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"sync"
	"time"
)

type server struct {
	token        string
	output       localOutput
	logger       *log.Logger
	recentMu     sync.Mutex
	recent       map[string]time.Time
	dedupeWindow time.Duration
	soundMu      sync.Mutex
	lastSoundAt  time.Time
	soundWindow  time.Duration
}

func newServer(token string, output localOutput, logger *log.Logger) *server {
	return &server{
		token:        token,
		output:       output,
		logger:       logger,
		recent:       make(map[string]time.Time),
		dedupeWindow: 5 * time.Second,
		soundWindow:  5 * time.Second,
	}
}

func (s *server) routes(version string) http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, http.StatusOK, HealthResponse{
			OK:      true,
			Service: "vk-notifier",
			Version: version,
		})
	})
	mux.HandleFunc("POST /notify", s.requireAuth(s.handleNotify))
	mux.HandleFunc("POST /test", s.requireAuth(s.handleTest))
	return mux
}

func (s *server) requireAuth(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if s.token == "" {
			next(w, r)
			return
		}

		token := bearerToken(r.Header.Get("Authorization"))
		if token == "" || subtle.ConstantTimeCompare([]byte(token), []byte(s.token)) != 1 {
			writeJSON(w, http.StatusUnauthorized, NotifyResponse{
				OK:    false,
				Error: "unauthorized",
			})
			return
		}

		next(w, r)
	}
}

func (s *server) handleNotify(w http.ResponseWriter, r *http.Request) {
	var event RemoteNotifierEventV1
	if err := json.NewDecoder(r.Body).Decode(&event); err != nil {
		writeJSON(w, http.StatusBadRequest, NotifyResponse{
			OK:    false,
			Error: "invalid_payload",
		})
		return
	}

	if err := validateRemoteEvent(event); err != nil {
		writeJSON(w, http.StatusBadRequest, NotifyResponse{
			OK:    false,
			Error: "invalid_payload",
		})
		return
	}

	if !s.shouldDispatch(event) {
		writeJSON(w, http.StatusOK, NotifyResponse{
			OK:    true,
			Event: event.Event,
			Action: &ActionResponse{
				SoundAttempted:   false,
				DesktopAttempted: false,
			},
		})
		return
	}

	actions := s.dispatch(event.TaskTitle, notifierMessage(event), event.Delivery.SoundEnabled, event.Delivery.DesktopEnabled)
	writeJSON(w, http.StatusOK, NotifyResponse{
		OK:    true,
		Event: event.Event,
		Action: &ActionResponse{
			SoundAttempted:   actions.SoundAttempted,
			DesktopAttempted: actions.DesktopAttempted,
		},
	})
}

func (s *server) handleTest(w http.ResponseWriter, r *http.Request) {
	var req TestRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeJSON(w, http.StatusBadRequest, NotifyResponse{
			OK:    false,
			Error: "invalid_payload",
		})
		return
	}

	title := req.Title
	if title == "" {
		title = "Test Notification"
	}
	message := req.Message
	if message == "" {
		message = "Vibe Kanban local notifier is reachable"
	}

	actions := s.dispatch(title, message, req.SoundEnabled, req.DesktopEnabled)
	writeJSON(w, http.StatusOK, NotifyResponse{
		OK: true,
		Action: &ActionResponse{
			SoundAttempted:   actions.SoundAttempted,
			DesktopAttempted: actions.DesktopAttempted,
		},
	})
}

func (s *server) dispatch(title, message string, soundEnabled, desktopEnabled bool) ActionResponse {
	var resp ActionResponse

	if soundEnabled && s.shouldPlaySound() {
		resp.SoundAttempted = true
		if err := s.output.playSound(); err != nil {
			s.logger.Printf("sound playback failed: %v", err)
		}
	} else if soundEnabled {
		s.logger.Printf("sound throttled")
	}

	if desktopEnabled && s.output.desktopEnabled {
		resp.DesktopAttempted = true
		if err := s.output.sendDesktopNotification(title, message); err != nil {
			s.logger.Printf("desktop notification failed: %v", err)
		}
	}

	return resp
}

func (s *server) shouldDispatch(event RemoteNotifierEventV1) bool {
	key := event.SchemaVersion + "|" + event.Event + "|" + event.TaskID + "|" + event.SessionID + "|" + event.Status + "|" + event.CompletedAt + "|" + event.Delivery.TargetID
	now := time.Now()

	s.recentMu.Lock()
	defer s.recentMu.Unlock()

	for existingKey, seenAt := range s.recent {
		if now.Sub(seenAt) > s.dedupeWindow {
			delete(s.recent, existingKey)
		}
	}

	if seenAt, ok := s.recent[key]; ok && now.Sub(seenAt) <= s.dedupeWindow {
		s.logger.Printf("deduplicated remote event key=%s", key)
		return false
	}

	s.recent[key] = now
	return true
}

func (s *server) shouldPlaySound() bool {
	now := time.Now()

	s.soundMu.Lock()
	defer s.soundMu.Unlock()

	if !s.lastSoundAt.IsZero() && now.Sub(s.lastSoundAt) <= s.soundWindow {
		return false
	}

	s.lastSoundAt = now
	return true
}

func validateRemoteEvent(event RemoteNotifierEventV1) error {
	if event.SchemaVersion != "v1" {
		return errors.New("unsupported schema version")
	}
	if event.Event == "" || event.TaskID == "" || event.TaskTitle == "" || event.Status == "" {
		return errors.New("missing required fields")
	}
	return nil
}

func notifierMessage(event RemoteNotifierEventV1) string {
	message := "Task is ready for review"
	if event.ProjectName != nil && *event.ProjectName != "" {
		message = *event.ProjectName + ": " + message
	}
	if event.Branch != nil && *event.Branch != "" {
		message += " (" + *event.Branch + ")"
	}
	return message
}

func bearerToken(header string) string {
	const prefix = "Bearer "
	if len(header) <= len(prefix) || header[:len(prefix)] != prefix {
		return ""
	}
	return header[len(prefix):]
}

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}
