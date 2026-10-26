package mobile

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strconv"
	"sync"
	"time"

	"tailscale.com/ipn"
	"tailscale.com/ipn/ipnstate"
)

const (
	discoveryTimeout                  = 2 * time.Second
	discoveryPositiveTTL              = 5 * time.Minute
	discoveryNegativeTTL              = 30 * time.Second
	discoveryLimit                    = 1000
	discoveryStateKey    ipn.StateKey = "shellfox-hosts-v1"
	discoverySignature                = `{"ok":true,"value":{"service":"shellfox","protocolVersion":1}}`
	legacySignature                   = `{"ok":false,"error":{"code":"VALIDATION","message":"Remote request could not be completed.","retryable":true}}`
)

type hostEntry struct {
	confirmed bool
	next      time.Time
}
type hostSnapshot struct {
	Peers       []peer `json:"peers"`
	Discovering bool   `json:"discovering"`
}
type confirmedHosts struct {
	Owner string   `json:"owner"`
	IDs   []string `json:"ids"`
}
type hostDiscovery struct {
	mu         sync.Mutex
	store      ipn.StateStore
	owner      string
	entries    map[string]hostEntry
	pending    int
	generation uint64
	cancel     context.CancelFunc
	slots      chan struct{}
}

func newHostDiscovery(store ipn.StateStore) *hostDiscovery {
	return &hostDiscovery{store: store, entries: map[string]hostEntry{}, slots: make(chan struct{}, 4)}
}

// Hosts returns confirmed hosts without waiting for probes. Poll again while
// discovering is true. Cached IDs never bypass the current same-owner peer map.
func Hosts() (string, error) {
	n, err := current()
	if err != nil {
		return "", err
	}
	ctx, cancel := context.WithTimeout(n.ctx, 5*time.Second)
	defer cancel()
	st, err := n.client.Status(ctx)
	if err != nil {
		return "", errors.New("Cannot read network hosts")
	}
	return marshal(n.discovery.snapshot(n.ctx, st, n.client, n.server.Dial, time.Now()))
}

func (d *hostDiscovery) snapshot(ctx context.Context, st *ipnstate.Status, client peerClient, dial dialFunc, now time.Time) hostSnapshot {
	d.mu.Lock()
	defer d.mu.Unlock()
	result := hostSnapshot{Peers: []peer{}}
	if st == nil || st.Self == nil || st.Self.ID == "" || st.Self.UserID == 0 || st.BackendState != "Running" {
		if d.cancel != nil {
			d.cancel()
		}
		d.generation++
		d.pending = 0
		return result
	}
	owner := string(st.Self.ID) + "/" + strconv.FormatInt(int64(st.Self.UserID), 10)
	if d.owner != owner {
		if d.cancel != nil {
			d.cancel()
		}
		d.generation++
		d.owner, d.pending = owner, 0
		d.entries = map[string]hostEntry{}
		d.loadLocked()
	}
	peers := ownPeers(st)
	if len(peers) > discoveryLimit {
		peers = peers[:discoveryLimit]
	}
	present := make(map[string]bool, len(peers))
	candidates := []string{}
	for _, p := range peers {
		present[p.ID] = true
		entry := d.entries[p.ID]
		if entry.confirmed {
			result.Peers = append(result.Peers, p)
		}
		if d.pending == 0 && p.Online && !now.Before(entry.next) {
			candidates = append(candidates, p.ID)
			if _, exists := d.entries[p.ID]; !exists {
				d.entries[p.ID] = hostEntry{}
			}
		}
	}
	changed := false
	for id, entry := range d.entries {
		if !present[id] {
			delete(d.entries, id)
			changed = changed || entry.confirmed
		}
	}
	if changed {
		d.saveLocked()
	}
	if d.pending == 0 && len(candidates) > 0 && ctx.Err() == nil {
		var scan context.Context
		scan, d.cancel = context.WithCancel(ctx)
		d.pending = len(candidates)
		go d.scan(scan, d.generation, candidates, client, dial)
	}
	result.Discovering = d.pending > 0
	return result
}

func (d *hostDiscovery) scan(ctx context.Context, generation uint64, candidates []string, client peerClient, dial dialFunc) {
	jobs := make(chan string)
	var workers sync.WaitGroup
	for range 4 {
		workers.Go(func() {
			for id := range jobs {
				confirmed := false
				select {
				case d.slots <- struct{}{}:
					if ctx.Err() == nil {
						confirmed = probeHost(ctx, client, dial, id)
					}
					<-d.slots
				case <-ctx.Done():
				}
				d.mu.Lock()
				if d.generation == generation {
					entry, exists := d.entries[id]
					// A peer removed from the current map cannot be restored by a late probe.
					if exists && ctx.Err() == nil {
						wasConfirmed := entry.confirmed
						entry.confirmed = wasConfirmed || confirmed
						ttl := discoveryNegativeTTL
						if confirmed {
							ttl = discoveryPositiveTTL
						}
						entry.next = time.Now().Add(ttl)
						d.entries[id] = entry
						if confirmed && !wasConfirmed {
							d.saveLocked()
						}
					}
					d.pending--
				}
				d.mu.Unlock()
			}
		})
	}
	for _, id := range candidates {
		jobs <- id
	}
	close(jobs)
	workers.Wait()
}

func probeHost(parent context.Context, client peerClient, dial dialFunc, id string) bool {
	ctx, cancel := context.WithTimeout(parent, discoveryTimeout)
	defer cancel()
	data, status, err := postPeer(ctx, client, dial, id, "/v1/discover", "{}", 4096)
	if err != nil {
		return false
	}
	if status == 200 {
		return matchesSignature(data, discoverySignature)
	}
	// Old helpers reject /discover before forwarding. The old host validates {}
	// before approval/pending/client operations. Never send a client ID or pair.
	if status != 404 {
		return false
	}
	data, status, err = postPeer(ctx, client, dial, id, "/v1/request", "{}", 4096)
	return err == nil && status == 200 && matchesSignature(data, legacySignature)
}

func matchesSignature(data, signature string) bool {
	var actual, expected interface{}
	return json.Unmarshal([]byte(data), &actual) == nil && json.Unmarshal([]byte(signature), &expected) == nil && reflect.DeepEqual(actual, expected)
}

func (d *hostDiscovery) loadLocked() {
	if d.store == nil {
		return
	}
	data, err := d.store.ReadState(discoveryStateKey)
	if err != nil || len(data) > 300*1024 {
		return
	}
	var saved confirmedHosts
	if json.Unmarshal(data, &saved) != nil || saved.Owner != d.owner || len(saved.IDs) > discoveryLimit {
		return
	}
	for _, id := range saved.IDs {
		if id == "" || len(id) > 256 {
			return
		}
	}
	for _, id := range saved.IDs {
		d.entries[id] = hostEntry{confirmed: true}
	}
}

func (d *hostDiscovery) saveLocked() {
	if d.store == nil {
		return
	}
	saved := confirmedHosts{Owner: d.owner, IDs: []string{}}
	for id, entry := range d.entries {
		if entry.confirmed {
			saved.IDs = append(saved.IDs, id)
		}
	}
	data, err := json.Marshal(saved)
	if err == nil {
		_ = d.store.WriteState(discoveryStateKey, data)
	}
}

func (d *hostDiscovery) clear() error {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.cancel != nil {
		d.cancel()
	}
	d.generation++
	d.owner, d.pending = "", 0
	d.entries = map[string]hostEntry{}
	if d.store != nil {
		if err := d.store.WriteState(discoveryStateKey, nil); err != nil {
			return errors.New("Cannot clear network hosts")
		}
	}
	return nil
}
