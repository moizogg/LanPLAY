# Repo layout (Phase 1)

```text
LanPLAY/
  apps/
    desktop/                 # Tauri app (Host + Client modes)
      src/                   # React UI
      src-tauri/             # Rust shell, session stubs, Tailscale detect
  packages/
    shared/                  # Ports, session types, errors
    protocol/                # Protocol version / channel IDs
    networking/              # NetworkTransport trait + StubTransport
    controllers/             # Stub (Phase 2)
    video/                   # Stub (Phase 4+)
    audio/                   # Stub (Phase 7+)
    input/                   # Stub (Phase 8+)
    overlay/                 # Overlay stats model
  docs/
    architecture/            # Plans + this file
    adr/
    research/
  Cargo.toml                 # Rust workspace root
  README.md
```

## Package dependency direction

```text
desktop (tauri)
  → networking, protocol, shared, overlay
networking → protocol, shared
protocol → shared
controllers / video / audio / input → shared  (later)
```

No package should depend on `apps/desktop`.
