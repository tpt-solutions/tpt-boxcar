package configpush

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/tpt-cloud-native/frontier/control-plane/internal/store"
)

const (
	// DefaultPath is the default config file location for the Rust proxy.
	DefaultPath = "/var/run/frontier/config.json"
)

// Pusher watches the config store and atomically writes JSON snapshots
// to disk using os.Rename for the Rust data plane to hot-reload.
type Pusher struct {
	store     *store.Store
	path      string
	interval  time.Duration
	lastVer   int64
	mu        sync.Mutex
	cancel    context.CancelFunc
}

// NewPusher creates a new atomic config file pusher.
func NewPusher(st *store.Store, path string, interval time.Duration) *Pusher {
	if path == "" {
		path = DefaultPath
	}
	if interval == 0 {
		interval = 2 * time.Second
	}
	return &Pusher{
		store:    st,
		path:     path,
		interval: interval,
	}
}

// Start begins the periodic config push loop.
func (p *Pusher) Start(ctx context.Context) {
	ctx, cancel := context.WithCancel(ctx)
	p.cancel = cancel

	go func() {
		log.Printf("config pusher started (path=%s, interval=%s)", p.path, p.interval)

		// Push initial snapshot immediately
		if err := p.push(); err != nil {
			log.Printf("initial config push error: %v", err)
		}

		ticker := time.NewTicker(p.interval)
		defer ticker.Stop()

		for {
			select {
			case <-ctx.Done():
				log.Printf("config pusher stopped")
				return
			case <-ticker.C:
				if err := p.push(); err != nil {
					log.Printf("config push error: %v", err)
				}
			}
		}
	}()
}

// Stop terminates the push loop.
func (p *Pusher) Stop() {
	if p.cancel != nil {
		p.cancel()
	}
}

// push writes the current config snapshot atomically to disk.
func (p *Pusher) push() error {
	snap := p.store.Snapshot()

	// Skip if version hasn't changed
	p.mu.Lock()
	if snap.Version == p.lastVer {
		p.mu.Unlock()
		return nil
	}
	p.mu.Unlock()

	data, err := json.MarshalIndent(snap, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal config: %w", err)
	}

	if err := atomicWrite(p.path, data); err != nil {
		return fmt.Errorf("atomic write: %w", err)
	}

	p.mu.Lock()
	p.lastVer = snap.Version
	p.mu.Unlock()

	log.Printf("config pushed: version=%d, size=%d bytes", snap.Version, len(data))
	return nil
}

// atomicWrite writes data to a temp file and renames it into place (os.Rename).
// This ensures the Rust proxy never reads a half-written file.
func atomicWrite(targetPath string, data []byte) error {
	dir := filepath.Dir(targetPath)

	// Ensure directory exists
	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("mkdir %s: %w", dir, err)
	}

	// Write to a temp file in the same directory (same filesystem for atomic rename)
	tmpFile, err := os.CreateTemp(dir, ".config-*.tmp")
	if err != nil {
		return fmt.Errorf("create temp: %w", err)
	}
	tmpPath := tmpFile.Name()

	// Clean up on failure
	success := false
	defer func() {
		if !success {
			os.Remove(tmpPath)
		}
	}()

	if _, err := tmpFile.Write(data); err != nil {
		tmpFile.Close()
		return fmt.Errorf("write temp: %w", err)
	}

	// Sync to disk before rename
	if err := tmpFile.Sync(); err != nil {
		tmpFile.Close()
		return fmt.Errorf("sync temp: %w", err)
	}

	if err := tmpFile.Close(); err != nil {
		return fmt.Errorf("close temp: %w", err)
	}

	// Atomic rename
	if err := os.Rename(tmpPath, targetPath); err != nil {
		return fmt.Errorf("rename: %w", err)
	}

	success = true
	return nil
}

// PushNow forces an immediate push regardless of version change.
func (p *Pusher) PushNow() error {
	return p.push()
}

// LastVersion returns the last pushed config version.
func (p *Pusher) LastVersion() int64 {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.lastVer
}
