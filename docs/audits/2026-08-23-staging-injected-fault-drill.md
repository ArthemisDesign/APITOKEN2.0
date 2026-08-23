# Staging injected-fault drill — 2026-08-23

- Exact implementation: `89d27138326f85326f865e66c4fdec5ac7dd980c`.
- Trusted policy digest: `49d2eabe8fad7f25d8a99845b94743c2afe2ed8ed9259815d65514544a20d497`.
- Injection: stage latency evidence changed to 99,999 ms after a GREEN bounded mock-twin sample.
- Expected result: the trusted degradation gate returns RED before any promotion path.
- Observed result: `stage-degrade-proof: injected regression caught`.
- Production effect: none. Production readiness and `deploy/watchdog` stayed GREEN.
- Recovery: the proof uses disposable evidence and removes it on exit. No stage or production unit changed.
