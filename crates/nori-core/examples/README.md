# Examples

## magnet_probe

Resolves a magnet link through a bare librqbit session and prints
state / peers / progress / download speed every 2s for 20s. Bypasses nori's
manager so engine behaviour can be observed without the app.

```
cargo run -p nori-core --example magnet_probe -- '<magnet-link>'
RUST_LOG=librqbit=debug cargo run -p nori-core --example magnet_probe -- '<magnet-link>'
```

Writes its session dir to `/tmp/nori-magnet-probe` (overwritten each run).
IPv4-only, ephemeral listener port, local service discovery disabled.

### Findings

<TODO>
