package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

type ConsulKV struct {
	address    string
	httpClient *http.Client
}

type ConsulKVEntry struct {
	Key         string `json:"Key"`
	Value       string `json:"Value"`
	ModifyIndex uint64 `json:"ModifyIndex"`
}

func NewConsulKV(address string) *ConsulKV {
	return &ConsulKV{
		address: strings.TrimRight(address, "/"),
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

func (c *ConsulKV) Get(key string) (string, error) {
	resp, err := c.httpClient.Get(fmt.Sprintf("%s/v1/kv/%s?raw", c.address, url.PathEscape(key)))
	if err != nil {
		return "", fmt.Errorf("consul GET failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return "", nil
	}
	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return "", fmt.Errorf("consul returned %d: %s", resp.StatusCode, string(body))
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", fmt.Errorf("failed to read response: %w", err)
	}
	return string(body), nil
}

func (c *ConsulKV) Set(key, value string) error {
	req, err := http.NewRequest(http.MethodPut, fmt.Sprintf("%s/v1/kv/%s", c.address, url.PathEscape(key)), strings.NewReader(value))
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("consul PUT failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("consul returned %d: %s", resp.StatusCode, string(body))
	}
	return nil
}

func (c *ConsulKV) Delete(key string) error {
	req, err := http.NewRequest(http.MethodDelete, fmt.Sprintf("%s/v1/kv/%s", c.address, url.PathEscape(key)), nil)
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("consul DELETE failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("consul returned %d: %s", resp.StatusCode, string(body))
	}
	return nil
}

func (c *ConsulKV) List(prefix string) ([]ConsulKVEntry, error) {
	resp, err := c.httpClient.Get(fmt.Sprintf("%s/v1/kv/%s?recurse&raw", c.address, url.PathEscape(prefix)))
	if err != nil {
		return nil, fmt.Errorf("consul LIST failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return nil, nil
	}
	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("consul returned %d: %s", resp.StatusCode, string(body))
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("failed to read response: %w", err)
	}

	var entries []ConsulKVEntry
	if err := json.Unmarshal(body, &entries); err != nil {
		return nil, fmt.Errorf("failed to unmarshal entries: %w", err)
	}
	return entries, nil
}

type WatchCallback func(key, value string) error

func (c *ConsulKV) Watch(key string, callback WatchCallback) error {
	var index uint64
	for {
		resp, err := c.httpClient.Get(fmt.Sprintf("%s/v1/kv/%s?index=%d&wait=30s", c.address, url.PathEscape(key), index))
		if err != nil {
			time.Sleep(time.Second)
			continue
		}

		if resp.StatusCode != http.StatusOK {
			resp.Body.Close()
			time.Sleep(time.Second)
			continue
		}

		body, err := io.ReadAll(resp.Body)
		resp.Body.Close()
		if err != nil {
			time.Sleep(time.Second)
			continue
		}

		var entries []ConsulKVEntry
		if err := json.Unmarshal(body, &entries); err != nil {
			time.Sleep(time.Second)
			continue
		}

		for _, entry := range entries {
			if entry.ModifyIndex > index {
				if err := callback(entry.Key, entry.Value); err != nil {
					return fmt.Errorf("watch callback failed: %w", err)
				}
				index = entry.ModifyIndex
			}
		}
	}
}
