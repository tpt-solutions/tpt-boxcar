package store

import (
	"encoding/json"
	"fmt"
	"sync"
)

// Route mirrors the proto Route message.
type Route struct {
	Name        string            `json:"name"`
	Prefix      string            `json:"prefix"`
	Cluster     string            `json:"cluster"`
	Headers     map[string]string `json:"headers,omitempty"`
	Priority    uint32            `json:"priority"`
	StripPrefix bool              `json:"strip_prefix"`
}

// Upstream mirrors the proto Upstream message.
type Upstream struct {
	Name           string   `json:"name"`
	Type           string   `json:"type"`
	Endpoints      []string `json:"endpoints"`
	ConnectTimeout string   `json:"connect_timeout"`
	LbPolicy       string   `json:"lb_policy"`
}

// Plugin mirrors the proto Plugin message.
type Plugin struct {
	Name   string            `json:"name"`
	Type   string            `json:"type"`
	Config map[string]string `json:"config,omitempty"`
}

// TlsCert mirrors the proto TlsCert message.
type TlsCert struct {
	Name    string `json:"name"`
	CertPEM string `json:"cert_pem"`
	KeyPEM  string `json:"key_pem"`
	CaPEM   string `json:"ca_pem"`
}

// ConfigSnapshot is the complete config state at a given version.
type ConfigSnapshot struct {
	Version   int64                `json:"version"`
	Routes    map[string]*Route    `json:"routes"`
	Upstreams map[string]*Upstream `json:"upstreams"`
	Plugins   map[string]*Plugin   `json:"plugins"`
	TlsCerts  map[string]*TlsCert  `json:"tls_certs"`
}

// ConfigDiff is the incremental diff between two versions.
type ConfigDiff struct {
	BaseVersion            int64       `json:"base_version"`
	TargetVersion          int64       `json:"target_version"`
	AddedOrUpdatedRoutes   []*Route    `json:"added_or_updated_routes,omitempty"`
	RemovedRoutes          []string    `json:"removed_routes,omitempty"`
	AddedOrUpdatedUpstreams []*Upstream `json:"added_or_updated_upstreams,omitempty"`
	RemovedUpstreams       []string    `json:"removed_upstreams,omitempty"`
	AddedOrUpdatedPlugins  []*Plugin   `json:"added_or_updated_plugins,omitempty"`
	RemovedPlugins         []string    `json:"removed_plugins,omitempty"`
	AddedOrUpdatedTlsCerts []*TlsCert  `json:"added_or_updated_tls_certs,omitempty"`
	RemovedTlsCerts        []string    `json:"removed_tls_certs,omitempty"`
}

// EventType indicates what happened in the store.
type EventType int

const (
	EventSnapshot EventType = iota
	EventDiff
)

// Event is published on the event bus when config changes.
type Event struct {
	Type     EventType
	Version  int64
	Snapshot *ConfigSnapshot
	Diff     *ConfigDiff
}

// Subscriber receives events from the event bus.
type Subscriber chan Event

// Store is a thread-safe in-memory config store with monotonic versioning
// and a fan-out pub/sub event bus.
type Store struct {
	mu        sync.RWMutex
	routes    map[string]*Route
	upstreams map[string]*Upstream
	plugins   map[string]*Plugin
	tlsCerts  map[string]*TlsCert
	version   int64

	// event bus
	subscribers map[Subscriber]struct{}
	subMu       sync.Mutex
}

// New creates an initialized Store.
func New() *Store {
	return &Store{
		routes:      make(map[string]*Route),
		upstreams:   make(map[string]*Upstream),
		plugins:     make(map[string]*Plugin),
		tlsCerts:    make(map[string]*TlsCert),
		version:     0,
		subscribers: make(map[Subscriber]struct{}),
	}
}

// Version returns the current monotonic version.
func (s *Store) Version() int64 {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.version
}

// --- Routes ---

