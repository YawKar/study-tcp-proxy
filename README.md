# What is it?

A small raw TCP proxy (TLS-passthrough, no decryption in the middle) on top of the `tokio` async runtime with the following features:
- Health checks
  - Passive probe
  - TCP connect probe
  - TCP Send-Expect probe
- Load balancers
  - Round robin
  - Weighted random
- Metrics
  - Prometheus endpoint with a bunch of primitive metrics

# A note about other design with config hot-reload

Config is loaded once at startup.
For production hot-reload, I'd model this as a reconciliation loop similar to Kubernetes controllers: config changes produce desired-state events, a reconciler diffs desired vs actual, and workers subscribe to relevant object changes (port bindings, target sets).
A generational key-value store would handle the consistency problem where workers hold stale references to deleted targets.
I prototyped SIGHUP-based reload but removed it because retrofitting reactivity onto an imperative design produced more complexity than value.

