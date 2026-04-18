package main

import "testing"

func TestParseServeConfigIncludesVolume(t *testing.T) {
	cfg, err := parseServeConfig([]string{
		"--listen", "127.0.0.1:9999",
		"--token", "secret",
		"--sound-file", "/tmp/test.wav",
		"--desktop-enabled",
		"--volume", "2.5",
	})
	if err != nil {
		t.Fatalf("parse serve config: %v", err)
	}

	if cfg.listen != "127.0.0.1:9999" {
		t.Fatalf("expected listen to be preserved, got %q", cfg.listen)
	}
	if cfg.token != "secret" {
		t.Fatalf("expected token to be preserved, got %q", cfg.token)
	}
	if cfg.soundFile != "/tmp/test.wav" {
		t.Fatalf("expected sound file to be preserved, got %q", cfg.soundFile)
	}
	if !cfg.desktopEnabled {
		t.Fatal("expected desktop notifications to be enabled")
	}
	if cfg.volume != 2.5 {
		t.Fatalf("expected volume 2.5, got %v", cfg.volume)
	}
}

func TestParseServeConfigRejectsNonPositiveVolume(t *testing.T) {
	if _, err := parseServeConfig([]string{"--volume", "0"}); err == nil {
		t.Fatal("expected zero volume to be rejected")
	}
}
