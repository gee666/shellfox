package mobile

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"strings"
	"sync"
	"time"

	"tailscale.com/ipn/ipnstate"
	"tailscale.com/tailcfg"
	"tailscale.com/types/key"
)

// Routes include both ends' identities. An address alone is not an identity.
type peerRoute struct {
	selfID, peerID            tailcfg.StableNodeID
	selfKey, peerKey, mapKey  key.NodePublic
	owner                     tailcfg.UserID
	address, selfIPs, peerIPs string
}
type pooledTransport struct {
	route peerRoute
	http  *http.Transport
}
type transportPool struct {
	sync.Mutex
	peers  map[string]pooledTransport
	closed bool
}

func (p *transportPool) clearLocked() {
	for _, entry := range p.peers {
		entry.http.CloseIdleConnections()
	}
	p.peers = nil
}
func (p *transportPool) close() {
	p.Lock()
	defer p.Unlock()
	p.closed = true
	p.clearLocked()
}

func authorizedRoute(st *ipnstate.Status, peerID string) (peerRoute, bool) {
	for _, peer := range ownPeers(st) {
		if peer.ID != peerID || !peer.Online {
			continue
		}
		for mapKey, raw := range st.Peer {
			if raw != nil && string(raw.ID) == peerID {
				return peerRoute{
					selfID: st.Self.ID, selfKey: st.Self.PublicKey, owner: st.Self.UserID,
					peerID: raw.ID, peerKey: raw.PublicKey, mapKey: mapKey,
					address: net.JoinHostPort(peer.Addresses[0], servicePort),
					selfIPs: fmt.Sprint(st.Self.TailscaleIPs), peerIPs: strings.Join(peer.Addresses, ","),
				}, true
			}
		}
	}
	return peerRoute{}, false
}

func (p *transportPool) transport(ctx context.Context, client peerClient, dial dialFunc, peerID string) (*http.Transport, string, error) {
	// Serialize fresh status reads with selection so concurrent requests cannot
	// restore an older route after a newer ownership map has been checked.
	p.Lock()
	defer p.Unlock()
	if p.closed {
		return nil, "", errors.New("Network is stopped")
	}
	st, err := client.Status(ctx)
	if err != nil {
		p.clearLocked()
		return nil, "", errors.New("Network unavailable")
	}
	route, ok := authorizedRoute(st, peerID)
	if !ok {
		p.clearLocked()
		return nil, "", errors.New("Peer unavailable or not owned by current user")
	}
	if previous, ok := p.peers[peerID]; ok {
		if previous.route == route {
			return previous.http, route.address, nil
		}
		previous.http.CloseIdleConnections()
		delete(p.peers, peerID)
	}
	// Bound idle resources even when discovery presents many different hosts.
	if len(p.peers) >= 64 {
		p.clearLocked()
	}
	transport := &http.Transport{
		Proxy: nil, DisableCompression: true,
		ResponseHeaderTimeout: requestTimeout, MaxResponseHeaderBytes: 16 * 1024,
		// Retire idle sockets before the helper's 30s keep-alive deadline.
		MaxIdleConns: 4, MaxIdleConnsPerHost: 4, IdleConnTimeout: 15 * time.Second,
		DialContext: func(ctx context.Context, network, target string) (net.Conn, error) {
			if network != "tcp" || target != route.address {
				return nil, errors.New("Invalid tailnet target")
			}
			return dial(ctx, "tcp", route.address)
		},
	}
	if p.peers == nil {
		p.peers = make(map[string]pooledTransport)
	}
	p.peers[peerID] = pooledTransport{route: route, http: transport}
	return transport, route.address, nil
}
