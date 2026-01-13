# Basic Auth Proxy (Dev)

A minimal reverse proxy with HTTP Basic Auth. It forwards `/api/*` to the backend
and everything else to the frontend.

## Environment

- `PROXY_LISTEN_ADDR` (default `:3002`)
- `FRONTEND_UPSTREAM` (default `http://127.0.0.1:3032`)
- `BACKEND_UPSTREAM` (default `http://127.0.0.1:3033`)
- `BASIC_AUTH_USER` (required)
- `BASIC_AUTH_PASS` (required)

## Run

```
BASIC_AUTH_USER=admin BASIC_AUTH_PASS=admin \
  go run .
```
