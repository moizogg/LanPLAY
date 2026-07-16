# Phase status

| Phase | Status | Notes |
|-------|--------|--------|
| **0 Research** | Done | Plan locked: IP connect, no rooms |
| **1 Shell** | Done | Tauri Host/Client, CI |
| **2 Controllers** | Done enough | Pad + KBM + capture toggle + accept/reject |
| **3 Networking** | Mostly done | TCP join + UDP; formal transport trait still stub |
| **4 Desktop capture** | Done | DXGI + FPS stats dogfooded |
| **5 Encode** | **In progress** | OpenH264 + Settings tab (res/fps/bitrate/encoder); HW next |
| 6 Video stream | Not started | |
| 7 Audio | Not started | |

## Phase 5 checklist

- [x] `VideoEncoder` trait
- [x] OpenH264 encode after DXGI BGRA download
- [x] Scale long edge ≤ 1280 for CPU budget
- [x] Rate-limit encode to target FPS (capture can be higher)
- [x] UI: encode FPS, encode ms, bitrate, encoder name
- [ ] Dogfood encode FPS / bitrate on host
- [ ] NVENC / AMF / QSV backends (hardware path)
