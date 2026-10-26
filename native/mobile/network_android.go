package mobile

import "tailscale.com/net/netmon"

func init() { netmon.RegisterInterfaceGetter(readInterfaces) }

func updateDefaultRoute(name, gateway string) {
	netmon.UpdateLastKnownDefaultRouteInterface(name)
	netmon.UpdateLastKnownDefaultGateway(gateway)
}
