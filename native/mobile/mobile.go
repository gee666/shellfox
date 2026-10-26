// Package mobile embeds an outbound-only Tailscale node for Shellfox clients.
// Its string/error API is intentionally limited to gomobile-supported types.
package mobile

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"regexp"
	"sync"
	"time"

	"tailscale.com/client/local"
	"tailscale.com/tsnet"
)

const requestTimeout = 30 * time.Second

var singleton struct {
	sync.Mutex
	node *node
}

type node struct {
	server     *tsnet.Server
	client     *local.Client
	ctx        context.Context
	cancel     context.CancelFunc
	requests   chan struct{}
	polls      chan struct{}
	directory  string
	hostname   string
	discovery  *hostDiscovery
	transports transportPool
}

// Start opens the persistent node in an absolute, app-private directory.
// It does not wait for login, advertise a service, or open an inbound listener.
func Start(stateDirectory, hostname string) error {
	if !filepath.IsAbs(stateDirectory) || !regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9-]{0,62}$`).MatchString(hostname) {
		return errors.New("Invalid state directory or hostname")
	}
	singleton.Lock()
	defer singleton.Unlock()
	if n := singleton.node; n != nil {
		if n.directory == filepath.Clean(stateDirectory) && n.hostname == hostname {
			return nil
		}
		return errors.New("Network already started with another configuration")
	}
	if err := prepareState(stateDirectory); err != nil {
		return err
	}
	store, err := openStateStore(stateDirectory)
	if err != nil {
		return err
	}
	if err := configureRuntime(stateDirectory); err != nil {
		return err
	}
	s := &tsnet.Server{Dir: stateDirectory, Hostname: hostname, Store: store,
		ControlURL: "https://controlplane.tailscale.com",
		Logf:       func(string, ...interface{}) {}, UserLogf: func(string, ...interface{}) {}}
	if err := s.Start(); err != nil {
		_ = s.Close()
		if errors.Is(err, errInterfaces) {
			return errInterfaces
		}
		return errors.New("Cannot initialize embedded Tailscale")
	}
	client, err := s.LocalClient()
	if err != nil {
		_ = s.Close()
		return errors.New("Cannot initialize network API")
	}
	ctx, cancel := context.WithCancel(context.Background())
	singleton.node = &node{
		server: s, client: client, ctx: ctx, cancel: cancel,
		requests: make(chan struct{}, 12), polls: make(chan struct{}, 64),
		directory: filepath.Clean(stateDirectory), hostname: hostname, discovery: newHostDiscovery(store),
	}
	return nil
}

func current() (*node, error) {
	singleton.Lock()
	defer singleton.Unlock()
	if singleton.node == nil {
		return nil, errors.New("Network is stopped")
	}
	return singleton.node, nil
}

// Status returns the desktop-compatible status/loginUrl/error JSON object.
func Status() (string, error) {
	n, err := current()
	if err != nil {
		return `{"status":"disabled","loginUrl":null,"error":null}`, nil
	}
	ctx, cancel := context.WithTimeout(n.ctx, 5*time.Second)
	defer cancel()
	st, err := n.client.StatusWithoutPeers(ctx)
	if err != nil || st == nil {
		return "", errors.New("Cannot read network status")
	}
	return marshal(statusFor(st))
}

// Peers returns only untagged nodes owned by the signed-in user.
func Peers() (string, error) {
	n, err := current()
	if err != nil {
		return "", err
	}
	ctx, cancel := context.WithTimeout(n.ctx, 5*time.Second)
	defer cancel()
	st, err := n.client.Status(ctx)
	if err != nil {
		return "", errors.New("Cannot read network peers")
	}
	return marshal(struct {
		Peers []peer `json:"peers"`
	}{ownPeers(st)})
}

// Stop cancels active requests and closes the node, retaining its identity.
func Stop() error {
	singleton.Lock()
	defer singleton.Unlock()
	return stopLocked()
}

func stopLocked() error {
	n := singleton.node
	if n == nil {
		return nil
	}
	singleton.node = nil
	n.cancel()
	n.transports.close()
	if err := n.server.Close(); err != nil {
		return errors.New("Cannot stop network")
	}
	return nil
}

// Logout revokes local login and stops the node. Call Start before logging in again.
// A failed logout is reported; it never claims persisted credentials were removed.
func Logout() error {
	singleton.Lock()
	defer singleton.Unlock()
	n := singleton.node
	if n == nil {
		return nil
	}
	n.cancel()
	ctx, cancel := context.WithTimeout(context.Background(), requestTimeout)
	defer cancel()
	err := n.client.Logout(ctx)
	if err == nil {
		err = n.discovery.clear()
	}
	closeErr := stopLocked()
	if err != nil {
		return errors.New("Cannot log out of network")
	}
	return closeErr
}

func prepareState(directory string) error {
	if err := os.MkdirAll(directory, 0700); err != nil {
		return errors.New("Cannot create node directory")
	}
	info, err := os.Lstat(directory)
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return errors.New("Invalid node directory")
	}
	if err := os.Chmod(directory, 0700); err != nil {
		return errors.New("Cannot secure node directory")
	}
	return nil
}

func marshal(value interface{}) (string, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return "", errors.New("Cannot encode network response")
	}
	return string(data), nil
}
