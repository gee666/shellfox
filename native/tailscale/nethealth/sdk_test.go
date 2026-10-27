package nethealth

import (
	"context"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"sync/atomic"
	"testing"
	"time"

	"tailscale.com/client/local"
	"tailscale.com/derp/derpserver"
	"tailscale.com/ipn"
	"tailscale.com/net/netns"
	"tailscale.com/net/stun/stuntest"
	"tailscale.com/tailcfg"
	"tailscale.com/tsnet"
	"tailscale.com/tstest/integration/testcontrol"
	"tailscale.com/types/key"
	"tailscale.com/types/logger"
)

func localRelay(t *testing.T) *tailcfg.DERPMap {
	t.Helper()
	d := derpserver.New(key.NewNode(), logger.Discard)
	httpServer := httptest.NewTLSServer(derpserver.Handler(d))
	stunAddr, closeSTUN := stuntest.Serve(t)
	t.Cleanup(func() { d.Close(); httpServer.Close(); closeSTUN() })
	return &tailcfg.DERPMap{Regions: map[tailcfg.DERPRegionID]*tailcfg.DERPRegion{1: {
		RegionID: 1, RegionCode: "isolated-test", Nodes: []*tailcfg.DERPNode{{
			Name: "local", RegionID: 1, HostName: "127.0.0.1", IPv4: "127.0.0.1", IPv6: "none",
			STUNPort: stunAddr.Port, DERPPort: httpServer.Listener.Addr().(*net.TCPAddr).Port,
			InsecureForTests: true, STUNTestIP: "127.0.0.1",
		}},
	}}}
}

type observedSDK struct {
	*local.Client
	starts     atomic.Int32
	restore    func()
	badOptions atomic.Bool
}

func (c *observedSDK) Start(ctx context.Context, options ipn.Options) error {
	c.starts.Add(1)
	if !reflect.DeepEqual(options, ipn.Options{}) {
		c.badOptions.Store(true)
	}
	c.restore()
	return c.Client.Start(ctx, options)
}

