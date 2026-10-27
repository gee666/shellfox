// Package nethealth observes embedded Tailscale connectivity without probing peers.
package nethealth

import (
	"net/url"

	"tailscale.com/health"
	"tailscale.com/ipn/ipnstate"
	"tailscale.com/tsconst"
)

type Status struct {
	Status   string  `json:"status"`
	LoginURL *string `json:"loginUrl"`
	Error    *string `json:"error"`
}

type LinkState uint8

const (
	LinkUnknown LinkState = iota
	LinkUp
	LinkDown
)

type Assessment struct {
	Status
	NetworkUp   bool
	Eligible    bool
	ControlLost bool
}

func SafeLoginURL(raw string) bool {
	u, err := url.Parse(raw)
	return err == nil && u.Scheme == "https" && u.Hostname() == "login.tailscale.com" && u.User == nil && u.Port() == ""
}

func failed(message string) Status { return Status{Status: "error", Error: &message} }

// Inspect uses structured SDK warnings, never their potentially sensitive text.
// Online means a live control stream. Error can describe degraded relay/socket
// paths without tearing down working direct connections.
func Inspect(st *ipnstate.Status, state *health.State, link LinkState) Assessment {
	// Missing mobile interface enumeration is not proof of an outage.
	a := Assessment{Status: Status{Status: "starting"}, NetworkUp: link != LinkDown}
	if st == nil {
		a.Status = failed("Cannot read embedded Tailscale connectivity. Retrying the health check.")
		return a
	}
	switch st.BackendState {
	case "NeedsLogin", "NeedsMachineAuth":
		a.Status.Status = "login-required"
		if SafeLoginURL(st.AuthURL) {
			a.LoginURL = &st.AuthURL
		}
		return a
	case "Stopped":
		a.Status = failed("Embedded Tailscale is stopped.")
		return a
	case "Running", "Starting":
	case "NoState":
		return a
	default:
		a.Status = failed("Embedded Tailscale is unavailable. Retrying the health check.")
		return a
	}
	a.ControlLost = st.Self == nil || !st.Self.Online
	transport, otherConnectivity := false, false
	if state != nil {
		for code, warning := range state.Warnings {
			switch code {
			case tsconst.HealthWarnableNotInMapPoll, tsconst.HealthWarnableMapResponseTimeout:
				a.ControlLost = true
			case tsconst.HealthWarnableNetworkStatus:
				a.NetworkUp = false
			case tsconst.HealthWarnableNoDERPHome, tsconst.HealthWarnableNoDERPConnection,
				tsconst.HealthWarnableDERPTimedOut, tsconst.HealthWarnableNoUDP4Bind,
				tsconst.HealthWarnableMagicsockReceiveFuncError:
				transport = true
			default:
				otherConnectivity = otherConnectivity || warning.ImpactsConnectivity
			}
		}
	}
	if !a.NetworkUp {
		a.Status = failed("Network connection unavailable. Tailscale will reconnect automatically.")
	} else if a.ControlLost {
		a.Status = failed("Tailscale control connection unavailable. Reconnecting automatically.")
	} else if st.BackendState == "Running" {
		a.Status.Status = "online"
		if transport {
			message := "Tailscale relay or socket connectivity is degraded. Repair is retrying automatically."
			a.Error = &message
		} else if otherConnectivity {
			message := "Tailscale reports degraded connectivity."
			a.Error = &message
		}
	}
	// Unknown control-plane warnings can require user action. Do not repeatedly
	// restart authentication for those or for stopped/unauthorized nodes.
	a.Eligible = st.HaveNodeKey && a.NetworkUp && (a.ControlLost || transport)
	return a
}
