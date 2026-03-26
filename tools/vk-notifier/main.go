package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
)

const version = "0.1.0"

func main() {
	logger := log.New(os.Stderr, "vk-notifier: ", log.LstdFlags)

	if len(os.Args) < 2 {
		usage()
		os.Exit(2)
	}

	switch os.Args[1] {
	case "serve":
		os.Exit(runServe(logger, os.Args[2:]))
	case "test":
		os.Exit(runTest(logger, os.Args[2:]))
	case "version":
		fmt.Println(version)
		os.Exit(0)
	default:
		usage()
		os.Exit(2)
	}
}

func runServe(logger *log.Logger, args []string) int {
	fs := flag.NewFlagSet("serve", flag.ContinueOnError)
	listen := fs.String("listen", "127.0.0.1:43210", "listen address")
	token := fs.String("token", "", "bearer token")
	soundFile := fs.String("sound-file", "", "sound file path")
	desktopEnabled := fs.Bool("desktop-enabled", false, "enable desktop notifications")
	if err := fs.Parse(args); err != nil {
		return 2
	}

	srv := newServer(*token, localOutput{
		soundFile:      *soundFile,
		desktopEnabled: *desktopEnabled,
	}, logger)

	logger.Printf("serving on %s", *listen)
	if err := http.ListenAndServe(*listen, srv.routes(version)); err != nil {
		logger.Printf("server exited: %v", err)
		return 1
	}
	return 0
}

func runTest(logger *log.Logger, args []string) int {
	fs := flag.NewFlagSet("test", flag.ContinueOnError)
	token := fs.String("token", "", "bearer token")
	url := fs.String("url", "", "remote notifier url; if empty, run local output test")
	soundFile := fs.String("sound-file", "", "sound file path")
	desktopEnabled := fs.Bool("desktop-enabled", false, "enable desktop notifications")
	if err := fs.Parse(args); err != nil {
		return 2
	}

	if *url == "" {
		output := localOutput{
			soundFile:      *soundFile,
			desktopEnabled: *desktopEnabled,
		}
		if err := output.playSound(); err != nil {
			logger.Printf("local sound test failed: %v", err)
			return 1
		}
		if *desktopEnabled {
			if err := output.sendDesktopNotification("Test Notification", "vk-notifier local test"); err != nil {
				logger.Printf("local desktop test failed: %v", err)
				return 1
			}
		}
		logger.Printf("local test completed")
		return 0
	}

	body, _ := json.Marshal(TestRequest{
		SoundEnabled:   true,
		DesktopEnabled: *desktopEnabled,
		Title:          "Test Notification",
		Message:        "Vibe Kanban local notifier is reachable",
	})
	req, err := http.NewRequest(http.MethodPost, *url, bytes.NewReader(body))
	if err != nil {
		logger.Printf("failed to build request: %v", err)
		return 1
	}
	req.Header.Set("Content-Type", "application/json")
	if *token != "" {
		req.Header.Set("Authorization", "Bearer "+*token)
	}

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		logger.Printf("remote test failed: %v", err)
		return 1
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		logger.Printf("remote test failed: status=%d body=%s", resp.StatusCode, string(raw))
		return 1
	}
	logger.Printf("remote test succeeded: %s", string(raw))
	return 0
}

func usage() {
	fmt.Fprintf(os.Stderr, `vk-notifier commands:
  serve   Run local notifier HTTP server
  test    Trigger local or remote test notification
  version Print version
`)
}
