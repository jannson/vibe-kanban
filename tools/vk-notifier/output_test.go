package main

import "testing"

func TestDarwinSoundCommandArgsIncludeVolumeForDefaultSound(t *testing.T) {
	args := darwinSoundCommandArgs("", 2.5)

	if len(args) != 2 {
		t.Fatalf("expected fallback commands, got %d", len(args))
	}

	first := args[0]
	if len(first) != 4 {
		t.Fatalf("expected afplay args with volume and file, got %v", first)
	}
	if first[0] != "afplay" || first[1] != "-v" || first[2] != "2.5" {
		t.Fatalf("expected volume flag in first command, got %v", first)
	}
	if first[3] != "/System/Library/Sounds/Glass.aiff" {
		t.Fatalf("expected default Glass sound, got %q", first[3])
	}
}

func TestDarwinSoundCommandArgsIncludeVolumeForCustomSound(t *testing.T) {
	args := darwinSoundCommandArgs("/tmp/custom.wav", 1.75)

	if len(args) != 1 {
		t.Fatalf("expected single custom command, got %d", len(args))
	}

	first := args[0]
	if len(first) != 4 {
		t.Fatalf("expected afplay args with volume and file, got %v", first)
	}
	if first[0] != "afplay" || first[1] != "-v" || first[2] != "1.75" {
		t.Fatalf("expected volume flag in custom command, got %v", first)
	}
	if first[3] != "/tmp/custom.wav" {
		t.Fatalf("expected custom sound file, got %q", first[3])
	}
}
