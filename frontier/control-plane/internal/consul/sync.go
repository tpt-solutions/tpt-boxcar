package consul

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"strconv"
	"sync"
	"time"

	"github.com/tpt-boxcar/frontier/control-plane/internal/store"
)

// Config holds the Consul connection settings.
type Config struct {
	Address    string `json:"address"`
	Token      string `json:"token,omitempty"`
	Datacenter string `json:"datacenter,omitempty"`
}

// CatalogService is a service entry from the Consul catalog.
type CatalogService struct {
	ID                string            `json:"ID"`
	Service           string            `json:"Service"`
	Tags              []string          `json:"Tags"`
	Address           string            `json:"Address"`
	Port              int               `json:"Port"`
	Meta              map[string]string `json:"Meta,omitempty"`
	EnableTagOverride bool              `json:"EnableTagOverride"`
}

// CatalogSyncer uses Consul HTTP blocking queries to watch the catalog
// and push discovered services/routes into the config store.
type CatalogSyncer struct {
	config  Config
	store   *store.Store
	client  *http.Client
	mu      sync.RWMutex
	lastIdx uint64
	cancel  context.CancelFunc
}

// NewCatalogSyncer creates a new Consul catalog synchronizer.
func NewCatalogSyncer(cfg Config, st *store.Store) *CatalogSyncer {
	return &CatalogSyncer{
		config: cfg,
		store:  st,
		client: &http.Client{Timeout: 60 * time.Second},
	}
}

// Start begins blocking-query loop against Consul.
func (cs *CatalogSyncer) Start(ctx context.Context) {
	ctx, cancel := context.WithCancel(ctx)
	cs.cancel = cancel

	go func() {
		log.Printf("Consul catalog syncer started (address=%s)", cs.config.Address)
		for {
			select {
			case <-ctx.Done():
				log.Printf("Consul catalog syncer stopped")
				return
			default:
				if err := cs.syncOnce(ctx); err != nil {
					log.Printf("Consul sync error: %v", err)
					// brief backoff before retry
					select {
					case <-ctx.Done():
						return
					case <-time.After(5 * time.Second):
					}
				}
			}
		}
	}()
}

// Stop terminates the blocking-query loop.
func (cs *CatalogSyncer) Stop() {
	if cs.cancel != nil {
		cs.cancel()
	}
}

// syncOnce performs one blocking query cycle.
func (cs *CatalogSyncer) syncOnce(ctx context.Context) error {
	services, idx, err := cs.blockingQueryServices(ctx)
	if err != nil {
		return fmt.Errorf("blocking query: %w", err)
	}

	cs.mu.Lock()
	cs.lastIdx = idx
	cs.mu.Unlock()

	for _, svc := range services {
		entry := &store.Upstream{
			Name:     svc.Service,
			Type:     "EDS",
			Endpoints: []string{fmt.Sprintf("%s:%d", svc.Address, svc.Port)},
			LbPolicy: "ROUND_ROBIN",
		}
		cs.store.SetUpstream(entry)

		if svc.Meta != nil {
			for k, v := range svc.Meta {
				_ = k
				_ = v
			}
		}
	}

	log.Printf("Consul sync complete: %d services at index %d", len(services), idx)
	return nil
}

// blockingQueryServices uses ?index=N&wait=30s to long-poll Consul.
func (cs *CatalogSyncer) blockingQueryServices(ctx context.Context) ([]CatalogService, uint64, error) {
	url := fmt.Sprintf("http://%s/v1/catalog/services?index=%d&wait=30s", cs.config.Address, cs.lastIdx)

	req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
	if err != nil {
		return nil, 0, err
	}
	if cs.config.Token != "" {
		req.Header.Set("X-Consul-Token", cs.config.Token)
	}
	if cs.config.Datacenter != "" {
		q := req.URL.Query()
		q.Set("dc", cs.config.Datacenter)
		req.URL.RawQuery = q.Encode()
	}

	resp, err := cs.client.Do(req)
	if err != nil {
		return nil, 0, err
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, 0, err
	}

	// Parse the services map: name -> tags
	var servicesMap map[string][]string
	if err := json.Unmarshal(body, &servicesMap); err != nil {
		return nil, 0, fmt.Errorf("parse services: %w", err)
	}

	// Extract index from X-Consul-Index header
	newIdx := cs.lastIdx
	if idxHeader := resp.Header.Get("X-Consul-Index"); idxHeader != "" {
		if parsed, err := strconv.ParseUint(idxHeader, 10, 64); err == nil {
			newIdx = parsed
		}
	}

	// Convert to CatalogService entries
	var services []CatalogService
	for name := range servicesMap {
		services = append(services, CatalogService{
			Service: name,
			Tags:    servicesMap[name],
		})
	}

	return services, newIdx, nil
}

// Register registers a service with Consul agent.
func (cs *CatalogSyncer) Register(svc CatalogService) error {
	payload, err := json.Marshal(svc)
	if err != nil {
		return err
	}

	url := fmt.Sprintf("http://%s/v1/agent/service/register", cs.config.Address)
	req, err := http.NewRequest("PUT", url, io.NopCloser(
		io.Reader(jsonReader(payload)),
	))
	if err != nil {
		return err
	}
	if cs.config.Token != "" {
		req.Header.Set("X-Consul-Token", cs.config.Token)
	}
	resp, err := cs.client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("register failed with status %d", resp.StatusCode)
	}
	return nil
}

// jsonReader wraps a byte slice in a reader.
func jsonReader(data []byte) io.Reader {
	return io.NopCloser(newBytesReader(data))
}

type bytesReader struct {
	data []byte
	pos  int
}

func newBytesReader(data []byte) *bytesReader {
	return &bytesReader{data: data}
}

func (r *bytesReader) Read(p []byte) (int, error) {
	if r.pos >= len(r.data) {
		return 0, io.EOF
	}
	n := copy(p, r.data[r.pos:])
	r.pos += n
	return n, nil
}
