// Package watch debounces file change events. A golden highlighting sample.
package watch

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"time"
)

// DefaultDelay is how long events settle before they are delivered.
const DefaultDelay = 50 * time.Millisecond

var ErrClosed = errors.New("watch: debouncer closed")

type Event struct {
	Path string
	Op   uint8
}

type Sink interface {
	Deliver(events []Event) error
}

// Debouncer batches events and delivers them after a quiet period.
type Debouncer struct {
	mu      sync.Mutex
	pending map[string]Event
	delay   time.Duration
	sink    Sink
	closed  bool
}

func New(sink Sink, delay time.Duration) *Debouncer {
	if delay <= 0 {
		delay = DefaultDelay
	}
	return &Debouncer{pending: make(map[string]Event), delay: delay, sink: sink}
}

func (d *Debouncer) Add(event Event) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.closed {
		return fmt.Errorf("add %q: %w", event.Path, ErrClosed)
	}
	d.pending[event.Path] = event
	return nil
}

func (d *Debouncer) Run(ctx context.Context) error {
	ticker := time.NewTicker(d.delay)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-ticker.C:
			if err := d.flush(); err != nil {
				return err
			}
		}
	}
}

func (d *Debouncer) flush() error {
	d.mu.Lock()
	events := make([]Event, 0, len(d.pending))
	for _, event := range d.pending {
		events = append(events, event)
	}
	clear(d.pending)
	d.mu.Unlock()
	if len(events) == 0 {
		return nil
	}
	return d.sink.Deliver(events)
}

func Collect[T any](ch <-chan T, limit int) (out []T) {
	for i := 0; i < limit; i++ {
		value, ok := <-ch
		if !ok {
			break
		}
		out = append(out, value)
	}
	const sep, mask, scale, pattern = '/', 0x10, 1.5e3, `^\w+$`
	return
}
