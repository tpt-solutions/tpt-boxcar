package rest

import (
	"crypto/subtle"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"strings"

	"github.com/tpt-cloud-native/frontier/control-plane/internal/store"
)

// Handler provides REST CRUD endpoints for Frontier config using Go 1.22 ServeMux.
type Handler struct {
	store *store.Store
}

// NewHandler creates a new REST API handler.
func NewHandler(st *store.Store) *Handler {
	return &Handler{store: st}
}

// apiKeyMiddleware returns 401 if the X-API-Key header is missing or doesn't
// match the FRONTIER_API_KEY environment variable (constant-time compare).
func apiKeyMiddleware(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		expected := os.Getenv("FRONTIER_API_KEY")
		if expected == "" {
			// No key configured — allow through (dev mode).
			next(w, r)
			return
		}
		got := r.Header.Get("X-API-Key")
		if subtle.ConstantTimeCompare([]byte(got), []byte(expected)) != 1 {
			http.Error(w, "unauthorized: invalid or missing API key", http.StatusUnauthorized)
			return
		}
		next(w, r)
	}
}

// RegisterRoutes registers all REST routes on the given ServeMux.
func (h *Handler) RegisterRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /api/v1/routes", apiKeyMiddleware(h.listRoutes))
	mux.HandleFunc("GET /api/v1/routes/{name}", apiKeyMiddleware(h.getRoute))
	mux.HandleFunc("POST /api/v1/routes", apiKeyMiddleware(h.createRoute))
	mux.HandleFunc("PUT /api/v1/routes/{name}", apiKeyMiddleware(h.updateRoute))
	mux.HandleFunc("DELETE /api/v1/routes/{name}", apiKeyMiddleware(h.deleteRoute))

	mux.HandleFunc("GET /api/v1/upstreams", apiKeyMiddleware(h.listUpstreams))
	mux.HandleFunc("GET /api/v1/upstreams/{name}", apiKeyMiddleware(h.getUpstream))
	mux.HandleFunc("POST /api/v1/upstreams", apiKeyMiddleware(h.createUpstream))
	mux.HandleFunc("PUT /api/v1/upstreams/{name}", apiKeyMiddleware(h.updateUpstream))
	mux.HandleFunc("DELETE /api/v1/upstreams/{name}", apiKeyMiddleware(h.deleteUpstream))

	mux.HandleFunc("GET /api/v1/plugins", apiKeyMiddleware(h.listPlugins))
	mux.HandleFunc("GET /api/v1/plugins/{name}", apiKeyMiddleware(h.getPlugin))
	mux.HandleFunc("POST /api/v1/plugins", apiKeyMiddleware(h.createPlugin))
	mux.HandleFunc("PUT /api/v1/plugins/{name}", apiKeyMiddleware(h.updatePlugin))
	mux.HandleFunc("DELETE /api/v1/plugins/{name}", apiKeyMiddleware(h.deletePlugin))

	mux.HandleFunc("GET /api/v1/tls-certs", apiKeyMiddleware(h.listTlsCerts))
	mux.HandleFunc("GET /api/v1/tls-certs/{name}", apiKeyMiddleware(h.getTlsCert))
	mux.HandleFunc("POST /api/v1/tls-certs", apiKeyMiddleware(h.createTlsCert))
	mux.HandleFunc("PUT /api/v1/tls-certs/{name}", apiKeyMiddleware(h.updateTlsCert))
	mux.HandleFunc("DELETE /api/v1/tls-certs/{name}", apiKeyMiddleware(h.deleteTlsCert))

	mux.HandleFunc("GET /health", h.health)
	mux.HandleFunc("GET /api/v1/version", apiKeyMiddleware(h.version))
}

// --- Routes ---

func (h *Handler) listRoutes(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, h.store.ListRoutes())
}

func (h *Handler) getRoute(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	route, ok := h.store.GetRoute(name)
	if !ok {
		http.Error(w, fmt.Sprintf("route %q not found", name), http.StatusNotFound)
		return
	}
	writeJSON(w, http.StatusOK, route)
}

func (h *Handler) createRoute(w http.ResponseWriter, r *http.Request) {
	var route store.Route
	if err := decodeBody(r, &route); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	if route.Name == "" {
		http.Error(w, "route name is required", http.StatusBadRequest)
		return
	}
	h.store.SetRoute(&route)
	writeJSON(w, http.StatusCreated, route)
}

func (h *Handler) updateRoute(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	var route store.Route
	if err := decodeBody(r, &route); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	route.Name = name
	if _, ok := h.store.GetRoute(name); !ok {
		http.Error(w, fmt.Sprintf("route %q not found", name), http.StatusNotFound)
		return
	}
	h.store.SetRoute(&route)
	writeJSON(w, http.StatusOK, route)
}

