package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"sync"
	"time"
)

// ConfigMapEvent represents a change event from the Kubernetes watch API.
type ConfigMapEvent struct {
	Type string `json:"type"`
	Data struct {
		Metadata struct {
			Name string `json:"name"`
		} `json:"metadata"`
		Data map[string]string `json:"data"`
	} `json:"object"`
}

type ConfigReloadCallback func(data map[string]string) error

type ConfigMapWatcher struct {
	namespace string
	configMap  string
	callback   ConfigReloadCallback
	mu         sync.RWMutex
	client     *http.Client
	cancel     context.CancelFunc
}

func NewConfigMapWatcher(namespace, configMap string, callback ConfigReloadCallback) (*ConfigMapWatcher, error) {
	return &ConfigMapWatcher{
		namespace: namespace,
		configMap:  configMap,
		callback:   callback,
		client: &http.Client{
			Timeout: 0, // No timeout for long-polling
		},
	}, nil
}

func (w *ConfigMapWatcher) Start(ctx context.Context) error {
	ctx, w.cancel = context.WithCancel(ctx)

	// Get Kubernetes API server URL from environment or default
	apiServer := "https://kubernetes.default.svc"
	if host := os.Getenv("KUBERNETES_SERVICE_HOST"); host != "" {
		port := os.Getenv("KUBERNETES_SERVICE_PORT")
		if port == "" {
			port = "443"
		}
		apiServer = fmt.Sprintf("https://%s:%s", host, port)
	}

	// Get initial data
	if err := w.loadInitialData(apiServer); err != nil {
		log.Printf("initial config load failed: %v", err)
	}

	// Start watching
	go w.watchLoop(ctx, apiServer)
	log.Printf("ConfigMap watcher started for %s/%s", w.namespace, w.configMap)
	return nil
}

func (w *ConfigMapWatcher) loadInitialData(apiServer string) error {
	url := fmt.Sprintf("%s/api/v1/namespaces/%s/configmaps/%s", apiServer, w.namespace, w.configMap)
	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/json")

	resp, err := w.client.Do(req)
	if err != nil {
		return fmt.Errorf("failed to get configmap: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("configmap get returned status %d", resp.StatusCode)
	}

	var result struct {
		Data map[string]string `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return fmt.Errorf("failed to parse configmap: %w", err)
	}

	return w.triggerReload(result.Data)
}

func (w *ConfigMapWatcher) watchLoop(ctx context.Context, apiServer string) {
	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		url := fmt.Sprintf("%s/api/v1/namespaces/%s/configmaps/%s?watch=true", apiServer, w.namespace, w.configMap)
		req, err := http.NewRequest(http.MethodGet, url, nil)
		if err != nil {
			log.Printf("watch request creation failed: %v", err)
			time.Sleep(time.Second * 5)
			continue
		}
		req.Header.Set("Accept", "application/json")

		resp, err := w.client.Do(req)
		if err != nil {
			log.Printf("watch request failed: %v", err)
			time.Sleep(time.Second * 5)
			continue
		}

		if resp.StatusCode != http.StatusOK {
			resp.Body.Close()
			log.Printf("watch returned status %d", resp.StatusCode)
			time.Sleep(time.Second * 5)
			continue
		}

		decoder := json.NewDecoder(resp.Body)
		for {
			var event ConfigMapEvent
			if err := decoder.Decode(&event); err != nil {
				if err == io.EOF {
					resp.Body.Close()
					break // Reconnect
				}
				log.Printf("watch decode error: %v", err)
				continue
			}

			if event.Type == "ADDED" || event.Type == "MODIFIED" {
				if err := w.triggerReload(event.Data.Data); err != nil {
					log.Printf("config reload callback failed: %v", err)
				}
			}
		}
	}
}

func (w *ConfigMapWatcher) triggerReload(data map[string]string) error {
	w.mu.Lock()
	defer w.mu.Unlock()

	if err := w.callback(data); err != nil {
		log.Printf("config reload callback failed: %v", err)
		return err
	}
	log.Printf("config reloaded successfully from %s/%s", w.namespace, w.configMap)
	return nil
}

func (w *ConfigMapWatcher) Stop() {
	if w.cancel != nil {
		w.cancel()
	}
}

func (w *ConfigMapWatcher) GetCurrentData() (map[string]string, error) {
	apiServer := "https://kubernetes.default.svc"
	if host := os.Getenv("KUBERNETES_SERVICE_HOST"); host != "" {
		port := os.Getenv("KUBERNETES_SERVICE_PORT")
		if port == "" {
			port = "443"
		}
		apiServer = fmt.Sprintf("https://%s:%s", host, port)
	}

	url := fmt.Sprintf("%s/api/v1/namespaces/%s/configmaps/%s", apiServer, w.namespace, w.configMap)
	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "application/json")

	resp, err := w.client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to get configmap: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("configmap get returned status %d", resp.StatusCode)
	}

	var result struct {
		Data map[string]string `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to parse configmap: %w", err)
	}
	return result.Data, nil
}