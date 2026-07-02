package main

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/tpt-boxcar/frontier/control-plane/internal/rest"
	"github.com/tpt-boxcar/frontier/control-plane/internal/store"
)

func TestControlPlaneE2E(t *testing.T) {
	cfgStore := store.New()
	restHandler := rest.NewHandler(cfgStore)

	mux := http.NewServeMux()
	restHandler.RegisterRoutes(mux)

	server := httptest.NewServer(mux)
	defer server.Close()

	t.Run("health_check", func(t *testing.T) {
		resp, err := http.Get(server.URL + "/health")
		if err != nil {
			t.Fatalf("health check failed: %v", err)
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusOK {
			t.Fatalf("expected 200, got %d", resp.StatusCode)
		}

		var body map[string]interface{}
		json.NewDecoder(resp.Body).Decode(&body)

		if body["status"] != "healthy" {
			t.Fatalf("expected healthy status, got %v", body["status"])
		}
	})

	t.Run("create_and_list_upstreams", func(t *testing.T) {
		upstream := store.Upstream{
			Name:     "test-upstream",
			Type:     "EDS",
			Endpoints: []string{"10.0.0.1:8080"},
			LbPolicy: "ROUND_ROBIN",
		}

		payload, _ := json.Marshal(upstream)
		resp, err := http.Post(
			server.URL+"/api/v1/upstreams",
			"application/json",
			bytes.NewReader(payload),
		)
		if err != nil {
			t.Fatalf("create upstream failed: %v", err)
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusCreated {
			body, _ := io.ReadAll(resp.Body)
			t.Fatalf("expected 201, got %d: %s", resp.StatusCode, string(body))
		}

		resp2, err := http.Get(server.URL + "/api/v1/upstreams")
		if err != nil {
			t.Fatalf("list upstreams failed: %v", err)
		}
		defer resp2.Body.Close()

		var upstreams []store.Upstream
		json.NewDecoder(resp2.Body).Decode(&upstreams)

		if len(upstreams) != 1 {
			t.Fatalf("expected 1 upstream, got %d", len(upstreams))
		}
		if upstreams[0].Name != "test-upstream" {
			t.Fatalf("expected upstream name 'test-upstream', got '%s'", upstreams[0].Name)
		}
	})

	t.Run("create_and_list_routes", func(t *testing.T) {
		route := store.Route{
			Name:    "test-route",
			Prefix:  "/api/test",
			Cluster: "test-cluster",
		}

		payload, _ := json.Marshal(route)
		resp, err := http.Post(
			server.URL+"/api/v1/routes",
			"application/json",
			bytes.NewReader(payload),
		)
		if err != nil {
			t.Fatalf("create route failed: %v", err)
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusCreated {
			t.Fatalf("expected 201, got %d", resp.StatusCode)
		}

		resp2, err := http.Get(server.URL + "/api/v1/routes")
		if err != nil {
			t.Fatalf("list routes failed: %v", err)
		}
		defer resp2.Body.Close()

		var routes []store.Route
		json.NewDecoder(resp2.Body).Decode(&routes)

		if len(routes) != 1 {
			t.Fatalf("expected 1 route, got %d", len(routes))
		}
	})

	t.Run("delete_route", func(t *testing.T) {
		req, _ := http.NewRequest(
			http.MethodDelete,
			server.URL+"/api/v1/routes/test-route",
			nil,
		)
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("delete route failed: %v", err)
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusNoContent {
			t.Fatalf("expected 204, got %d", resp.StatusCode)
		}

		resp2, err := http.Get(server.URL + "/api/v1/routes")
		if err != nil {
			t.Fatalf("list routes failed: %v", err)
		}
		defer resp2.Body.Close()

		var routes []store.Route
		json.NewDecoder(resp2.Body).Decode(&routes)

		if len(routes) != 0 {
			t.Fatalf("expected 0 routes after delete, got %d", len(routes))
		}
	})

	t.Run("version_increments", func(t *testing.T) {
		initialVersion := cfgStore.Version()

		upstream := store.Upstream{
			Name:     "version-test",
			Type:     "EDS",
			Endpoints: []string{"10.0.0.2:9090"},
		}
		payload, _ := json.Marshal(upstream)
		http.Post(server.URL+"/api/v1/upstreams", "application/json", bytes.NewReader(payload))

		if cfgStore.Version() <= initialVersion {
			t.Fatalf("expected version to increment, got %d <= %d", cfgStore.Version(), initialVersion)
		}
	})

	t.Run("get_upstream_by_name", func(t *testing.T) {
		resp, err := http.Get(server.URL + "/api/v1/upstreams/version-test")
		if err != nil {
			t.Fatalf("get upstream failed: %v", err)
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusOK {
			t.Fatalf("expected 200, got %d", resp.StatusCode)
		}

		var upstream store.Upstream
		json.NewDecoder(resp.Body).Decode(&upstream)

		if upstream.Name != "version-test" {
			t.Fatalf("expected name 'version-test', got '%s'", upstream.Name)
		}
	})

	t.Run("upstream_not_found", func(t *testing.T) {
		resp, err := http.Get(server.URL + "/api/v1/upstreams/nonexistent")
		if err != nil {
			t.Fatalf("request failed: %v", err)
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusNotFound {
			t.Fatalf("expected 404, got %d", resp.StatusCode)
		}
	})
}

func TestStoreEventBus(t *testing.T) {
	cfgStore := store.New()
	sub := cfgStore.Subscribe()
	defer cfgStore.Unsubscribe(sub)

	route := &store.Route{Name: "event-test", Prefix: "/test", Cluster: "backend"}
	cfgStore.SetRoute(route)

	event := <-sub
	if event.Type != store.EventDiff {
		t.Fatalf("expected diff event, got %d", event.Type)
	}
	if event.Diff == nil {
		t.Fatal("expected non-nil diff")
	}
	if event.Diff.TargetVersion != 1 {
		t.Fatalf("expected version 1, got %d", event.Diff.TargetVersion)
	}
}

func TestConfigSnapshot(t *testing.T) {
	cfgStore := store.New()

	cfgStore.SetRoute(&store.Route{Name: "r1", Prefix: "/a", Cluster: "c1"})
	cfgStore.SetUpstream(&store.Upstream{Name: "u1", Endpoints: []string{"10.0.0.1:80"}})

	snap := cfgStore.Snapshot()
	if snap.Version != 2 {
		t.Fatalf("expected version 2, got %d", snap.Version)
	}
	if len(snap.Routes) != 1 {
		t.Fatalf("expected 1 route in snapshot, got %d", len(snap.Routes))
	}
	if len(snap.Upstreams) != 1 {
		t.Fatalf("expected 1 upstream in snapshot, got %d", len(snap.Upstreams))
	}

	data, err := cfgStore.SnapshotJSON()
	if err != nil {
		t.Fatalf("SnapshotJSON failed: %v", err)
	}
	if len(data) == 0 {
		t.Fatal("expected non-empty JSON")
	}
}
