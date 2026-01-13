package main

import (
	"crypto/subtle"
	"log"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"strings"
	"time"
)

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

	h := basicAuth(username, password, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/api/") {
			backendProxy.ServeHTTP(w, r)
			return
		}
		frontendProxy.ServeHTTP(w, r)
	}))

	srv := &http.Server{
		Addr:              listenAddr,
		Handler:           h,
		ReadHeaderTimeout: 5 * time.Second,
	}

	log.Printf("Basic auth proxy listening on %s", listenAddr)
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

func basicAuth(user, pass string, next http.Handler) http.Handler {
	realm := `Basic realm="KBoard"`
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		u, p, ok := r.BasicAuth()
		if !ok || !secureEqual(u, user) || !secureEqual(p, pass) {
			w.Header().Set("WWW-Authenticate", realm)
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte("unauthorized"))
			return
		}
		next.ServeHTTP(w, r)
	})
}

func secureEqual(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1
}

func envOrDefault(key, fallback string) string {
	if val := strings.TrimSpace(os.Getenv(key)); val != "" {
		return val
	}
	return fallback
}
