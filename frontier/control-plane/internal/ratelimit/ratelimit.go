package ratelimit

import (
	"net"
	"net/http"
	"strconv"
	"sync"
	"time"
)

// entry tracks request count for a single time window.
type entry struct {
	count    int
	windowAt int64 // unix seconds when this window started
}

// RateLimiter implements a per-IP fixed-window rate limiter.
type RateLimiter struct {
	mu       sync.Mutex
	clients  map[string]*entry
	limit    int
	window   time.Duration
	interval int64
}

// New creates a RateLimiter allowing up to limit requests per window duration.
func New(limit int, window time.Duration) *RateLimiter {
	return &RateLimiter{
		clients:  make(map[string]*entry),
		limit:    limit,
		window:   window,
		interval: int64(window.Seconds()),
	}
}

// Middleware returns an http.Handler that wraps the next handler with rate limiting.
func (rl *RateLimiter) Middleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		ip := extractIP(r)
		now := time.Now().Unix()

		rl.mu.Lock()
		e, ok := rl.clients[ip]
		if !ok || now-e.windowAt >= rl.interval {
			rl.clients[ip] = &entry{count: 1, windowAt: now}
			rl.mu.Unlock()
			next.ServeHTTP(w, r)
			return
		}

		e.count++
		if e.count > rl.limit {
			rl.mu.Unlock()
			retryAfter := int(rl.window.Seconds())
			w.Header().Set("Retry-After", strconv.Itoa(retryAfter))
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusTooManyRequests)
			w.Write([]byte(`{"error":"rate limit exceeded"}`))
			return
		}
		rl.mu.Unlock()
		next.ServeHTTP(w, r)
	})
}

// extractIP extracts the client IP from the request, respecting X-Forwarded-For.
func extractIP(r *http.Request) string {
	if fwd := r.Header.Get("X-Forwarded-For"); fwd != "" {
		if host, _, err := net.SplitHostPort(fwd); err == nil {
			return host
		}
		return fwd
	}
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}