package nethealth

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"time"

	"tailscale.com/health"
	"tailscale.com/ipn"
	"tailscale.com/ipn/ipnstate"
	"tailscale.com/net/netmon"
	"tailscale.com/tsnet"
)

type Client interface {
	StatusWithoutPeers(context.Context) (*ipnstate.Status, error)
	DebugAction(context.Context, string) error
	Start(context.Context, ipn.Options) error
}

type Source struct {
	Client         Client
	Health         func() *health.State
	NetworkState   func() LinkState
	RefreshNetwork func()
	Subscribe      func(func()) func()
}

func ServerSource(server *tsnet.Server, client Client) Source {
	return Source{Client: client,
		Health: func() *health.State { return server.Sys().HealthTracker.Get().CurrentState() },
		NetworkState: func() LinkState {
			monitor, ok := server.Sys().NetMon.GetOK()
			if !ok {
				return LinkUnknown
			}
			state := monitor.InterfaceState()
			if state == nil || len(state.Interface) == 0 {
				return LinkUnknown
			}
			if state.AnyInterfaceUp() {
				return LinkUp
			}
			return LinkDown
		},
		RefreshNetwork: func() {
			if monitor, ok := server.Sys().NetMon.GetOK(); ok {
				monitor.InjectEvent()
			}
		},
		Subscribe: func(wake func()) func() {
			if monitor, ok := server.Sys().NetMon.GetOK(); ok {
				return monitor.RegisterChangeCallback(func(*netmon.ChangeDelta) { wake() })
			}
			return func() {}
		},
	}
}

type Monitor struct {
	source     Source
	config     Config
	notify     func(Status)
	wake       chan struct{}
	operations sync.Mutex
	reset      atomic.Bool
	running    atomic.Bool
}

func New(source Source, config Config, notify func(Status)) (*Monitor, error) {
	if err := config.validate(); err != nil {
		return nil, err
	}
	if source.Client == nil || source.Health == nil || source.NetworkState == nil || source.RefreshNetwork == nil || source.Subscribe == nil {
		return nil, errors.New("Invalid network health source")
	}
	return &Monitor{source: source, config: config, notify: notify, wake: make(chan struct{}, 1)}, nil
}

func (m *Monitor) Wake() {
	select {
	case m.wake <- struct{}{}:
	default:
	}
}

// Exclusive serializes explicit enrollment with automatic recovery and gives
// the new state a full grace period. It never starts login itself.
func (m *Monitor) Exclusive(operation func() error) error {
	m.operations.Lock()
	defer m.operations.Unlock()
	m.reset.Store(true)
	defer func() { m.reset.Store(true); m.Wake() }()
	return operation()
}

func (m *Monitor) inspect(ctx context.Context) Assessment {
	st, err := m.source.Client.StatusWithoutPeers(ctx)
	if err != nil {
		st = nil
	}
	return Inspect(st, m.source.Health(), m.source.NetworkState())
}
func (m *Monitor) Status(ctx context.Context) Status { return m.inspect(ctx).Status }

// Run must be called once. All recovery work runs serially here, never in an OS
// callback or a caller's status request. SDK calls have independent deadlines.
func (m *Monitor) Run(ctx context.Context) {
	if !m.running.CompareAndSwap(false, true) {
		return
	}
	defer m.running.Store(false)
	unregister := m.source.Subscribe(m.Wake)
	defer unregister()
	ticker := time.NewTicker(m.config.CheckInterval)
	defer ticker.Stop()
	p := policy{config: m.config}
	var last Status
	for {
		if ctx.Err() != nil {
			return
		}
		callCtx, cancel := context.WithTimeout(ctx, m.config.CallTimeout)
		a := m.inspect(callCtx)
		cancel()
		if m.reset.Swap(false) {
			p.reset()
		}
		if next := p.decide(time.Now(), a); next != none {
			m.operations.Lock()
			if m.reset.Swap(false) {
				p.reset()
			} else if ctx.Err() == nil {
				callCtx, cancel = context.WithTimeout(ctx, m.config.CallTimeout)
				// Recheck after waiting for explicit enrollment. Never restart a
				// node that recovered, logged out or became stopped meanwhile.
				fresh := m.inspect(callCtx)
				if fresh.Eligible {
					m.source.RefreshNetwork()
					if next == restartControl && fresh.ControlLost {
						// Empty options preserve current prefs and persisted keys.
						// Do not close tsnet, its listeners or the host process.
						_ = m.source.Client.Start(callCtx, ipn.Options{})
					} else {
						_ = m.source.Client.DebugAction(callCtx, "rebind")
					}
				} else {
					p.reset()
				}
				cancel()
			}
			m.operations.Unlock()
			callCtx, cancel = context.WithTimeout(ctx, m.config.CallTimeout)
			a = m.inspect(callCtx)
			cancel()
		}
		if ctx.Err() != nil {
			return
		}
		if !same(last, a.Status) {
			last = a.Status
			if m.notify != nil {
				m.notify(a.Status)
			}
		}
		select {
		case <-ctx.Done():
			return
		case <-m.wake:
		case <-ticker.C:
		}
	}
}
func same(a, b Status) bool {
	value := func(p *string) string {
		if p == nil {
			return ""
		}
		return *p
	}
	return a.Status == b.Status && value(a.Error) == value(b.Error) && value(a.LoginURL) == value(b.LoginURL)
}
