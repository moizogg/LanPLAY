# Capture APIs (Phase 4)

## Decision for V1 host capture

**Primary:** DXGI Desktop Duplication API (GPU path).

| API | Pros | Cons |
|-----|------|------|
| **DXGI Desktop Duplication** | Low latency, GPU textures, proven in game stream | Same GPU as display; exclusive fullscreen quirks |
| Windows Graphics Capture | Cross-GPU, window pick | Yellow border (Win11), slightly higher latency |
| GDI | Simple | Slow — **do not use** |

## Phase 4 scope

- Open DXGI duplication on primary output
- `AcquireNextFrame` loop
- Report FPS, resolution, acquire timing to Host UI
- Reopen on `DXGI_ERROR_ACCESS_LOST` (resolution / mode change)

## Not yet

- Encode (Phase 5)
- Network stream (Phase 6)
- Client decode/present
