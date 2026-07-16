# Phase status

| Phase | Status | Notes |
|-------|--------|--------|
| **0 Research** | Done | Plan locked: IP connect, no rooms |
| **1 Shell** | Done | Tauri Host/Client, CI |
| **2 Controllers** | Done enough | Pad + KBM + capture toggle + accept/reject |
| **3 Networking** | Mostly done | TCP join + UDP; formal transport trait still stub |
| **4 Desktop capture** | Done | DXGI + FPS stats dogfooded |
| **5 Encode** | **Done enough** | OpenH264 + Settings + dogfood; HW (NVENC/AMF/QSV) later |
| **6 Video stream** | **In progress** | Host→client H.264 UDP + decode + JPEG preview |
| 7 Audio | Not started | |

## Phase 6 checklist

- [x] Video fragment UDP protocol (`LPVD`)
- [x] Host send after Accept (media_port + 1)
- [x] Client reassemble + OpenH264 decode
- [x] Client UI preview (JPEG)
- [ ] Dogfood two-PC / local loopback stream
- [ ] Low-latency present (skip JPEG path later)

## Phase 5 checklist

- [x] `VideoEncoder` trait
- [x] OpenH264 encode after DXGI BGRA download
- [x] Scale / fixed res + Settings tab (fps, bitrate, encoder, display)
- [x] Rate-limit encode to target FPS (capture can be higher)
- [x] UI: encode FPS, encode ms, bitrate, encoder name
- [x] Dogfood encode on host (pipeline works; software ~15–30 FPS typical)
- [ ] NVENC / AMF / QSV backends (not required to start Phase 6)
