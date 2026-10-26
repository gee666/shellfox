package mobile

import (
	"encoding/json"
	"errors"
	"net"
	"net/netip"
	"sync"

	"tailscale.com/net/netmon"
)

// NetworkProvider supplies OS interface metadata through the Android SDK.
// Android SDK 30+ prohibits Go's netlink-based net.Interfaces.
type NetworkProvider interface {
	Interfaces() (string, error)
}

var networkProvider struct {
	sync.RWMutex
	value NetworkProvider
}
var errInterfaces = errors.New("Cannot read Android network interfaces")

// SetNetworkProvider is native bridge setup, not part of the JS network API.
func SetNetworkProvider(provider NetworkProvider) {
	networkProvider.Lock()
	networkProvider.value = provider
	networkProvider.Unlock()
}

// NetworkChanged asks tsnet to recheck interfaces after an OS network change.
func NetworkChanged() {
	n, err := current()
	if err != nil {
		return
	}
	if monitor, ok := n.server.Sys().NetMon.GetOK(); ok {
		monitor.InjectEvent()
	}
}

type interfaceInfo struct {
	Name         string   `json:"name"`
	Index        int      `json:"index"`
	MTU          int      `json:"mtu"`
	Up           bool     `json:"up"`
	Loopback     bool     `json:"loopback"`
	PointToPoint bool     `json:"pointToPoint"`
	Multicast    bool     `json:"multicast"`
	Addresses    []string `json:"addresses"`
}

type interfaceSnapshot struct {
	Interfaces       []interfaceInfo `json:"interfaces"`
	DefaultInterface string          `json:"defaultInterface"`
	Gateway          string          `json:"gateway"`
}

func readInterfaces() ([]netmon.Interface, error) {
	networkProvider.RLock()
	provider := networkProvider.value
	networkProvider.RUnlock()
	if provider == nil {
		return nil, errInterfaces
	}
	raw, err := provider.Interfaces()
	if err != nil {
		return nil, errInterfaces
	}
	snapshot, result, err := decodeInterfaces(raw)
	if err != nil {
		return nil, err
	}
	updateDefaultRoute(snapshot.DefaultInterface, snapshot.Gateway)
	return result, nil
}

func decodeInterfaces(raw string) (interfaceSnapshot, []netmon.Interface, error) {
	var snapshot interfaceSnapshot
	if len(raw) > 256*1024 || json.Unmarshal([]byte(raw), &snapshot) != nil || len(snapshot.Interfaces) > 256 || len(snapshot.DefaultInterface) > 128 {
		return snapshot, nil, errInterfaces
	}
	if snapshot.Gateway != "" {
		if _, err := netip.ParseAddr(snapshot.Gateway); err != nil {
			return snapshot, nil, errInterfaces
		}
	}
	result := make([]netmon.Interface, 0, len(snapshot.Interfaces))
	for _, item := range snapshot.Interfaces {
		if item.Name == "" || len(item.Name) > 128 || item.Index < 0 || item.MTU < 0 || item.MTU > 1024*1024 || len(item.Addresses) > 128 {
			return snapshot, nil, errInterfaces
		}
		flags := net.Flags(0)
		if item.Up {
			flags |= net.FlagUp | net.FlagRunning
		}
		if item.Loopback {
			flags |= net.FlagLoopback
		}
		if item.PointToPoint {
			flags |= net.FlagPointToPoint
		}
		if item.Multicast {
			flags |= net.FlagMulticast
		}
		addresses := make([]net.Addr, 0, len(item.Addresses))
		for _, address := range item.Addresses {
			prefix, err := netip.ParsePrefix(address)
			if err != nil || prefix.Addr().Zone() != "" {
				return snapshot, nil, errInterfaces
			}
			bits := 128
			if prefix.Addr().Is4() {
				bits = 32
			}
			addresses = append(addresses, &net.IPNet{IP: prefix.Addr().AsSlice(), Mask: net.CIDRMask(prefix.Bits(), bits)})
		}
		result = append(result, netmon.Interface{Interface: &net.Interface{Name: item.Name, Index: item.Index, MTU: item.MTU, Flags: flags}, AltAddrs: addresses})
	}
	return snapshot, result, nil
}
