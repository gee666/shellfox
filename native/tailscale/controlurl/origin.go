// Package controlurl validates the explicit, local-only control-server setting.
package controlurl

import (
	"errors"
	"net"
	"net/netip"
	"net/url"
	"strconv"
	"strings"
)

// ParseLoopback validates an explicit local control origin, not a peer
// request URL. Invalid configuration never selects a fallback.
func ParseLoopback(value string) (string, error) {
	invalid := func() (string, error) { return "", errors.New("Control server must be a literal loopback HTTP origin") }
	if len(value) > 128 || !strings.HasPrefix(value, "http://") || strings.ContainsAny(value, "?#%") {
		return invalid()
	}
	u, err := url.Parse(value)
	if err != nil || u.Scheme != "http" || u.Opaque != "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" || u.RawPath != "" || (u.Path != "" && u.Path != "/") {
		return invalid()
	}
	ip, err := netip.ParseAddr(u.Hostname())
	if err != nil || !ip.IsLoopback() || ip.Is4In6() || ip.Zone() != "" {
		return invalid()
	}
	port, err := strconv.ParseUint(u.Port(), 10, 16)
	if err != nil || port == 0 {
		return invalid()
	}
	return "http://" + net.JoinHostPort(ip.String(), strconv.FormatUint(port, 10)), nil
}