func (s *Store) ListRoutes() []*Route {
	s.mu.RLock()
	defer s.mu.RUnlock()
	result := make([]*Route, 0, len(s.routes))
	for _, r := range s.routes {
		result = append(result, r)
	}
	return result
}

func (s *Store) GetRoute(name string) (*Route, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	r, ok := s.routes[name]
	return r, ok
}

func (s *Store) SetRoute(r *Route) {
	s.mu.Lock()
	s.routes[r.Name] = r
	s.version++
	v := s.version
	s.mu.Unlock()

	s.publishDiff(&ConfigDiff{
		BaseVersion:          v - 1,
		TargetVersion:        v,
		AddedOrUpdatedRoutes: []*Route{r},
	})
}

func (s *Store) DeleteRoute(name string) bool {
	s.mu.Lock()
	if _, ok := s.routes[name]; !ok {
		s.mu.Unlock()
		return false
	}
	delete(s.routes, name)
	s.version++
	v := s.version
	s.mu.Unlock()

	s.publishDiff(&ConfigDiff{
		BaseVersion:   v - 1,
		TargetVersion: v,
		RemovedRoutes: []string{name},
	})
	return true
}

// --- Upstreams ---

func (s *Store) ListUpstreams() []*Upstream {
	s.mu.RLock()
	defer s.mu.RUnlock()
	result := make([]*Upstream, 0, len(s.upstreams))
	for _, u := range s.upstreams {
		result = append(result, u)
	}
	return result
}

func (s *Store) GetUpstream(name string) (*Upstream, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	u, ok := s.upstreams[name]
	return u, ok
}

func (s *Store) SetUpstream(u *Upstream) {
	s.mu.Lock()
	s.upstreams[u.Name] = u
	s.version++
	v := s.version
	s.mu.Unlock()

	s.publishDiff(&ConfigDiff{
		BaseVersion:            v - 1,
		TargetVersion:          v,
		AddedOrUpdatedUpstreams: []*Upstream{u},
	})
}

func (s *Store) DeleteUpstream(name string) bool {
	s.mu.Lock()
	if _, ok := s.upstreams[name]; !ok {
		s.mu.Unlock()
		return false
	}
	delete(s.upstreams, name)
	s.version++
	v := s.version
	s.mu.Unlock()

	s.publishDiff(&ConfigDiff{
		BaseVersion:     v - 1,
		TargetVersion:   v,
		RemovedUpstreams: []string{name},
	})
	return true
}

// --- Plugins ---

func (s *Store) ListPlugins() []*Plugin {
	s.mu.RLock()
	defer s.mu.RUnlock()
	result := make([]*Plugin, 0, len(s.plugins))
	for _, p := range s.plugins {
		result = append(result, p)
	}
	return result
}

func (s *Store) GetPlugin(name string) (*Plugin, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	p, ok := s.plugins[name]
	return p, ok
}

func (s *Store) SetPlugin(p *Plugin) {
	s.mu.Lock()
	s.plugins[p.Name] = p
	s.version++
	v := s.version
	s.mu.Unlock()

	s.publishDiff(&ConfigDiff{
		BaseVersion:           v - 1,
		TargetVersion:         v,
		AddedOrUpdatedPlugins: []*Plugin{p},
	})
}

func (s *Store) DeletePlugin(name string) bool {
	s.mu.Lock()
	if _, ok := s.plugins[name]; !ok {
		s.mu.Unlock()
		return false
	}
	delete(s.plugins, name)
	s.version++
	v := s.version
	s.mu.Unlock()

	s.publishDiff(&ConfigDiff{
		BaseVersion:    v - 1,
		TargetVersion:  v,
		RemovedPlugins: []string{name},
	})
	return true
}

// --- TlsCerts ---

func (s *Store) ListTlsCerts() []*TlsCert {
	s.mu.RLock()
	defer s.mu.RUnlock()
	result := make([]*TlsCert, 0, len(s.tlsCerts))
	for _, t := range s.tlsCerts {
		result = append(result, t)
	}
	return result
}

