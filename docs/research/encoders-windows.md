# Encoders (Phase 5)

## Current

| Backend | Status | Notes |
|---------|--------|--------|
| **OpenH264** | **Active** | Software H.264; validates capture→encode pipeline |
| NVENC | Planned | Replace via `VideoEncoder` trait |
| AMF | Planned | AMD |
| QSV | Planned | Intel |

## Settings (V1 software)

- Encode long edge capped at **1280** (nearest-neighbor scale from capture)
- Target ~**8 Mbps**
- 60 FPS budget (desktop idle still lower capture FPS)

## Next

1. Media Foundation hardware MFT probe (often NVENC/QSV under the hood)
2. Native NVENC session for lowest latency
3. Stream Annex-B / length-prefixed NALUs in Phase 6
