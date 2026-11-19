// A loopback-only Tailscale control plane and relay for simulator integration tests.
package main

import (
	"context"
	"fmt"
	"net"
	"net/http/httptest"
	"os"
	"os/signal"
	"strconv"
	"syscall"

	"tailscale.com/derp/derpserver"
	"tailscale.com/tailcfg"
	"tailscale.com/tstest/integration/testcontrol"
	"tailscale.com/types/key"
	"tailscale.com/types/logger"
)

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: testcontrol PORT")
		os.Exit(2)
	}
	port, err := strconv.ParseUint(os.Args[1], 10, 16)
	if err != nil || port == 0 {
		fmt.Fprintln(os.Stderr, "invalid fixture port")
		os.Exit(2)
	}
	listener, err := net.Listen("tcp4", fmt.Sprintf("127.0.0.1:%d", port))
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	relay := derpserver.New(key.NewNode(), logger.Discard)
	defer relay.Close()
	derp := httptest.NewTLSServer(derpserver.Handler(relay))
	defer derp.Close()
	control := &testcontrol.Server{
		AllNodesSameUser: true,
		Logf:             logger.Discard,
		DERPMap: &tailcfg.DERPMap{Regions: map[tailcfg.DERPRegionID]*tailcfg.DERPRegion{1: {
			RegionID: 1, RegionCode: "shellfox-ci", Nodes: []*tailcfg.DERPNode{{
				Name: "local", RegionID: 1, HostName: "127.0.0.1", IPv4: "127.0.0.1", IPv6: "none",
				STUNPort: -1, DERPPort: derp.Listener.Addr().(*net.TCPAddr).Port, InsecureForTests: true,
			}},
		}}},
	}
	server := httptest.NewUnstartedServer(control)
	_ = server.Listener.Close()
	server.Listener = listener
	control.HTTPTestServer = server
	server.Start()
	defer server.Close()
	fmt.Println("SHELLFOX_TESTCONTROL_READY", server.URL)
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	<-ctx.Done()
	server.CloseClientConnections()
}
