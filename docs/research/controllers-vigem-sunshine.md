# Controllers: ViGEm + Sunshine-style path (Phase 2)

## Model

Same idea as Sunshine host + Moonlight client:

1. Client reads a real pad (we use **XInput** user index 0).
2. Client sends a compact binary state packet over the network (we use **UDP** to host `:47801`).
3. Host applies state to a **ViGEm virtual Xbox 360** device.
4. Games see a normal XInput pad — no game injection.

## Packet

`lanplay-protocol::InputPacket` — 32 bytes, magic `LPIP`, seq + client timestamp + buttons/axes/triggers.

## Host dependencies (bundled — no user GitHub downloads)

| Component | Bundled? | User action |
|-----------|----------|-------------|
| `ViGEmClient.dll` | Yes (app resources) | None — auto-loaded from install dir |
| ViGEmBus kernel driver | Yes (installer inside app) | One-time **Install gamepad support** in Host UI (UAC) |

**Why not 100% silent embed?**  
Windows kernel drivers **must** be installed through the driver store with elevation. Same model as many game tools: ship the official signed setup, run it for the user.

Fetch redist for builds:

```powershell
powershell -File tools/fetch-vigem-redist.ps1
```

CI does this automatically before `tauri build`.

## Client

- Any XInput-compatible pad (Xbox 360/One/Series, many others via XInput wrappers)
- Poll rate: 250 Hz

## Testing without video

1. Host: install ViGEmBus, Start Host, copy Tailscale IP  
2. Client: Connect with that IP, plug controller  
3. Host: open Windows Game Controllers / a game — extra Xbox 360 should move  

## Latency metric

Host computes `now_us - client_ts_us` per packet (requires roughly synced clocks; Tailscale machines are usually fine). Clamped if absurd skew.
