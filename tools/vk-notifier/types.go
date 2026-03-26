package main

type RemoteNotifierEventV1 struct {
	SchemaVersion string                 `json:"schema_version"`
	Event         string                 `json:"event"`
	TaskID        string                 `json:"task_id"`
	TaskTitle     string                 `json:"task_title"`
	ProjectID     string                 `json:"project_id"`
	ProjectName   *string                `json:"project_name,omitempty"`
	WorkspaceID   string                 `json:"workspace_id"`
	SessionID     string                 `json:"session_id"`
	Status        string                 `json:"status"`
	Branch        *string                `json:"branch,omitempty"`
	Executor      *string                `json:"executor,omitempty"`
	CompletedAt   string                 `json:"completed_at"`
	Delivery      RemoteNotifierDelivery `json:"delivery"`
}

type RemoteNotifierDelivery struct {
	TargetID       string `json:"target_id"`
	SoundEnabled   bool   `json:"sound_enabled"`
	DesktopEnabled bool   `json:"desktop_enabled"`
}

type TestRequest struct {
	SoundEnabled   bool   `json:"sound_enabled"`
	DesktopEnabled bool   `json:"desktop_enabled"`
	Title          string `json:"title"`
	Message        string `json:"message"`
}

type ActionResponse struct {
	SoundAttempted   bool `json:"sound_attempted"`
	DesktopAttempted bool `json:"desktop_attempted"`
}

type NotifyResponse struct {
	OK     bool            `json:"ok"`
	Event  string          `json:"event,omitempty"`
	Action *ActionResponse `json:"action,omitempty"`
	Error  string          `json:"error,omitempty"`
}

type HealthResponse struct {
	OK      bool   `json:"ok"`
	Service string `json:"service"`
	Version string `json:"version"`
}
