# Phase status

| Phase | Status | Notes |
|-------|--------|--------|
| **0 Research** | Done enough | Plan locked: IP connect, no rooms |
| **1 Shell** | **Code complete** | Tauri Host/Client UI, CI workflow. **Validate** by downloading green CI artifact and opening the app. |
| **2 Controllers** | **In progress / implemented** | XInput → UDP → ViGEm X360. Needs ViGEmBus on host to fully pass success criteria. |
| 3 Networking | Not started | |
| 4+ Video… | Not started | |

## Phase 1 checklist

- [x] Monorepo + packages  
- [x] Host / Client UI  
- [x] Tailscale IP display  
- [x] Documented online `.exe` build (GitHub Actions)  
- [ ] Human opens CI-built `lanplay.exe` and clicks around  

## Phase 2 checklist

- [x] `InputPacket` binary codec + unit test  
- [x] Client XInput poll + UDP send  
- [x] Host UDP recv + ViGEm (dynamic DLL)  
- [x] Latency / packet stats in UI  
- [x] Bundle ViGEmClient + driver setup inside app (no user GitHub downloads)  
- [x] Host UI one-click driver install (UAC once — Windows requirement)  
- [ ] Two PCs: remote pad moves virtual Xbox 360 in a game  

