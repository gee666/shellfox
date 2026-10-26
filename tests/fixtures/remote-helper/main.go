// Test transport only. Production test-build injection requires a path inside tmp/.
// This simulates tsnet delivery, not Tailscale authentication or encryption.
package main

import (
	"bufio"
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

type registration struct {
	BackendURL string `json:"backendUrl"`
	Secret     string `json:"backendSecret"`
}
type startInput struct {
	StateDirectory string `json:"stateDirectory"`
	Hostname       string `json:"hostname"`
	BackendURL     string `json:"backendUrl"`
	Secret         string `json:"backendSecret"`
	ProxySecret    string `json:"proxySecret"`
}
type command struct {
	ID      string          `json:"id"`
	Method  string          `json:"method"`
	Payload json.RawMessage `json:"payload"`
}

func main() {
	bus := os.Getenv("SHELLFOX_TEST_REMOTE_BUS")
	if bus == "" {
		panic("test bus missing")
	}
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), 1048576)
	var config startInput
	var server *http.Server
	status := map[string]any{"status": "online", "loginUrl": nil, "error": nil}
	for scanner.Scan() {
		var cmd command
		if json.Unmarshal(scanner.Bytes(), &cmd) != nil {
			continue
		}
		var value any = map[string]any{}
		switch cmd.Method {
		case "start":
			if json.Unmarshal(cmd.Payload, &config) != nil {
				panic("invalid start")
			}
			if err := os.MkdirAll(bus, 0700); err != nil {
				panic(err)
			}
			data, _ := json.Marshal(registration{config.BackendURL, config.Secret})
			if err := os.WriteFile(filepath.Join(bus, config.Hostname+".json"), data, 0600); err != nil {
				panic(err)
			}
			listener, err := net.Listen("tcp", "127.0.0.1:0")
			if err != nil {
				panic(err)
			}
			server = &http.Server{Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { proxy(w, r, bus, config) }), ReadHeaderTimeout: 5 * time.Second}
			go server.Serve(listener)
			value = map[string]any{"proxyUrl": "http://" + listener.Addr().String()}
		case "status", "login":
			value = status
		case "peers":
			peers := []map[string]any{}
			entries, _ := os.ReadDir(bus)
			for _, entry := range entries {
				name := strings.TrimSuffix(entry.Name(), ".json")
				if name != config.Hostname && strings.HasSuffix(entry.Name(), ".json") {
					peers = append(peers, map[string]any{"id": name, "hostname": name, "addresses": []string{"100.100.0.1"}, "online": true})
				}
			}
			value = map[string]any{"peers": peers}
		case "stop":
			if server != nil {
				server.Close()
			}
			os.Remove(filepath.Join(bus, config.Hostname+".json"))
			json.NewEncoder(os.Stdout).Encode(map[string]any{"id": cmd.ID, "ok": true, "value": value})
			return
		default:
			panic("unsupported test command")
		}
		json.NewEncoder(os.Stdout).Encode(map[string]any{"id": cmd.ID, "ok": true, "value": value})
	}
	if server != nil {
		server.Close()
	}
	if config.Hostname != "" {
		os.Remove(filepath.Join(bus, config.Hostname+".json"))
	}
}

func proxy(w http.ResponseWriter, r *http.Request, bus string, config startInput) {
	if r.Method != "POST" || r.URL.Path != "/proxy" || r.Header.Get("Authorization") != "Bearer "+config.ProxySecret {
		http.Error(w, "unauthorized", 401)
		return
	}
	var input struct {
		PeerID string          `json:"peerId"`
		Path   string          `json:"path"`
		Body   json.RawMessage `json:"body"`
	}
	if json.NewDecoder(io.LimitReader(r.Body, 1048577)).Decode(&input) != nil || strings.ContainsAny(input.PeerID, "/\\.") {
		http.Error(w, "bad request", 400)
		return
	}
	data, err := os.ReadFile(filepath.Join(bus, input.PeerID+".json"))
	if err != nil {
		http.Error(w, "offline", 503)
		return
	}
	var peer registration
	if json.Unmarshal(data, &peer) != nil {
		http.Error(w, "invalid peer", 500)
		return
	}
	body := []byte(input.Body)
	request, err := http.NewRequestWithContext(r.Context(), "POST", peer.BackendURL+input.Path, bytes.NewReader(body))
	if err != nil {
		http.Error(w, "invalid request", 400)
		return
	}
	identity, _ := json.Marshal(map[string]string{"id": config.Hostname, "hostname": config.Hostname})
	encoded := base64.RawURLEncoding.EncodeToString(identity)
	timestamp := strconv.FormatInt(time.Now().UnixMilli(), 10)
	hash := sha256.Sum256(body)
	mac := hmac.New(sha256.New, []byte(peer.Secret))
	fmt.Fprintf(mac, "%s\n%s\nPOST\n%s\n%s", timestamp, encoded, input.Path, hex.EncodeToString(hash[:]))
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-shellfox-peer", encoded)
	request.Header.Set("x-shellfox-timestamp", timestamp)
	request.Header.Set("x-shellfox-signature", hex.EncodeToString(mac.Sum(nil)))
	response, err := (&http.Client{Timeout: 30 * time.Second}).Do(request)
	if err != nil {
		http.Error(w, "unavailable", 503)
		return
	}
	defer response.Body.Close()
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(response.StatusCode)
	io.Copy(w, response.Body)
}
