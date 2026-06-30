package main

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"sync"
)

type Backend struct {
	ID       string            `json:"id"`
	Type     string            `json:"type"`
	Host     string            `json:"host"`
	Port     int               `json:"port"`
	Database string            `json:"database"`
	Options  map[string]string `json:"options,omitempty"`
}

type Route struct {
	ID        string `json:"id"`
	Pattern   string `json:"pattern"`
	BackendID string `json:"backend_id"`
	Priority  int    `json:"priority"`
}

type ConfigServer struct {
	mu        sync.RWMutex
	backends  map[string]*Backend
	routes    map[string]*Route
}

func NewConfigServer() *ConfigServer {
	return &ConfigServer{
		backends: make(map[string]*Backend),
		routes:   make(map[string]*Route),
	}
}

func (cs *ConfigServer) handleBackends(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")

	switch r.Method {
	case http.MethodGet:
		cs.mu.RLock()
		result := make([]*Backend, 0, len(cs.backends))
		for _, b := range cs.backends {
			result = append(result, b)
		}
		cs.mu.RUnlock()
		json.NewEncoder(w).Encode(result)

	case http.MethodPost:
		var b Backend
		if err := json.NewDecoder(r.Body).Decode(&b); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		cs.mu.Lock()
		cs.backends[b.ID] = &b
		cs.mu.Unlock()
		w.WriteHeader(http.StatusCreated)
		json.NewEncoder(w).Encode(b)

	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (cs *ConfigServer) handleRoutes(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")

	switch r.Method {
	case http.MethodGet:
		cs.mu.RLock()
		result := make([]*Route, 0, len(cs.routes))
		for _, rt := range cs.routes {
			result = append(result, rt)
		}
		cs.mu.RUnlock()
		json.NewEncoder(w).Encode(result)

	case http.MethodPost:
		var rt Route
		if err := json.NewDecoder(r.Body).Decode(&rt); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		cs.mu.Lock()
		cs.routes[rt.ID] = &rt
		cs.mu.Unlock()
		w.WriteHeader(http.StatusCreated)
		json.NewEncoder(w).Encode(rt)

	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

func (cs *ConfigServer) handleHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "healthy"})
}

func main() {
	cs := NewConfigServer()

	http.HandleFunc("/api/v1/backends", cs.handleBackends)
	http.HandleFunc("/api/v1/routes", cs.handleRoutes)
	http.HandleFunc("/health", cs.handleHealth)

	addr := ":8080"
	fmt.Printf("TPT Tether Control Plane listening on %s\n", addr)
	log.Fatal(http.ListenAndServe(addr, nil))
}
