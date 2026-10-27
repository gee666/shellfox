package nethealth

import (
	"errors"
	"time"
)

type Config struct {
	CheckInterval time.Duration
	Grace         time.Duration
	MaxBackoff    time.Duration
	CallTimeout   time.Duration
}

func DefaultConfig() Config {
	return Config{CheckInterval: 5 * time.Second, Grace: 30 * time.Second, MaxBackoff: 5 * time.Minute, CallTimeout: 5 * time.Second}
}

func (c Config) validate() error {
	if c.CheckInterval <= 0 || c.CheckInterval > time.Minute || c.Grace <= 0 || c.Grace > 10*time.Minute || c.MaxBackoff < c.Grace || c.MaxBackoff > time.Hour || c.CallTimeout <= 0 || c.CallTimeout > 30*time.Second {
		return errors.New("Invalid network recovery settings")
	}
	return nil
}

type action uint8

const (
	none action = iota
	rebind
	restartControl
)

// policy belongs to one worker. Link callbacks only wake that worker; they do
// not reset its grace or bypass backoff during a burst of OS notifications.
type policy struct {
	config      Config
	badSince    time.Time
	next        time.Time
	attempts    int
	controlLost bool
}

func (p *policy) reset() {
	p.badSince = time.Time{}
	p.next = time.Time{}
	p.attempts = 0
	p.controlLost = false
}
func (p *policy) decide(now time.Time, a Assessment) action {
	if !a.Eligible {
		p.reset()
		return none
	}
	// A fresh control outage gets its own grace and backoff. A long-lived relay
	// warning must neither accelerate control resets nor delay them for minutes.
	if p.controlLost != a.ControlLost {
		p.reset()
		p.controlLost = a.ControlLost
	}
	if p.badSince.IsZero() {
		p.badSince = now
		return none
	}
	if now.Sub(p.badSince) < p.config.Grace || now.Before(p.next) {
		return none
	}
	result := rebind
	if a.ControlLost && p.attempts > 0 && now.Sub(p.badSince) >= 2*p.config.Grace {
		result = restartControl
	}
	delay := p.config.Grace
	for i := 0; i < p.attempts && delay < p.config.MaxBackoff; i++ {
		delay = min(delay*2, p.config.MaxBackoff)
	}
	p.attempts++
	p.next = now.Add(delay)
	return result
}
