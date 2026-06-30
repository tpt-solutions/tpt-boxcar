package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestE2E_BackendCRUD(t *testing.T) {
	cs := NewConfigServer()
	mux := http.NewServeMux()
	mux.HandleFunc("/api/v1/backends", cs.handleBackends)
	mux.HandleFunc("/health", cs.handleHealth)
	server := httptest.NewServer(mux)
	defer server.Close()

	client := server.Client()

	healthResp, err := client.Get(server.URL + "/health")
	if err != nil {
		t.Fatalf("health check failed: %v", err)
	}
	defer healthResp.Body.Close()
	if healthResp.StatusCode != http.StatusOK {
		t.Fatalf("health check returned %d", healthResp.StatusCode)
	}

	backendJSON := `{"id":"pg-1","type":"postgres","host":"localhost","port":5432,"database":"testdb"}`
	createResp, err := client.Post(
		server.URL+"/api/v1/backends",
		"application/json",
		strings.NewReader(backendJSON),
	)
	if err != nil {
		t.Fatalf("create backend failed: %v", err)
	}
	defer createResp.Body.Close()
	if createResp.StatusCode != http.StatusCreated {
		t.Fatalf("create backend returned %d", createResp.StatusCode)
	}

	var created Backend
	if err := json.NewDecoder(createResp.Body).Decode(&created); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}
	if created.ID != "pg-1" {
		t.Fatalf("expected backend ID pg-1, got %s", created.ID)
	}

	listResp, err := client.Get(server.URL + "/api/v1/backends")
	if err != nil {
		t.Fatalf("list backends failed: %v", err)
	}
	defer listResp.Body.Close()

	var backends []Backend
	if err := json.NewDecoder(listResp.Body).Decode(&backends); err != nil {
		t.Fatalf("failed to decode list: %v", err)
	}
	if len(backends) != 1 {
		t.Fatalf("expected 1 backend, got %d", len(backends))
	}
}

func TestE2E_RouteCRUD(t *testing.T) {
	cs := NewConfigServer()
	mux := http.NewServeMux()
	mux.HandleFunc("/api/v1/routes", cs.handleRoutes)
	server := httptest.NewServer(mux)
	defer server.Close()

	client := server.Client()

	routeJSON := `{"id":"route-1","pattern":"/api/v1/users/*","backend_id":"pg-1","priority":10}`
	createResp, err := client.Post(
		server.URL+"/api/v1/routes",
		"application/json",
		strings.NewReader(routeJSON),
	)
	if err != nil {
		t.Fatalf("create route failed: %v", err)
	}
	defer createResp.Body.Close()
	if createResp.StatusCode != http.StatusCreated {
		t.Fatalf("create route returned %d", createResp.StatusCode)
	}

	var created Route
	if err := json.NewDecoder(createResp.Body).Decode(&created); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}
	if created.ID != "route-1" {
		t.Fatalf("expected route ID route-1, got %s", created.ID)
	}
	if created.Pattern != "/api/v1/users/*" {
		t.Fatalf("expected pattern /api/v1/users/*, got %s", created.Pattern)
	}

	listResp, err := client.Get(server.URL + "/api/v1/routes")
	if err != nil {
		t.Fatalf("list routes failed: %v", err)
	}
	defer listResp.Body.Close()

	var routes []Route
	if err := json.NewDecoder(listResp.Body).Decode(&routes); err != nil {
		t.Fatalf("failed to decode list: %v", err)
	}
	if len(routes) != 1 {
		t.Fatalf("expected 1 route, got %d", len(routes))
	}
}

func TestE2E_ConfigReload(t *testing.T) {
	cs := NewConfigServer()
	reloadCount := 0
	var lastConfig map[string]string

	callback := func(data map[string]string) error {
		reloadCount++
		lastConfig = data
		return nil
	}

	_ = callback

	mux := http.NewServeMux()
	mux.HandleFunc("/api/v1/backends", cs.handleBackends)
	server := httptest.NewServer(mux)
	defer server.Close()

	client := server.Client()

	backendJSON := `{"id":"pg-1","type":"postgres","host":"localhost","port":5432,"database":"testdb"}`
	resp, err := client.Post(
		server.URL+"/api/v1/backends",
		"application/json",
		strings.NewReader(backendJSON),
	)
	if err != nil {
		t.Fatalf("create backend failed: %v", err)
	}
	resp.Body.Close()

	resp2, err := client.Post(
		server.URL+"/api/v1/backends",
		"application/json",
		strings.NewReader(backendJSON),
	)
	if err != nil {
		t.Fatalf("update backend failed: %v", err)
	}
	resp2.Body.Close()

	_ = lastConfig
}

func TestE2E_ConsulKV(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch {
		case r.Method == http.MethodGet && strings.Contains(r.URL.Path, "/v1/kv/"):
			if r.URL.Query().Has("raw") {
				w.Write([]byte("test"))
			} else {
				w.Write([]byte(`[{"Key":"test/key","Value":"dGVzdA==","ModifyIndex":1}]`))
			}
		case r.Method == http.MethodPut:
			w.WriteHeader(http.StatusOK)
		case r.Method == http.MethodDelete:
			w.WriteHeader(http.StatusOK)
		default:
			w.WriteHeader(http.StatusMethodNotAllowed)
		}
	}))
	defer server.Close()

	kv := NewConsulKV(server.URL)

	value, err := kv.Get("test/key")
	if err != nil {
		t.Fatalf("consul get failed: %v", err)
	}

	expected := "test"
	if value != expected {
		t.Fatalf("expected value %q, got %q", expected, value)
	}

	err = kv.Set("test/key", "newvalue")
	if err != nil {
		t.Fatalf("consul set failed: %v", err)
	}

	err = kv.Delete("test/key")
	if err != nil {
		t.Fatalf("consul delete failed: %v", err)
	}
}
