package main

import (
	"errors"
	"fmt"
	"os/exec"
	"runtime"
)

type localOutput struct {
	soundFile      string
	desktopEnabled bool
}

func (o localOutput) playSound() error {
	if o.soundFile != "" {
		return playSoundFile(o.soundFile)
	}

	switch runtime.GOOS {
	case "darwin":
		return runFirst(
			exec.Command("afplay", "/System/Library/Sounds/Glass.aiff"),
			exec.Command("afplay", "/System/Library/Sounds/Ping.aiff"),
		)
	case "linux":
		return runFirst(
			exec.Command("paplay", "/usr/share/sounds/freedesktop/stereo/complete.oga"),
			exec.Command("aplay", "/usr/share/sounds/alsa/Front_Center.wav"),
			exec.Command("sh", "-c", "printf '\\a'"),
		)
	case "windows":
		return exec.Command("powershell.exe", "-c", "[console]::beep(1000,300)").Run()
	default:
		return errors.New("unsupported OS for sound playback")
	}
}

func playSoundFile(soundFile string) error {
	switch runtime.GOOS {
	case "darwin":
		return exec.Command("afplay", soundFile).Run()
	case "linux":
		return runFirst(
			exec.Command("paplay", soundFile),
			exec.Command("aplay", soundFile),
		)
	case "windows":
		cmd := fmt.Sprintf(`(New-Object Media.SoundPlayer "%s").PlaySync()`, soundFile)
		return exec.Command("powershell.exe", "-c", cmd).Run()
	default:
		return errors.New("unsupported OS for sound playback")
	}
}

func (o localOutput) sendDesktopNotification(title, message string) error {
	if !o.desktopEnabled {
		return nil
	}

	switch runtime.GOOS {
	case "darwin":
		script := fmt.Sprintf(`display notification "%s" with title "%s"`,
			escapeAppleScript(message),
			escapeAppleScript(title),
		)
		return exec.Command("osascript", "-e", script).Run()
	case "linux":
		return exec.Command("notify-send", title, message).Run()
	case "windows":
		cmd := fmt.Sprintf(`[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] > $null; $template = "<toast><visual><binding template='ToastText02'><text id='1'>%s</text><text id='2'>%s</text></binding></visual></toast>"; $xml = New-Object Windows.Data.Xml.Dom.XmlDocument; $xml.LoadXml($template); $toast = [Windows.UI.Notifications.ToastNotification]::new($xml); $notifier = [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('vk-notifier'); $notifier.Show($toast)`, title, message)
		return exec.Command("powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-c", cmd).Run()
	default:
		return errors.New("unsupported OS for desktop notifications")
	}
}

func runFirst(commands ...*exec.Cmd) error {
	var lastErr error
	for _, cmd := range commands {
		if err := cmd.Run(); err == nil {
			return nil
		} else {
			lastErr = err
		}
	}
	if lastErr == nil {
		return errors.New("no commands were provided")
	}
	return lastErr
}

func escapeAppleScript(value string) string {
	escaped := make([]rune, 0, len(value))
	for _, r := range value {
		if r == '"' || r == '\\' {
			escaped = append(escaped, '\\')
		}
		escaped = append(escaped, r)
	}
	return string(escaped)
}