func (s *Store) GetTlsCert(name string) (*TlsCert, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	t, ok := s.tlsCerts[name]
	return t, ok
}

func (s *Store) SetTlsCert(t *TlsCert) {
	s.mu.Lock()
	s.tlsCerts[t.Name] = t
	s.version++
	v := s.version
	s.mu.Unlock()

	s.publishDiff(&ConfigDiff{
		BaseVersion:           v - 1,
		TargetVersion:         v,
		AddedOrUpdatedTlsCerts: []*TlsCert{t},
	})
}

func (s *Store) DeleteTlsCert(name string) bool {
	s.mu.Lock()
	if _, ok := s.tlsCerts[name]; !ok {
		s.mu.Unlock()
		return false
	}
	delete(s.tlsCerts, name)
	s.version++
	v := s.version
	s.mu.Unlock()

	s.publishDiff(&ConfigDiff{
		BaseVersion:     v - 1,
		TargetVersion:   v,
		RemovedTlsCerts: []string{name},
	})
	return true
}

// --- Snapshot ---

// Snapshot returns a point-in-time copy of the entire config state.
func (s *Store) Snapshot() *ConfigSnapshot {
	s.mu.RLock()
	defer s.mu.RUnlock()

	routes := make(map[string]*Route, len(s.routes))
	for k, v := range s.routes {
		r := *v
		routes[k] = &r
	}
	upstreams := make(map[string]*Upstream, len(s.upstreams))
	for k, v := range s.upstreams {
		u := *v
		upstreams[k] = &u
	}
	plugins := make(map[string]*Plugin, len(s.plugins))
	for k, v := range s.plugins {
		p := *v
		plugins[k] = &p
	}
	tlsCerts := make(map[string]*TlsCert, len(s.tlsCerts))
	for k, v := range s.tlsCerts {
		t := *v
		tlsCerts[k] = &t
	}

	return &ConfigSnapshot{
		Version:   s.version,
		Routes:    routes,
		Upstreams: upstreams,
		Plugins:   plugins,
		TlsCerts:  tlsCerts,
	}
}

// --- Event Bus ---

// Subscribe registers a channel to receive config events.
func (s *Store) Subscribe() Subscriber {
	ch := make(chan Event, 64)
	s.subMu.Lock()
	s.subscribers[ch] = struct{}{}
	s.subMu.Unlock()
	return ch
}

// Unsubscribe removes a channel from the event bus.
func (s *Store) Unsubscribe(ch Subscriber) {
	s.subMu.Lock()
	delete(s.subscribers, ch)
	s.subMu.Unlock()
	close(ch)
}

func (s *Store) publishDiff(diff *ConfigDiff) {
	s.subMu.Lock()
	defer s.subMu.Unlock()
	for ch := range s.subscribers {
		select {
		case ch <- Event{Type: EventDiff, Version: diff.TargetVersion, Diff: diff}:
		default:
			// subscriber full, drop event to avoid blocking
		}
	}
}

// PublishSnapshot sends a full snapshot to all subscribers (used on WatchConfig connect).
func (s *Store) PublishSnapshot(sub Subscriber) {
	snap := s.Snapshot()
	select {
	case sub <- Event{Type: EventSnapshot, Version: snap.Version, Snapshot: snap}:
	default:
	}
}

// SnapshotJSON returns the config snapshot as JSON bytes.
func (s *Store) SnapshotJSON() ([]byte, error) {
	return json.Marshal(s.Snapshot())
}

// String returns a debug representation.
func (s *Store) String() string {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return fmt.Sprintf("Store{v=%d, routes=%d, upstreams=%d, plugins=%d, tls_certs=%d}",
		s.version, len(s.routes), len(s.upstreams), len(s.plugins), len(s.tlsCerts))
}
