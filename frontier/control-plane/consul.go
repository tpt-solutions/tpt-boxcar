package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/tpt-cloud-native/frontier/control-plane/internal/store"
)

type ConsulConfig struct {
	Address    string `json:"address"`
	Token      string `json:"token,omitempty"`
	Datacenter string `json:"datacenter,omitempty"`
}

type ConsulService struct {
	ID                string            `json:"ID"`
	Service           string            `json:"Service"`
	Tags              []string          `json:"Tags"`
	Address           string            `json:"Address"`
	Port              int               `json:"Port"`
	Meta              map[string]string `json:"Meta,omitempty"`
	EnableTagOverride bool              `json:"EnableTagOverride"`
}

type ConsulIntention struct {
	ID              string `json:"ID"`
	SourceNS        string `json:"SourceNS"`
	SourceName      string `json:"SourceName"`
	DestinationNS   string `json:"DestinationNS"`
	DestinationName string `json:"DestinationName"`
	Permission      string `json:"Permission"`
	Action          string `json:"Action"`
}

type ConsulConnectConsumer struct {
	config    ConsulConfig
	store     *store.Store
	client    *http.Client
	mu        sync.RWMutex
	lastSync  time.Time
	cancel    context.CancelFunc
}

func NewConsulConnectConsumer(config ConsulConfig, st *store.Store) *ConsulConnectConsumer {
	return &ConsulConnectConsumer{
		config: config,
		store:  st,
		client: &http.Client{Timeout: 30 * time.Second},
	}
}

func (c *ConsulConnectConsumer) Start(ctx context.Context) {
	ctx, cancel := context.WithCancel(ctx)
	c.cancel = cancel

	go func() {
		ticker := time.NewTicker(15 * time.Second)
		defer ticker.Stop()

		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				if err := c.syncFromConsul(); err != nil {
					log.Printf("Consul sync error: %v", err)
				}
			}
		}
	}()

	log.Printf("Consul Connect consumer started (address=%s)", c.config.Address)
}

func (c *ConsulConnectConsumer) Stop() {
	if c.cancel != nil {
		c.cancel()
	}
}

func (c *ConsulConnectConsumer) syncFromConsul() error {
	services, err := c.fetchServices()
	if err != nil {
		return fmt.Errorf("fetch services: %w", err)
	}

	intentErr := c.fetchAndApplyIntentions()

	c.mu.Lock()
	c.lastSync = time.Now()
	c.mu.Unlock()

	for _, svc := range services {
		entry := &store.Upstream{
			Name:     svc.Service,
			Type:     "EDS",
			Endpoints: []string{fmt.Sprintf("%s:%d", svc.Address, svc.Port)},
			LbPolicy: "ROUND_ROBIN",
		}
		c.store.SetUpstream(entry)
	}

	if intentErr != nil {
		log.Printf("intentions sync error: %v (continuing with services only)", intentErr)
	}

	log.Printf("Consul sync complete: %d services", len(services))
	return nil
}

func (c *ConsulConnectConsumer) fetchAndApplyIntentions() error {
	intentions, err := c.fetchIntentions()
	if err != nil {
		return err
	}

	for _, intent := range intentions {
		if intent.Permission == "allow" {
			route := &store.Route{
				Name:    fmt.Sprintf("%s-to-%s", intent.SourceName, intent.DestinationName),
				Cluster: intent.DestinationName,
			}
			c.store.SetRoute(route)
		}
	}
	return nil
}

func (c *ConsulConnectConsumer) fetchServices() ([]ConsulService, error) {
	url := fmt.Sprintf("http://%s/v1/agent/services", c.config.Address)
	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		return nil, err
	}

	if c.config.Token != "" {
		req.Header.Set("X-Consul-Token", c.config.Token)
	}

	resp, err := c.client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}

	var servicesMap map[string]ConsulService
	if err := json.Unmarshal(body, &servicesMap); err != nil {
		return nil, fmt.Errorf("parse services: %w", err)
	}

	services := make([]ConsulService, 0, len(servicesMap))
	for _, svc := range servicesMap {
		services = append(services, svc)
	}

	return services, nil
}

func (c *ConsulConnectConsumer) fetchIntentions() ([]ConsulIntention, error) {
	url := fmt.Sprintf("http://%s/v1/connect/intentions", c.config.Address)
	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		return nil, err
	}

	if c.config.Token != "" {
		req.Header.Set("X-Consul-Token", c.config.Token)
	}

	resp, err := c.client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}

	var intentions []ConsulIntention
	if err := json.Unmarshal(body, &intentions); err != nil {
		return nil, fmt.Errorf("parse intentions: %w", err)
	}

	return intentions, nil
}

func (c *ConsulConnectConsumer) RegisterService(svc ConsulService) error {
	payload, err := json.Marshal(svc)
	if err != nil {
		return err
	}

	url := fmt.Sprintf("http://%s/v1/agent/service/register", c.config.Address)
	req, err := http.NewRequest("PUT", url, bytes.NewReader(payload))
	if err != nil {
		return err
	}

	if c.config.Token != "" {
		req.Header.Set("X-Consul-Token", c.config.Token)
	}

	resp, err := c.client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("register failed with status %d", resp.StatusCode)
	}

	return nil
}

func (c *ConsulConnectConsumer) DeregisterService(serviceID string) error {
	url := fmt.Sprintf("http://%s/v1/agent/service/deregister/%s", c.config.Address, serviceID)
	req, err := http.NewRequest("PUT", url, nil)
	if err != nil {
		return err
	}

	if c.config.Token != "" {
		req.Header.Set("X-Consul-Token", c.config.Token)
	}

	resp, err := c.client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("deregister failed with status %d", resp.StatusCode)
	}

	return nil
}

func (c *ConsulConnectConsumer) LastSync() time.Time {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.lastSync
}
