package buffer

import (
	"sync"
	"time"
)

// Flusher periodically drains a RingBuffer and writes items via flushFn.
type Flusher[T any] struct {
	ring     *RingBuffer[T]
	flushFn  func([]T)
	interval time.Duration
	stopCh   chan struct{}
	wg       sync.WaitGroup
}

// NewFlusher creates a Flusher that drains the ring buffer at the given interval.
func NewFlusher[T any](ring *RingBuffer[T], interval time.Duration, flushFn func([]T)) *Flusher[T] {
	return &Flusher[T]{
		ring:     ring,
		flushFn:  flushFn,
		interval: interval,
		stopCh:   make(chan struct{}),
	}
}

// Start begins the background flush loop.
func (f *Flusher[T]) Start() {
	f.wg.Add(1)
	go func() {
		defer f.wg.Done()
		ticker := time.NewTicker(f.interval)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				f.Flush()
			case <-f.stopCh:
				return
			}
		}
	}()
}

// Flush drains the ring buffer and invokes the flush function with the items.
func (f *Flusher[T]) Flush() {
	items := f.ring.Drain()
	if len(items) > 0 {
		f.flushFn(items)
	}
}

// Stop halts the background loop and performs a final flush.
func (f *Flusher[T]) Stop() {
	close(f.stopCh)
	f.wg.Wait()
	f.Flush()
}
