package buffer

import "sync"

// RingBuffer is a fixed-capacity ring buffer that drops the oldest item when full.
type RingBuffer[T any] struct {
	mu    sync.Mutex
	buf   []T
	head  int
	tail  int
	count int
	cap   int
}

// NewRingBuffer creates a RingBuffer with the given capacity.
func NewRingBuffer[T any](capacity int) *RingBuffer[T] {
	return &RingBuffer[T]{
		buf: make([]T, capacity),
		cap: capacity,
	}
}

// Push adds an item to the buffer. If full, the oldest item is overwritten.
func (r *RingBuffer[T]) Push(item T) {
	r.mu.Lock()
	defer r.mu.Unlock()

	r.buf[r.head] = item
	r.head = (r.head + 1) % r.cap
	if r.count == r.cap {
		r.tail = (r.tail + 1) % r.cap
	} else {
		r.count++
	}
}

// Drain removes and returns all items from the buffer in FIFO order.
func (r *RingBuffer[T]) Drain() []T {
	r.mu.Lock()
	defer r.mu.Unlock()

	if r.count == 0 {
		return nil
	}

	out := make([]T, 0, r.count)
	for i := 0; i < r.count; i++ {
		idx := (r.tail + i) % r.cap
		out = append(out, r.buf[idx])
	}
	r.head = 0
	r.tail = 0
	r.count = 0
	return out
}

// Len returns the number of items currently in the buffer.
func (r *RingBuffer[T]) Len() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.count
}

// Cap returns the maximum capacity.
func (r *RingBuffer[T]) Cap() int {
	return r.cap
}