func (h *Handler) deleteRoute(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	if !h.store.DeleteRoute(name) {
		http.Error(w, fmt.Sprintf("route %q not found", name), http.StatusNotFound)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// --- Upstreams ---

func (h *Handler) listUpstreams(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, h.store.ListUpstreams())
}

func (h *Handler) getUpstream(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	upstream, ok := h.store.GetUpstream(name)
	if !ok {
		http.Error(w, fmt.Sprintf("upstream %q not found", name), http.StatusNotFound)
		return
	}
	writeJSON(w, http.StatusOK, upstream)
}

func (h *Handler) createUpstream(w http.ResponseWriter, r *http.Request) {
	var upstream store.Upstream
	if err := decodeBody(r, &upstream); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	if upstream.Name == "" {
		http.Error(w, "upstream name is required", http.StatusBadRequest)
		return
	}
	h.store.SetUpstream(&upstream)
	writeJSON(w, http.StatusCreated, upstream)
}

func (h *Handler) updateUpstream(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	var upstream store.Upstream
	if err := decodeBody(r, &upstream); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	upstream.Name = name
	if _, ok := h.store.GetUpstream(name); !ok {
		http.Error(w, fmt.Sprintf("upstream %q not found", name), http.StatusNotFound)
		return
	}
	h.store.SetUpstream(&upstream)
	writeJSON(w, http.StatusOK, upstream)
}

func (h *Handler) deleteUpstream(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	if !h.store.DeleteUpstream(name) {
		http.Error(w, fmt.Sprintf("upstream %q not found", name), http.StatusNotFound)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// --- Plugins ---

func (h *Handler) listPlugins(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, h.store.ListPlugins())
}

func (h *Handler) getPlugin(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	plugin, ok := h.store.GetPlugin(name)
	if !ok {
		http.Error(w, fmt.Sprintf("plugin %q not found", name), http.StatusNotFound)
		return
	}
	writeJSON(w, http.StatusOK, plugin)
}

func (h *Handler) createPlugin(w http.ResponseWriter, r *http.Request) {
	var plugin store.Plugin
	if err := decodeBody(r, &plugin); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	if plugin.Name == "" {
		http.Error(w, "plugin name is required", http.StatusBadRequest)
		return
	}
	h.store.SetPlugin(&plugin)
	writeJSON(w, http.StatusCreated, plugin)
}

func (h *Handler) updatePlugin(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	var plugin store.Plugin
	if err := decodeBody(r, &plugin); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	plugin.Name = name
	if _, ok := h.store.GetPlugin(name); !ok {
		http.Error(w, fmt.Sprintf("plugin %q not found", name), http.StatusNotFound)
		return
	}
	h.store.SetPlugin(&plugin)
	writeJSON(w, http.StatusOK, plugin)
}

func (h *Handler) deletePlugin(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	if !h.store.DeletePlugin(name) {
		http.Error(w, fmt.Sprintf("plugin %q not found", name), http.StatusNotFound)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// --- TlsCerts ---

func (h *Handler) listTlsCerts(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, h.store.ListTlsCerts())
}

func (h *Handler) getTlsCert(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	cert, ok := h.store.GetTlsCert(name)
	if !ok {
		http.Error(w, fmt.Sprintf("tls cert %q not found", name), http.StatusNotFound)
		return
	}
	writeJSON(w, http.StatusOK, cert)
}

func (h *Handler) createTlsCert(w http.ResponseWriter, r *http.Request) {
	var cert store.TlsCert
	if err := decodeBody(r, &cert); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	if cert.Name == "" {
		http.Error(w, "tls cert name is required", http.StatusBadRequest)
		return
	}
	h.store.SetTlsCert(&cert)
	writeJSON(w, http.StatusCreated, cert)
}

func (h *Handler) updateTlsCert(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	var cert store.TlsCert
	if err := decodeBody(r, &cert); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	cert.Name = name
	if _, ok := h.store.GetTlsCert(name); !ok {
		http.Error(w, fmt.Sprintf("tls cert %q not found", name), http.StatusNotFound)
		return
	}
	h.store.SetTlsCert(&cert)
	writeJSON(w, http.StatusOK, cert)
}

func (h *Handler) deleteTlsCert(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	if !h.store.DeleteTlsCert(name) {
		http.Error(w, fmt.Sprintf("tls cert %q not found", name), http.StatusNotFound)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// --- Health & Version ---

func (h *Handler) health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]interface{}{
		"status":  "healthy",
		"version": h.store.Version(),
	})
}

func (h *Handler) version(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]interface{}{
		"version": h.store.Version(),
	})
}

// --- Helpers ---

func writeJSON(w http.ResponseWriter, status int, v interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	if err := enc.Encode(v); err != nil {
		log.Printf("JSON encode error: %v", err)
	}
}

func decodeBody(r *http.Request, v interface{}) error {
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()
	if err := dec.Decode(v); err != nil {
		return fmt.Errorf("invalid request body: %w", err)
	}
	return nil
}

// ExtractNameFromPath is a helper for extracting the {name} path value
// from older Go versions that don't support PathValue.
func ExtractNameFromPath(path, prefix string) string {
	trimmed := strings.TrimPrefix(path, prefix)
	trimmed = strings.TrimPrefix(trimmed, "/")
	return trimmed
}
