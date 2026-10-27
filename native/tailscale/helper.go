package main

import (
	"context"
	"errors"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"

	"shellfox.local/tailscale-helper/nethealth"
	"tailscale.com/client/local"
	"tailscale.com/ipn/ipnstate"
	"tailscale.com/tsnet"
)

const servicePort = "52743"
const bodyLimit = 1024 * 1024
const responseLimit = 8 * 1024 * 1024
const requestTimeout = 30 * time.Second

type startConfig struct {
	StateDirectory string `json:"stateDirectory"`
	Hostname       string `json:"hostname"`
	AuthKey        string `json:"authKey,omitempty"`
	BackendURL     string `json:"backendUrl"`
	BackendSecret  string `json:"backendSecret"`
	ProxySecret    string `json:"proxySecret"`
}

type networkStatus = nethealth.Status

type peer struct {
	ID        string   `json:"id"`
	Hostname  string   `json:"hostname"`
	Addresses []string `json:"addresses"`
	Online    bool     `json:"online"`
}

type helper struct {
	server   *tsnet.Server
	client   *local.Client
	proxy    *http.Server
	incoming *http.Server
	cancel   context.CancelFunc
	workers  sync.WaitGroup
	health   *nethealth.Monitor
	emit     func(networkStatus)
}

func (c startConfig) validate() (*url.URL, error) {
	if !filepath.IsAbs(c.StateDirectory) || !regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9-]{0,62}$`).MatchString(c.Hostname) {
		return nil, errors.New("State directory must be absolute and hostname must be a DNS label")
	}
	if !validSecret(c.BackendSecret) || !validSecret(c.ProxySecret) || c.BackendSecret == c.ProxySecret || !validAuthKey(c.AuthKey) {
		return nil, errors.New("Invalid helper secrets")
	}
	u, err := url.Parse(c.BackendURL)
	if err != nil || u.Scheme != "http" || u.User != nil || u.RawQuery != "" || u.Fragment != "" || u.RawPath != "" || (u.Path != "" && u.Path != "/") || u.Port() == "" {
		return nil, errors.New("Backend must be a loopback HTTP origin with an explicit port")
	}
	ip, err := netip.ParseAddr(u.Hostname())
	if err != nil || !ip.IsLoopback() || ip.Is4In6() {
		return nil, errors.New("Backend must use a literal loopback address")
	}
	if _, err := net.LookupPort("tcp", u.Port()); err != nil || u.Port() == "0" {
		return nil, errors.New("Invalid backend port")
	}
	return u, nil
}

func validSecret(s string) bool {
	if len(s) < 32 || len(s) > 256 {
		return false
	}
	for _, r := range s {
		if r < 33 || r > 126 {
			return false
		}
	}
	return true
}

func prepareState(directory string) error {
	if err := os.MkdirAll(directory, 0700); err != nil {
		return errors.New("Cannot create state directory")
	}
	info, err := os.Lstat(directory)
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return errors.New("Invalid state directory")
	}
	if err := protectStatePath(directory, true); err != nil {
		return errors.New("Cannot secure state directory")
	}
	return nil
}

func httpServer(handler http.Handler) *http.Server {
	return &http.Server{Handler: handler, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: requestTimeout,
		WriteTimeout: requestTimeout + 5*time.Second, IdleTimeout: 30 * time.Second, MaxHeaderBytes: 16 * 1024}
}

func (h *helper) start(c startConfig) (interface{}, error) {
	backend, err := c.validate()
	if err != nil {
		return nil, err
	}
	if err := prepareState(c.StateDirectory); err != nil {
		return nil, err
	}
	state, err := openStateStore(c.StateDirectory)
	if err != nil {
		return nil, err
	}
	s := &tsnet.Server{Dir: c.StateDirectory, Hostname: c.Hostname, Store: state,
		ControlURL: "https://controlplane.tailscale.com", Ephemeral: false,
		UserLogf: func(string, ...interface{}) {}, Logf: func(string, ...interface{}) {}}
	h.server = s
	fail := func(phase string) (interface{}, error) {
		h.close()
		return nil, errors.New("Cannot start embedded Tailscale: " + phase)
	}
	if err := s.Start(); err != nil {
		return fail("network initialization")
	}
	client, err := s.LocalClient()
	if err != nil {
		return fail("local API")
	}
	h.client = client
	if c.AuthKey != "" {
		ctx, cancel := context.WithTimeout(context.Background(), requestTimeout)
		err := enroll(ctx, client, c.AuthKey)
		cancel()
		if err != nil {
			return fail("enrollment")
		}
	}
	tailListener, err := s.Listen("tcp", ":"+servicePort)
	if err != nil {
		return fail("tailnet listener")
	}
	loopListener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		_ = tailListener.Close()
		return fail("loopback listener")
	}
	bridge := &bridge{network: client, dial: s.Dial, backend: backend, backendSecret: c.BackendSecret, proxySecret: c.ProxySecret}
	h.proxy = httpServer(http.HandlerFunc(bridge.outgoing))
	h.incoming = httpServer(http.HandlerFunc(bridge.incoming))
	ctx, cancel := context.WithCancel(context.Background())
	h.cancel = cancel
	h.health, err = nethealth.New(nethealth.ServerSource(s, client), nethealth.DefaultConfig(), h.emit)
	if err != nil {
		_ = tailListener.Close()
		_ = loopListener.Close()
		return fail("network health")
	}
	h.workers.Add(3)
	go h.serve(h.proxy, loopListener)
	go h.serve(h.incoming, tailListener)
	go func() {
		defer h.workers.Done()
		h.health.Run(ctx)
	}()
	return map[string]string{"proxyUrl": "http://" + loopListener.Addr().String()}, nil
}

func (h *helper) serve(server *http.Server, listener net.Listener) {
	defer h.workers.Done()
	if err := server.Serve(listener); err != nil && err != http.ErrServerClosed {
		message := "Network listener stopped"
		h.emit(networkStatus{Status: "error", Error: &message})
	}
}

func (h *helper) close() {
	if h.cancel != nil {
		h.cancel()
	}
	if h.proxy != nil {
		_ = h.proxy.Close()
	}
	if h.incoming != nil {
		_ = h.incoming.Close()
	}
	h.workers.Wait()
	if h.server != nil {
		_ = h.server.Close()
	}
	h.proxy, h.incoming, h.server, h.client, h.cancel, h.health = nil, nil, nil, nil, nil, nil
}

func (h *helper) status(ctx context.Context) networkStatus {
	if h.health == nil {
		message := "Network health is unavailable."
		return networkStatus{Status: "error", Error: &message}
	}
	return h.health.Status(ctx)
}

func ownPeers(st *ipnstate.Status) []peer {
	result := []peer{}
	if st == nil || st.Self == nil || st.Self.UserID == 0 || st.BackendState != "Running" {
		return result
	}
	for _, p := range st.Peer {
		if p == nil || p.ID == "" || p.ID == st.Self.ID || p.UserID != st.Self.UserID || (p.Tags != nil && p.Tags.Len() != 0) {
			continue
		}
		addresses := []string{}
		for _, ip := range p.TailscaleIPs {
			if tailnetAddress(ip) {
				addresses = append(addresses, ip.String())
			}
		}
		if len(addresses) != 0 {
			result = append(result, peer{string(p.ID), p.HostName, addresses, p.Online})
		}
	}
	sort.Slice(result, func(i, j int) bool { return strings.Compare(result[i].ID, result[j].ID) < 0 })
	return result
}

func tailnetAddress(ip netip.Addr) bool {
	return netip.MustParsePrefix("100.64.0.0/10").Contains(ip) || netip.MustParsePrefix("fd7a:115c:a1e0::/48").Contains(ip)
}
