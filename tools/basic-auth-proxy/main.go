package main

import (
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"html"
	"log"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"strings"
	"time"
)

const authCookieName = "kb_auth"

func main() {
	listenAddr := envOrDefault("PROXY_LISTEN_ADDR", ":3002")
	frontendURL := envOrDefault("FRONTEND_UPSTREAM", "http://127.0.0.1:3032")
	backendURL := envOrDefault("BACKEND_UPSTREAM", "http://127.0.0.1:3033")
	username := os.Getenv("BASIC_AUTH_USER")
	password := os.Getenv("BASIC_AUTH_PASS")

	if username == "" || password == "" {
		log.Fatal("BASIC_AUTH_USER and BASIC_AUTH_PASS must be set")
	}

	frontendProxy := mustProxy(frontendURL)
	backendProxy := mustProxy(backendURL)
	token := authToken(username, password)

	h := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/gateway/") {
			handleGateway(w, r, username, password, token)
			return
		}

		if !hasAuthCookie(r, token) {
			if r.URL.Path == "/" || r.URL.Path == "/index.html" {
				http.Redirect(w, r, "/gateway/login/", http.StatusFound)
				return
			}
			http.Error(w, "unauthorized", http.StatusUnauthorized)
			return
		}

		if strings.HasPrefix(r.URL.Path, "/api/") {
			backendProxy.ServeHTTP(w, r)
			return
		}
		frontendProxy.ServeHTTP(w, r)
	})

	srv := &http.Server{
		Addr:              listenAddr,
		Handler:           h,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       15 * time.Second,
		WriteTimeout:      15 * time.Second,
		IdleTimeout:       5 * time.Minute,
	}

	log.Printf("Gateway auth proxy listening on %s", listenAddr)
	log.Printf("Frontend upstream: %s", frontendURL)
	log.Printf("Backend upstream: %s", backendURL)
	log.Fatal(srv.ListenAndServe())
}

func mustProxy(rawURL string) *httputil.ReverseProxy {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		log.Fatalf("Invalid upstream URL %q: %v", rawURL, err)
	}

	proxy := httputil.NewSingleHostReverseProxy(parsed)
	origDirector := proxy.Director
	proxy.Director = func(req *http.Request) {
		origDirector(req)
		// Preserve the original Host header for upstream routing if needed.
		req.Host = parsed.Host
	}

	proxy.ErrorHandler = func(w http.ResponseWriter, r *http.Request, err error) {
		log.Printf("proxy error: %v", err)
		w.WriteHeader(http.StatusBadGateway)
		_, _ = w.Write([]byte("bad gateway"))
	}

	return proxy
}

func secureEqual(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1
}

func hasAuthCookie(r *http.Request, token string) bool {
	cookie, err := r.Cookie(authCookieName)
	if err != nil {
		return false
	}
	return secureEqual(cookie.Value, token)
}

func handleGateway(
	w http.ResponseWriter,
	r *http.Request,
	user string,
	pass string,
	token string,
) {
	switch r.URL.Path {
	case "/gateway/login/", "/gateway/login":
		if r.Method == http.MethodGet {
			renderLogin(w, r)
			return
		}
		if r.Method == http.MethodPost {
			if err := r.ParseForm(); err != nil {
				http.Error(w, "invalid form", http.StatusBadRequest)
				return
			}
			inputUser := r.FormValue("username")
			inputPass := r.FormValue("password")
			if !secureEqual(inputUser, user) || !secureEqual(inputPass, pass) {
				renderLogin(w, r, "Invalid credentials")
				return
			}
			http.SetCookie(w, &http.Cookie{
				Name:     authCookieName,
				Value:    token,
				Path:     "/",
				HttpOnly: true,
				SameSite: http.SameSiteLaxMode,
			})
			next := r.FormValue("next")
			if next == "" {
				next = "/"
			}
			http.Redirect(w, r, next, http.StatusFound)
			return
		}
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	case "/gateway/logout/", "/gateway/logout":
		http.SetCookie(w, &http.Cookie{
			Name:     authCookieName,
			Value:    "",
			Path:     "/",
			MaxAge:   -1,
			HttpOnly: true,
			SameSite: http.SameSiteLaxMode,
		})
		http.Redirect(w, r, "/gateway/login/", http.StatusFound)
		return
	default:
		http.NotFound(w, r)
	}
}

func renderLogin(w http.ResponseWriter, r *http.Request, message ...string) {
	next := "/"
	if r.Method == http.MethodGet {
		if qp := r.URL.Query().Get("next"); qp != "" {
			next = qp
		}
	}
	msg := ""
	if len(message) > 0 {
		msg = message[0]
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	_, _ = w.Write([]byte(`<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>KBoard Login</title>
  <style>
    body { font-family: system-ui, -apple-system, Segoe UI, sans-serif; background: #f4f5f7; }
    .card { max-width: 420px; margin: 12vh auto; padding: 28px; background: #fff; border-radius: 12px; box-shadow: 0 10px 30px rgba(0,0,0,0.08); }
    h1 { margin: 0 0 16px; font-size: 20px; }
    label { display: block; margin: 12px 0 6px; font-size: 14px; color: #444; }
    input { width: 100%; padding: 10px 12px; border-radius: 8px; border: 1px solid #d0d5dd; font-size: 14px; }
    button { margin-top: 18px; width: 100%; padding: 10px 12px; border-radius: 8px; border: 0; background: #1f2937; color: #fff; font-size: 15px; cursor: pointer; }
    .msg { color: #b91c1c; margin-bottom: 12px; }
  </style>
</head>
<body>
  <div class="card">
    <h1>Sign in</h1>`))
	if msg != "" {
		_, _ = w.Write([]byte(`<div class="msg">` + html.EscapeString(msg) + `</div>`))
	}
	_, _ = w.Write([]byte(`
    <form method="post" action="/gateway/login/">
      <input type="hidden" name="next" value="` + html.EscapeString(next) + `">
      <label for="username">Username</label>
      <input id="username" name="username" type="text" autocomplete="username" required>
      <label for="password">Password</label>
      <input id="password" name="password" type="password" autocomplete="current-password" required>
      <button type="submit">Login</button>
    </form>
  </div>
</body>
</html>`))
}

func authToken(user, pass string) string {
	sum := sha256.Sum256([]byte(user + ":" + pass))
	return hex.EncodeToString(sum[:])
}

func envOrDefault(key, fallback string) string {
	if val := strings.TrimSpace(os.Getenv(key)); val != "" {
		return val
	}
	return fallback
}
