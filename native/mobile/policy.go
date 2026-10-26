package mobile

import (
	"net/netip"
	"net/url"
	"sort"

	"tailscale.com/ipn/ipnstate"
)

// Keep these restrictions aligned with native/tailscale/helper.go and bridge.go.
// Mobile never accepts a URL, port, IP address, method, or caller-supplied headers.
const servicePort = "52743"
const bodyLimit = 1024 * 1024
const responseLimit = 8 * 1024 * 1024

type peer struct {
	ID        string   `json:"id"`
	Hostname  string   `json:"hostname"`
	Addresses []string `json:"addresses"`
	Online    bool     `json:"online"`
}

func allowedPath(path string) bool {
	switch path {
	case "/v1/discover", "/v1/pair", "/v1/request", "/v1/events", "/v1/disconnect":
		return true
	}
	return false
}

func tailnetAddress(ip netip.Addr) bool {
	return netip.MustParsePrefix("100.64.0.0/10").Contains(ip) || netip.MustParsePrefix("fd7a:115c:a1e0::/48").Contains(ip)
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
		if len(addresses) > 0 {
			result = append(result, peer{string(p.ID), p.HostName, addresses, p.Online})
		}
	}
	sort.Slice(result, func(i, j int) bool { return result[i].ID < result[j].ID })
	return result
}

type networkStatus struct {
	Status   string  `json:"status"`
	LoginURL *string `json:"loginUrl"`
	Error    *string `json:"error"`
}

func safeLoginURL(raw string) bool {
	u, err := url.Parse(raw)
	return err == nil && u.Scheme == "https" && u.Hostname() == "login.tailscale.com" && u.User == nil && u.Port() == ""
}

func statusFor(st *ipnstate.Status) networkStatus {
	result := networkStatus{Status: "starting"}
	switch st.BackendState {
	case "Running":
		result.Status = "online"
	case "NeedsLogin", "NeedsMachineAuth", "Stopped":
		result.Status = "login-required"
	case "NoState", "Starting":
	default:
		message := "Network unavailable"
		result.Status, result.Error = "error", &message
	}
	if result.Status == "login-required" && safeLoginURL(st.AuthURL) {
		result.LoginURL = &st.AuthURL
	}
	return result
}
