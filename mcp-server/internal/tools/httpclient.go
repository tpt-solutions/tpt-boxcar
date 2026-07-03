package tools

import (
	"bytes"
	"fmt"
	"io"
	"net/http"
	"time"
)

var httpClient = &http.Client{Timeout: 10 * time.Second}

// doJSON makes an HTTP request with an optional JSON body and API key,
// returning the raw response body text (the tool handlers pass this straight
// through as text content — the REST APIs already return well-formed JSON).
func doJSON(method, url, apiKey string, body []byte) (string, error) {
	var reader io.Reader
	if body != nil {
		reader = bytes.NewReader(body)
	}

	req, err := http.NewRequest(method, url, reader)
	if err != nil {
		return "", fmt.Errorf("build request: %w", err)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if apiKey != "" {
		req.Header.Set("X-API-Key", apiKey)
	}

	resp, err := httpClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("request to %s failed: %w", url, err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", fmt.Errorf("read response: %w", err)
	}

	if resp.StatusCode >= 400 {
		return "", fmt.Errorf("%s %s returned %d: %s", method, url, resp.StatusCode, string(respBody))
	}

	return string(respBody), nil
}
