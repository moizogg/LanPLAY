# Phase status

| Phase | Status | Notes |
|-------|--------|--------|
| **0 Research** | Done | Plan locked: IP connect, no rooms |
| **1 Shell** | Done | Tauri Host/Client, CI |
| **2 Controllers** | Done enough | Pad + KBM + capture toggle + accept/reject |
| **3 Networking** | Mostly done | TCP join + UDP; formal transport trait still stub |
| **4 Desktop capture** | **In progress** | DXGI Desktop Duplication + FPS stats on Host |
| 5 Encode | Not started | |
| 6 Video stream | Not started | |
| 7 Audio | Not started | |

## Phase 4 checklist

- [x] Capture trait + DXGI backend
- [x] Host starts capture with Start Host
- [x] UI: resolution, FPS, capture ms
- [ ] Dogfood on real hardware (60 FPS stable)
- [ ] Notes on exclusive fullscreen edge cases
