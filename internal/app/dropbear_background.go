package app

import "context"

// Serialize background command lifetimes and SFTP startup for Dropbear.
// Some embedded servers close concurrent channels before delivering their
// buffered data, which can terminate the shared SSH transport. The gate is
// session-scoped; cancelled monitor workers retain it until channel cleanup
// finishes, and SFTP workers retain it until initialization returns.
// Active SFTP use, terminal input, and other SSH servers are unaffected.
func (s *Session) acquireDropbearBackground(ctx context.Context) (func(), error) {
	noop := func() {}
	if !s.usesDropbearServer() {
		return noop, ctx.Err()
	}
	s.backgroundGateOnce.Do(func() {
		s.backgroundGate = make(chan struct{}, 1)
		s.backgroundGate <- struct{}{}
	})
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-s.backgroundGate:
		if err := ctx.Err(); err != nil {
			s.backgroundGate <- struct{}{}
			return nil, err
		}
		return func() { s.backgroundGate <- struct{}{} }, nil
	}
}