// This uses real tsnet and its local control API, but only a loopback control
// server and loopback DERP/STUN. It never enrolls into a real tailnet or flaps OS Wi-Fi.
func TestRealSDKRetainedControlRestartKeepsNodeAndListeners(t *testing.T) {
	for _, key := range []string{"TS_AUTH_KEY", "TS_AUTHKEY", "TS_CLIENT_SECRET", "TS_CLIENT_ID", "TS_ID_TOKEN", "TS_AUDIENCE"} {
		t.Setenv(key, "")
	}
	t.Setenv("TS_NO_LOGS_NO_SUPPORT", "true")
	netns.SetEnabled(false)
	defer netns.SetEnabled(true)
	root, err := filepath.Abs(filepath.Join("..", "..", "..", "tmp"))
	if err != nil {
		t.Fatal(err)
	}
	if err = os.MkdirAll(root, 0700); err != nil {
		t.Fatal(err)
	}
	directory, err := os.MkdirTemp(root, "tsnet-recovery-")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(directory)
	t.Setenv("TS_LOGS_DIR", directory)
	for _, name := range []string{"TMP", "TEMP", "TMPDIR"} {
		t.Setenv(name, root)
	}
	var down atomic.Bool
	var streams atomic.Int32
	lost := make(chan struct{})
	control := &testcontrol.Server{DERPMap: localRelay(t), AllNodesSameUser: true, Logf: func(string, ...any) {}}
	control.MaybeRejectRequest = func(*http.Request) (int, string, string) {
		if down.Load() {
			return http.StatusServiceUnavailable, "", "simulated control outage"
		}
		return 0, "", ""
	}
	control.AltMapStream = func(ctx context.Context, w testcontrol.MapStreamWriter, req *tailcfg.MapRequest) {
		response, err := control.MapResponse(req)
		if err != nil || response == nil {
			return
		}
		if err = w.SendMapMessage(response); err != nil || !req.Stream {
			return
		}
		if streams.Add(1) == 1 {
			select {
			case <-lost:
			case <-ctx.Done():
			}
		} else {
			<-ctx.Done()
		}
	}
	control.HTTPTestServer = httptest.NewUnstartedServer(control)
	control.HTTPTestServer.Start()
	defer control.HTTPTestServer.Close()
	server := &tsnet.Server{Dir: directory, Hostname: "isolated-recovery", ControlURL: control.HTTPTestServer.URL,
		Logf: func(string, ...any) {}, UserLogf: func(string, ...any) {}}
	if err = server.Start(); err != nil {
		t.Fatal(err)
	}
	defer server.Close() // Test cleanup only; automatic recovery never closes the server.
	client, err := server.LocalClient()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 12*time.Second)
	defer cancel()
	wait(t, func() bool {
		st, err := client.StatusWithoutPeers(ctx)
		return err == nil && st.Self != nil && st.Self.Online && st.BackendState == "Running"
	})
	before, err := client.StatusWithoutPeers(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if before.Self.ID == "" || before.Self.PublicKey.IsZero() || len(before.TailscaleIPs) == 0 {
		t.Fatal("fixture has no verifiable node identity")
	}
	prefsBefore, err := client.GetPrefs(ctx)
	if err != nil {
		t.Fatal(err)
	}
	listener, err := server.Listen("tcp", ":52743")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			go func(conn net.Conn) {
				defer conn.Close()
				_ = conn.SetDeadline(time.Now().Add(2 * time.Second))
				_, _ = io.CopyN(conn, conn, 1)
			}(conn)
		}
	}()
	echo := func() {
		t.Helper()
		conn, err := server.Dial(ctx, "tcp", net.JoinHostPort(before.TailscaleIPs[0].String(), "52743"))
		if err != nil {
			t.Fatal(err)
		}
		defer conn.Close()
		_ = conn.SetDeadline(time.Now().Add(2 * time.Second))
		if _, err = conn.Write([]byte{'x'}); err != nil {
			t.Fatal(err)
		}
		var result [1]byte
		if _, err = io.ReadFull(conn, result[:]); err != nil || result[0] != 'x' {
			t.Fatalf("listener failed: %v", err)
		}
	}
	echo()
	proxy := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { _, _ = io.WriteString(w, "stable proxy") }))
	defer proxy.Close()
	proxyURL := proxy.URL
	down.Store(true)
	close(lost)
	wait(t, func() bool {
		st, err := client.StatusWithoutPeers(ctx)
		return err == nil && st.Self != nil && !st.Self.Online
	})
	broken, err := client.StatusWithoutPeers(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if broken.BackendState != "Running" {
		t.Fatalf("fixture did not reproduce Running while control was lost: %s", broken.BackendState)
	}
	t.Log("reproduced SDK BackendState=Running with Self.Online=false")
	echo() // A control outage alone need not break the existing data path.
	observed := &observedSDK{Client: client, restore: func() { down.Store(false) }}
	source := ServerSource(server, observed)
	// Only control is being flapped. Do not depend on CI having a physical uplink.
	source.NetworkState = func() LinkState { return LinkUp }
	config := Config{CheckInterval: 5 * time.Millisecond, Grace: 20 * time.Millisecond, MaxBackoff: time.Second, CallTimeout: 2 * time.Second}
	monitor, err := New(source, config, nil)
	if err != nil {
		t.Fatal(err)
	}
	if monitor.Status(ctx).Status == "online" {
		t.Fatal("the helper hid the real control outage")
	}
	runCtx, stop := context.WithCancel(ctx)
	done := make(chan struct{})
	go func() { defer close(done); monitor.Run(runCtx) }()
	defer func() { stop(); <-done }()
	wait(t, func() bool { return observed.starts.Load() > 0 && monitor.Status(ctx).Status == "online" })
	after, err := client.StatusWithoutPeers(ctx)
	if err != nil {
		t.Fatal(err)
	}
	prefsAfter, err := client.GetPrefs(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if before.Self.ID != after.Self.ID || before.Self.PublicKey != after.Self.PublicKey || !reflect.DeepEqual(before.TailscaleIPs, after.TailscaleIPs) {
		t.Fatal("control recovery changed the node identity")
	}
	if !reflect.DeepEqual(prefsBefore, prefsAfter) || observed.badOptions.Load() {
		t.Fatal("control recovery changed preferences or supplied new enrollment options")
	}
	if control.NumNodes() != 1 {
		t.Fatal("recovery enrolled another node")
	}
	echo()
	response, err := http.Get(proxyURL)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if body, err := io.ReadAll(response.Body); err != nil || string(body) != "stable proxy" || proxy.URL != proxyURL {
		t.Fatal("loopback proxy changed during recovery")
	}
	t.Logf("recovered same node with %d retained-identity control restart(s); listeners remained usable", observed.starts.Load())
}
