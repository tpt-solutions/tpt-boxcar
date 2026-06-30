package grpc

import (
	"context"
	"fmt"
	"io"
	"log"
	"net"
	"sync"

	"github.com/tpt-cloud-native/frontier/control-plane/internal/store"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// Server implements the FrontierConfig gRPC service.
type Server struct {
	store  *store.Store
	grpc   *grpc.Server
	addr   string
	mu     sync.Mutex
}

// NewServer creates a new gRPC FrontierConfig server.
func NewServer(addr string, st *store.Store) *Server {
	return &Server{
		store: st,
		addr:  addr,
	}
}

// Start begins serving gRPC connections.
func (s *Server) Start(ctx context.Context) error {
	lis, err := net.Listen("tcp", s.addr)
	if err != nil {
		return fmt.Errorf("failed to listen on %s: %w", s.addr, err)
	}

	s.mu.Lock()
	s.grpc = grpc.NewServer()
	s.mu.Unlock()

	// Register our service (using the handler registration interface)
	// In production, this would use generated pb.RegisterFrontierConfigServer
	log.Printf("gRPC FrontierConfig server listening on %s", s.addr)

	go func() {
		<-ctx.Done()
		s.mu.Lock()
		if s.grpc != nil {
			s.grpc.GracefulStop()
		}
		s.mu.Unlock()
	}()

	if err := s.grpc.Serve(lis); err != nil && err != grpc.ErrServerStopped {
		return fmt.Errorf("gRPC serve error: %w", err)
	}
	return nil
}

// Stop gracefully stops the gRPC server.
func (s *Server) Stop() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.grpc != nil {
		s.grpc.GracefulStop()
	}
}

// --- WatchConfig Streaming ---

// WatchConfigStream manages a single WatchConfig streaming session.
type WatchConfigStream struct {
	store      *store.Store
	subscriber store.Subscriber
	sent       bool
}

// NewWatchConfigStream creates a new stream handler.
func NewWatchConfigStream(st *store.Store) *WatchConfigStream {
	return &WatchConfigStream{
		store: st,
	}
}

// Run sends the initial snapshot, then streams diffs until the context is done.
// sendFn is called with each event; it should serialize and send to the gRPC stream.
func (w *WatchConfigStream) Run(ctx context.Context, lastKnownVersion int64, sendFn func(event store.Event) error) error {
	w.subscriber = w.store.Subscribe()
	defer w.store.Unsubscribe(w.subscriber)

	snap := w.store.Snapshot()

	// If the client already has a version, send only the diff.
	if lastKnownVersion > 0 && lastKnownVersion < snap.Version {
		diff := w.buildDiffSince(lastKnownVersion, snap)
		if diff != nil {
			if err := sendFn(store.Event{Type: store.EventDiff, Version: snap.Version, Diff: diff}); err != nil {
				return err
			}
		}
	} else if lastKnownVersion == 0 || lastKnownVersion < snap.Version {
		// Send full snapshot
		if err := sendFn(store.Event{Type: store.EventSnapshot, Version: snap.Version, Snapshot: snap}); err != nil {
			return err
		}
	}
	// If lastKnownVersion == snap.Version, send nothing.

	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case event, ok := <-w.subscriber:
			if !ok {
				return io.EOF
			}
			if err := sendFn(event); err != nil {
				return err
			}
		}
	}
}

// buildDiffSince constructs a diff from a base version to the current snapshot.
func (w *WatchConfigStream) buildDiffSince(baseVersion int64, current *store.ConfigSnapshot) *store.ConfigDiff {
	// For simplicity, send a full snapshot as diff with all resources added.
	diff := &store.ConfigDiff{
		BaseVersion:   baseVersion,
		TargetVersion: current.Version,
	}
	for _, r := range current.Routes {
		diff.AddedOrUpdatedRoutes = append(diff.AddedOrUpdatedRoutes, r)
	}
	for _, u := range current.Upstreams {
		diff.AddedOrUpdatedUpstreams = append(diff.AddedOrUpdatedUpstreams, u)
	}
	for _, p := range current.Plugins {
		diff.AddedOrUpdatedPlugins = append(diff.AddedOrUpdatedPlugins, p)
	}
	for _, t := range current.TlsCerts {
		diff.AddedOrUpdatedTlsCerts = append(diff.AddedOrUpdatedTlsCerts, t)
	}
	return diff
}

// --- CRUD Implementations ---

func handleMetadata(ctx context.Context) metadata.MD {
	md, _ := metadata.FromIncomingContext(ctx)
	if md == nil {
		md = metadata.New(nil)
	}
	return md
}

func serverError(format string, args ...interface{}) error {
	return status.Errorf(codes.Internal, format, args...)
}

func notFoundError(name string) error {
	return status.Errorf(codes.NotFound, "%s not found", name)
}

// --- Health ---

// HealthCheck returns the server status.
func (s *Server) HealthCheck() map[string]interface{} {
	return map[string]interface{}{
		"status":  "healthy",
		"version": s.store.Version(),
	}
}
