package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"syscall"
	"time"

	"github.com/tpt-cloud-native/frontier/control-plane/internal/ratelimit"

	"github.com/tpt-cloud-native/frontier/control-plane/internal/configpush"
	consulsync "github.com/tpt-cloud-native/frontier/control-plane/internal/consul"
	grpcserver "github.com/tpt-cloud-native/frontier/control-plane/internal/grpc"
	"github.com/tpt-cloud-native/frontier/control-plane/internal/rest"
	"github.com/tpt-cloud-native/frontier/control-plane/internal/store"
	"github.com/tpt-cloud-native/frontier/control-plane/internal/xds"
)

func main() {
	httpAddr := flag.String("http-addr", ":8090", "HTTP listen address")
	grpcAddr := flag.String("grpc-addr", ":8091", "gRPC listen address")
	consulAddr := flag.String("consul-addr", "", "Consul address (optional)")
	configPath := flag.String("config-path", "/var/run/frontier/config.json", "Config file output path")
	flag.Parse()

	// Shared in-memory config store with event bus
	cfgStore := store.New()
	log.Printf("config store initialized: %s", cfgStore)

	// xDS resource builder
	_ = xds.NewResourceBuilder(cfgStore)

	// REST API handler
	restHandler := rest.NewHandler(cfgStore)

	// gRPC server
	grpcSrv := grpcserver.NewServer(*grpcAddr, cfgStore)

	// Atomic config file pusher for Rust proxy
	pusher := configpush.NewPusher(cfgStore, *configPath, 2*time.Second)

	// Consul catalog syncer (optional)
	var consulSyncer *consulsync.CatalogSyncer
	if *consulAddr != "" {
		consulSyncer = consulsync.NewCatalogSyncer(consulsync.Config{
			Address: *consulAddr,
		}, cfgStore)
	}

	// Metrics
	metrics := NewMetrics()

	// Setup HTTP server with Go 1.22 ServeMux
	mux := http.NewServeMux()
	restHandler.RegisterRoutes(mux)

	// xDS discovery endpoint (legacy JSON API)
	mux.HandleFunc("POST /v3/discovery", handleXdsDiscovery(cfgStore))
	mux.Handle("GET /metrics", metrics)

	// Rate limiting
	frontierRateLimit := 300
	if env := os.Getenv("FRONTIER_RATE_LIMIT"); env != "" {
		if v, err := strconv.Atoi(env); err == nil && v > 0 {
			frontierRateLimit = v
		}
	}
	rl := ratelimit.New(frontierRateLimit, time.Minute)

	httpSrv := &http.Server{
		Addr:         *httpAddr,
		Handler:      rl.Middleware(metrics.Instrument(mux)),
		ReadTimeout:  15 * time.Second,
		WriteTimeout: 15 * time.Second,
		IdleTimeout:  60 * time.Second,
	}

	// Graceful shutdown context
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	// Start all components
	go func() {
		if err := grpcSrv.Start(ctx); err != nil {
			log.Printf("gRPC server error: %v", err)
		}
	}()

	pusher.Start(ctx)

	if consulSyncer != nil {
		consulSyncer.Start(ctx)
	}

	// Handle signals
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

	go func() {
		sig := <-sigCh
		log.Printf("received signal %v, shutting down", sig)
		cancel()
		httpSrv.Shutdown(context.Background())
	}()

	log.Printf("TPT Frontier Control Plane listening on %s (HTTP) / %s (gRPC)", *httpAddr, *grpcAddr)
	if err := httpSrv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
		log.Fatalf("HTTP server error: %v", err)
	}

	// Cleanup
	grpcSrv.Stop()
	pusher.Stop()
	if consulSyncer != nil {
		consulSyncer.Stop()
	}
	fmt.Println("shutdown complete")
}

// handleXdsDiscovery handles the legacy JSON xDS discovery endpoint.
func handleXdsDiscovery(cfgStore *store.Store) http.HandlerFunc {
	type xdsRequest struct {
		TypeURL       string   `json:"type_url"`
		VersionInfo   string   `json:"version_info"`
		ResponseNonce string   `json:"response_nonce"`
		ResourceNames []string `json:"resource_names"`
	}
	type xdsResource struct {
		TypeURL string      `json:"type_url"`
		Name    string      `json:"name"`
		Version int64       `json:"version"`
		Payload interface{} `json:"payload"`
	}
	type xdsResponse struct {
		TypeURL     string        `json:"type_url"`
		VersionInfo string        `json:"version_info"`
		Nonce       string        `json:"nonce"`
		Resources   []xdsResource `json:"resources"`
	}

	return func(w http.ResponseWriter, r *http.Request) {
		var req xdsRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}

		version := cfgStore.Version()
		resp := xdsResponse{
			TypeURL:     req.TypeURL,
			VersionInfo: fmt.Sprintf("%d", version),
			Nonce:       fmt.Sprintf("nonce-%d", time.Now().UnixNano()),
		}

		switch req.TypeURL {
		case "type.googleapis.com/envoy.config.cluster.v3.Cluster":
			for _, u := range cfgStore.ListUpstreams() {
				resp.Resources = append(resp.Resources, xdsResource{
					TypeURL: req.TypeURL,
					Name:    u.Name,
					Version: version,
					Payload: u,
				})
			}
		case "type.googleapis.com/envoy.config.route.v3.RouteConfiguration":
			for _, r := range cfgStore.ListRoutes() {
				resp.Resources = append(resp.Resources, xdsResource{
					TypeURL: req.TypeURL,
					Name:    r.Name,
					Version: version,
					Payload: r,
				})
			}
		case "type.googleapis.com/envoy.config.endpoint.v3.ClusterLoadAssignment":
			for _, u := range cfgStore.ListUpstreams() {
				resp.Resources = append(resp.Resources, xdsResource{
					TypeURL: req.TypeURL,
					Name:    u.Name,
					Version: version,
					Payload: u,
				})
			}
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(resp)
	}
}
