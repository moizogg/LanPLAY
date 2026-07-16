# LANPlay — Full Architecture & Implementation Plan

**Status:** Planning only (no application code yet)  
**Audience:** Solo / vibe-coding builder who wants a clean, long-lived codebase  
**Constraint:** Phase-gated. Do not start Phase N until Phase N−1 success criteria pass.  
**Doc version:** 1.1 — simplified connect (no rooms); general desktop stream use cases

---

## 1. What LANPlay Is

### 1.1 Mission (V1 testing scope)

LANPlay is a **low-latency desktop streaming app** over Tailscale:

1. **Host PC** captures the desktop (or game), encodes video + audio, listens for a client  
2. **Client PC** connects by typing the host’s **Tailscale IP** — nothing else  
3. Client sees the host screen, hears host audio  
4. Client can control the host with:
   - **Xbox / XInput controller** → injected as virtual **Xbox 360** on host (ViGEm, Sunshine-style)  
   - **Keyboard + mouse** → for desktop, browsers, movies, non-game apps  

You can use it to:

| Use case | How |
|----------|-----|
| Local multiplayer games over internet | Remote pad = Player 2 on host |
| Single-player / play your gaming PC from elsewhere | Controller or KBM on client |
| “Remote desktop” style control | KBM + full desktop capture |
| Watch movies / YouTube on host | Stream + audio; KBM to control player |
| Browse / casual PC use | Same pipeline |

**Not** TeamViewer as a product category — we still optimize for **gaming latency** first — but V1 is a **general stream**, not multiplayer-only.

### 1.2 Explicitly out of V1 (later)

| Deferred | When |
|----------|------|
| 6-digit room codes | Future product polish |
| Go matchmaking backend | Future |
| Friends list / accounts | Future |
| Zero-config NAT without Tailscale | V2 networking |
| Multiple simultaneous clients | Later |

**V1 connect model is intentionally dumb and testable:**

```text
Host:  click "Start Host" → shows Tailscale IP + "Listening…"
Client: paste host Tailscale IP → Connect → stream
```

No server in the middle. No room service. Just Tailscale reachability + our session protocol.

### 1.3 Product priorities

1. **Input latency** (controller + KBM) — never sacrifice for pretty video  
2. **Simple connect** — Tailscale IP only for testing  
3. **Useful stream** — desktop works for games *and* movies/desktop  
4. Clean modular code so rooms/native net can land later without rewrites  

---

## 2. End-to-End User Journey (V1)

### 2.1 One-time setup (both machines)

1. Install **Tailscale**, log into same tailnet (or shared machine)  
2. Install **LANPlay**  
3. On **host** (the PC that will be streamed): install **ViGEmBus** (for virtual Xbox 360)  
4. Confirm you can `ping` the host’s Tailscale IP from the client (e.g. `100.x.y.z`)

### 2.2 Every session

```
┌─────────────┐                              ┌─────────────┐
│   HOST PC   │                              │  CLIENT PC  │
│ (streamed)  │                              │ (you/friend)│
└──────┬──────┘                              └──────┬──────┘
       │                                            │
       │ 1. Open LANPlay → Host mode                │
       │ 2. Start Host (listen on fixed ports)      │
       │ 3. UI shows Tailscale IP e.g. 100.64.1.5   │
       │                                            │
       │         4. Client: enter 100.64.1.5        │
       │            → Connect                       │
       │◄════════ 5. Session handshake ════════════►│
       │                                            │
       │ 6. Capture desktop + system audio          │
       │════════ 7. Video + audio stream ══════════►│ decode + display
       │                                            │
       │◄═══════ 8a. Controller packets ════════════│ XInput pad
       │◄═══════ 8b. Keyboard / mouse packets ══════│ for desktop
       │                                            │
       │ 9. ViGEm X360 + synthetic KBM on host      │
       └────────────────────────────────────────────┘
```

**Host responsibilities**

- Start listening (no room code)  
- Show own Tailscale IP (copy button) + listen port  
- Capture + encode video/audio  
- Accept **one** client connection (V1)  
- Create virtual Xbox 360 pad when client has a pad  
- Inject keyboard/mouse for desktop control  
- Optional: “allow input” toggle for safety when streaming movies to someone  

**Client responsibilities**

- Enter host Tailscale IP (and port if not default)  
- Connect  
- Decode video, play audio  
- Send controller + keyboard/mouse  
- Fullscreen stream view + latency overlay  

**How to get the host Tailscale IP**

- LANPlay Host UI displays it if Tailscale is up  
- Or: `tailscale ip -4` on host  
- Or: Tailscale admin console / tray app  

---

## 3. Technology Stack (V1)

### 3.1 Desktop application

| Layer | Choice | Why |
|-------|--------|-----|
| Shell | **Tauri 2** | Small installer, native window, Rust core |
| App logic | **Rust** | Capture, encode, input, networking performance |
| UI | **React + TypeScript** | Fast vibe UI |
| Styling | **Tailwind CSS** | Speed |
| Motion | **Framer Motion** | Polish (can be light in testing UI) |
| IPC | Tauri commands + events | UI ↔ Rust |

### 3.2 Media

| Concern | V1 choice | Notes |
|---------|-----------|-------|
| Capture | **Desktop** via WGC or DXGI Desktop Duplication | Full desktop so movies + games + remote desktop all work |
| Encode | **NVENC** → AMF → QSV | No default CPU encode |
| Codec | **H.264** | Universal |
| Decode | D3D11 / DXVA | Client |
| Audio | **WASAPI loopback** + **Opus** | System audio (Netflix, game, Discord, etc.) |

**Why full desktop capture for V1:**  
One path covers games, browser movies, file explorer, settings — true “use my PC remotely.” Window-only capture can come later as a quality/perf option.

### 3.3 Input stack

| Input | Client capture | Host inject | Priority |
|-------|----------------|-------------|----------|
| Gamepad | **XInput** | **ViGEm Xbox 360** | Highest |
| Mouse | relative + buttons + wheel | `SendInput` / similar | Highest |
| Keyboard | key down/up | `SendInput` / similar | Highest |

**Controllers (Sunshine-aligned)**

```text
Client XInput state
  → binary InputPacket
  → high-priority channel
  → Host ViGEm target_x360_update(...)
  → Game sees real Xbox 360 pad
```

**Keyboard / mouse (needed for remote desktop + movies)**

```text
Client OS events (when stream window focused)
  → binary KbmPacket
  → same input channel (or sibling high-priority)
  → Host SendInput / relative mouse
```

**V1 controller scope:** Xbox 360 virtual only.  
**V1 KBM scope:** enough to use desktop and control media players; not perfect multi-layout IME.

Sunshine also forwards mouse/keyboard for desktop sessions — same idea: separate from gamepad, still low latency.

### 3.4 Networking — Tailscale IP only (no backend)

| Piece | V1 |
|-------|----|
| Reachability | **Tailscale** (user installs / same tailnet) |
| Addressing | Client types **host Tailscale IPv4** (e.g. `100.x.y.z`) |
| Discovery | **None** — manual IP |
| Matchmaking / rooms | **None** |
| Media path | Direct host ↔ client sockets over Tailscale |
| App protocol | Our session + channels (still required) |

**Critical design rule:**  
Modules never hardcode Tailscale. They use:

```text
trait NetworkTransport {
  // Host
  listen(bind_addr, port) -> Incoming
  // Client
  connect(host_ip, port) -> Session
  send(channel, bytes, priority)
  recv()
  rtt_ms() / stats()
}
```

`TailscaleIpConnect` is just: user-supplied IP + our ports, running on the tailnet interface.  
Later: rooms, mDNS, ICE — all become other ways to get `(ip, port)` into `connect()`.

**Default ports (document; changeable in settings)**

| Port | Use |
|------|-----|
| `47800` | Control / session (TCP recommended for handshake) |
| `47801` | Media + input multiplex (UDP) |

Channels:

| Channel | Priority | Reliability | Content |
|---------|----------|-------------|---------|
| `input` | **Highest** | Latest-only | Gamepad + KBM |
| `control` | High | Reliable | Hello, version, quality, input lock |
| `audio` | Medium-high | Best-effort | Opus |
| `video` | Medium | Best-effort + IDR request | H.264 |
| `metrics` | Low | Best-effort | Overlay stats |

### 3.5 Backend / rooms — **not in V1**

| V1 | Future |
|----|--------|
| No Go server required | Optional room service |
| No Postgres / Redis | When you want codes / friends |
| No accounts | Public alpha polish |

Keep monorepo **ready** for `apps/backend` later, but **do not build it for testing**.

### 3.6 Monorepo layout

```text
LanPLAY/
  apps/
    desktop/          # Tauri: Host mode + Client mode (one exe)
    # backend/        # FUTURE only — room codes, not for testing
  packages/
    networking/       # Transport trait + direct IP/UDP/TCP + Tailscale helpers
    video/
    audio/
    controllers/      # XInput + ViGEm X360
    input/            # KBM capture + host injection (or fold into controllers/)
    protocol/         # Packet layouts
    overlay/
    shared/
    ui/
  docs/
    architecture/
    adr/
    research/
  prototypes/
  tools/
```

**UI surface for V1 testing (keep tiny):**

- Toggle: **Host** | **Client**  
- Host: **Start / Stop**, show `Tailscale IP`, port, status, “Allow remote input”  
- Client: **IP field**, Connect / Disconnect, quality preset  
- In session: stream + overlay  

No lobby, no codes, no friends.

---

## 4. System Architecture

### 4.1 High-level (V1 — no cloud)

```text
┌──────────────────────────────────────────────────────────┐
│                     LANPlay Desktop (Tauri)              │
│  React UI  ──IPC──  Rust App Core                        │
│     │                  │                                 │
│     │     ┌────────────┼────────────┬────────────┐       │
│     │     ▼            ▼            ▼            ▼       │
│     │  Session      Input         Video       Audio      │
│     │  Manager   (pad + KBM)      Package     Package    │
│     │     │            │            │            │       │
│     │     └────────────┴─────┬──────┴────────────┘       │
│     │                        ▼                           │
│     │              Networking Package                    │
│     │         listen(port) / connect(ip, port)           │
└─────┼────────────────────────┼───────────────────────────┘
      │                        │
      │                        ▼
      │              Tailscale tunnel (100.x.y.z)
      │              Direct P2P sockets only
      └────────── no matchmaking server ──────────
```

### 4.2 Host pipeline

```text
Desktop (games, browser, movies, anything)
        │
        ▼
   GPU Capture (WGC / DXGI)
        │
        ▼
   HW Encode (NVENC / AMF / QSV) → H.264 packets → Network (video)

WASAPI loopback → Opus → Network (audio)

Network (input) → unpack
        ├── Gamepad → ViGEm Xbox 360
        └── KBM → SendInput (if "allow input" on)
```

### 4.3 Client pipeline

```text
Network (video) → HW decode → fullscreen/window present
Network (audio) → Opus → speakers

XInput pad → InputPacket → Network (input)
Focused stream window KBM → KbmPacket → Network (input)
```

### 4.4 Connect sequence (V1 — Tailscale IP)

```text
Host                                      Client
 |                                          |
 | Start Host → listen :47800 / :47801      |
 | UI: "Your IP: 100.64.1.5"                |
 |                                          |
 |                     User pastes 100.64.1.5
 |                     Connect
 |                                          |
 |◄──── TCP control connect + hello ────────|
 |───── accept if version OK ───────────────►|
 |───── session params (codec, res) ────────►|
 |                                          |
 |══════ UDP media+input flow ══════════════|
 | video/audio ─────────────────────────────►|
 |◄──────────────────────────────── input ──|
 |                                          |
 | Stop / client disconnect → teardown      |
 | (destroy ViGEm pad, stop encode)         |
```

**Optional quality-of-life (still no backend):**

- Remember last 5 host IPs in client UI  
- Host auto-detect Tailscale IPv4 via `tailscale ip -4` or Windows Tailscale API  
- “Copy IP” button  

---

## 5. Input Subsystem (Deep Dive)

### 5.1 Why input is early

Bad input = unusable for games **and** desktop. Video can be rough first.

### 5.2 Gamepad (Xbox 360 / ViGEm)

Same as Sunshine host path:

1. Client polls XInput  
2. Fixed binary packet (buttons, axes, triggers, seq, timestamp)  
3. Host updates ViGEm X360 target  

| Field | Type | Notes |
|-------|------|-------|
| `controller_id` | u8 | 0..3 |
| `buttons` | u16 | face, shoulders, dpad, sticks |
| `lt` / `rt` | u8 | triggers |
| `lx ly rx ry` | i16 | sticks |
| `seq` | u32 | monotonic |
| `client_ts_us` | u64 | latency measure |
| `flags` | u8 | connected |

**Rules**

- Latest-state wins (drop stale stick packets)  
- Input priority > video under congestion  
- Hotplug: create/destroy ViGEm target  
- Host needs ViGEmBus installed  

### 5.3 Keyboard + mouse (desktop / movies)

| Event | Payload idea |
|-------|----------------|
| Mouse move | dx, dy (relative) preferred for games; absolute optional for desktop |
| Mouse buttons | left/right/middle down/up |
| Wheel | delta |
| Key | virtual-key or scan code + down/up |

**Host inject:** Windows `SendInput` (or equivalent).  
**Client:** only capture when LANPlay stream window is focused (so you can alt-tab locally).

**Safety**

- Host default: **Allow remote input = ON** for personal testing; easy toggle OFF (view-only movie share)  
- Future: PIN before accept — not required for solo testing  

### 5.4 Latency budget (input)

| Stage | Budget |
|-------|--------|
| Capture + pack | ≤ 1–2 ms |
| Network one-way | path-dependent (Tailscale) |
| Apply on host | ≤ 1 ms |

---

## 6. Networking Notes (Tailscale Now → Better Later)

### 6.1 V1 user story

1. Both PCs on Tailscale  
2. Host starts LANPlay Host  
3. Client enters `100.x.y.z`  
4. Stream works  

No STUN, no TURN, no room server.

### 6.2 What we still implement

- Session handshake + version check  
- Channel multiplex  
- Input prioritization  
- Bitrate presets / later adapt  
- Disconnect detection  
- Stats (RTT, loss)  

### 6.3 Future connect methods (same `connect(ip,port)`)

| Method | Era |
|--------|-----|
| Manual Tailscale IP | **V1 now** |
| Room code → backend returns host TS IP | Product later |
| Pure LAN IP (same Wi-Fi, no Tailscale) | Easy add |
| ICE/STUN/TURN native | V2 networking |
| QUIC / WebRTC | Research later |

---

## 7. Video & Audio

### 7.1 Goals

- Full **desktop** capture for all use cases  
- GPU encode H.264  
- Hardware decode on client  
- Audio always on (movies need it)  

### 7.2 Presets (start simple)

| Preset | Bitrate | Use |
|--------|---------|-----|
| Low | 4–8 Mbps | weak WAN |
| Medium | 12–20 Mbps | default |
| High | 30–50 Mbps | LAN / fat link |
| Movie-ish | higher bitrate, slightly less “game” encode aggressiveness | optional later |

### 7.3 Latency targets

| Metric | Good | Acceptable |
|--------|------|------------|
| Input feel | local-ish &lt;30 ms RTT | playable &lt;60 ms |
| Glass-to-glass video | &lt;50 ms local | &lt;100 ms good WAN |

Movies tolerate more video delay than competitive games; still prefer low latency so UI feels snappy.

### 7.4 Avoid

- OBS embedding  
- GDI capture  
- Default software encode  
- Putting encode on UI thread  

---

## 8. Shipping Windows `.exe`

Same as before:

```text
React (Vite) → assets
Tauri build → LANPlay.exe + NSIS Setup.exe
```

**Toolchain:** Node, Rust, MSVC Build Tools, WebView2, Tauri CLI.

**Installer / first run checks**

1. WebView2  
2. ViGEmBus (host / input injection)  
3. Tailscale installed + logged in  
4. GPU encoder probe  

Rooms/backend **not** required to ship a useful test build.

---

## 9. Performance Budgets (V1)

| Resource | Host | Client |
|----------|------|--------|
| Idle RAM | &lt; 150 MB | &lt; 150 MB |
| Streaming RAM | &lt; 500 MB extra | &lt; 400 MB |
| CPU with HW encode | &lt; 15% modern | &lt; 10% |

Input bandwidth is tiny — never drop input to save video.

---

## 10. Security (V1 pragmatic)

| Concern | V1 approach |
|---------|-------------|
| Path encryption | Tailscale WireGuard |
| Who can connect | Anyone who knows Tailscale IP + open port on host **while listening** |
| Mitigation | Only Start Host when you want sessions; stop when done; later: simple shared PIN |
| Drivers | Official ViGEmBus only |
| KBM risk | Host can disable remote input |

**Important:** On a tailnet, IP connect is fine for personal testing. Before “invite randos,” add at least a session PIN or accept dialog.

---

## 11. Architecture Decision Records (summary)

### ADR-001 — Tauri over Electron  
Small, native, Rust core.

### ADR-002 — Tailscale for reachability  
Skip NAT hell; abstract transport for V2.

### ADR-003 — Manual Tailscale IP connect for V1  
**No room codes / no backend for testing.** Fastest path to real use.  
Rooms are a future UX layer that still ends in `connect(ip, port)`.

### ADR-004 — ViGEm Xbox 360 for gamepads  
Sunshine-proven; max game compatibility.

### ADR-005 — Keyboard + mouse in V1  
Required for remote desktop / movies / non-game use. Not controller-only.

### ADR-006 — Full desktop capture first  
One pipeline for games + desktop + media.

### ADR-007 — Input channel highest priority  
Latest-state binary packets; drop video first under load.

### ADR-008 — One exe, Host + Client modes  
Simple distribution.

### ADR-009 — No media server  
All media P2P. Future Go backend is optional matchmaking only.

---

## 12. Risks, Unknowns, Trade-offs

| Risk | Mitigation |
|------|------------|
| User types wrong IP | Show host IP big; copy button; recent IPs |
| Tailscale down | Clear error: “Can’t reach host — check Tailscale” |
| Open host without auth | Stop when idle; later PIN |
| Exclusive fullscreen black capture | Prefer borderless; document |
| ViGEm missing | First-run detect + install link |
| KBM layout / games fighting mouse | Relative mouse; focus rules |
| Scope creep (rooms, friends) | Stay on IP connect until stream is solid |

**Trade-off accepted:** Manual IP is less “magic” than room codes, but **perfect for testing and real personal use**. Magic comes after the pipeline works.

---

## 13. Clean Code Rules (vibe coding)

1. Streaming/input logic in **Rust packages**, not React.  
2. Traits: `NetworkTransport`, `CaptureBackend`, `VideoEncoder`, `VirtualPad`, `KbmInjector`.  
3. **No room service imports** anywhere in V1.  
4. `connect(host: IpAddr, port)` is the only join API for now.  
5. Prototypes in `prototypes/`; promote when proven.  
6. Measure latency stages early.  
7. One responsibility per package.  
8. Phase gates: no “quick room codes” mid-stream work.

---

## 14. Phased Development Roadmap

---

### Phase 0 — Research & Scope Lock

**Objective**  
Lock V1: IP connect, desktop stream, pad + KBM, no backend.

**Deliverables**

- This plan (v1.1)  
- Research notes: ViGEm/Sunshine input, capture APIs, encoders, Tailscale IP usage  
- ADRs above  

**Success criteria**

- [ ] Agreed: **no rooms in V1**  
- [ ] Agreed: use cases include games + desktop + movies  
- [ ] Tailscale ping works between your two machines  

---

### Phase 1 — Monorepo + Empty Desktop App

**Objective**  
Bootable Tauri app with Host/Client shell UI (no stream yet).

**Deliverables**

- Monorepo skeleton (`apps/desktop`, `packages/*` stubs)  
- UI: Host | Client switch  
- Host: Start/Stop stub, fake/real Tailscale IP display  
- Client: IP text box + Connect stub  
- CI builds Windows binary  
- **No `apps/backend` required**  

**Success criteria**

- [ ] `LANPlay` window opens  
- [ ] Can switch modes and type an IP  
- [ ] Documented `tauri build` → exe  

---

### Phase 2 — Controller Prototype (Xbox 360)

**Objective**  
Remote pad → ViGEm X360 on host (can use raw IP/UDP before full transport polish).

**Deliverables**

- XInput read, ViGEm inject, `InputPacket`  
- Works over direct UDP to host Tailscale IP  
- Latency log  

**Success criteria**

- [ ] Real game sees remote as Xbox 360  
- [ ] Connect/disconnect clean  
- [ ] Software path low latency on LAN  

**Gate:** Controllers before pretty video.

---

### Phase 3 — Networking Package + Direct IP Session

**Objective**  
Proper `listen` / `connect(ip)` + channels; Tailscale is just the network under the IP.

**Deliverables**

- `NetworkTransport`  
- Host listen, client connect by IP  
- Control hello + version  
- Move controller traffic onto transport  
- Disconnect handling  
- UI: real connect status  

**Success criteria**

- [ ] Enter host TS IP → session up  
- [ ] Controllers work through transport only  
- [ ] Wrong IP / host offline → clear error  

---

### Phase 4 — Desktop Capture Prototype

**Objective**  
GPU capture full desktop; FPS + capture timing.

**Success criteria**

- [ ] Stable ≥ 60 FPS capture on test machine  
- [ ] Notes on fullscreen game edge cases  

---

### Phase 5 — Hardware Encoding

**Objective**  
NVENC (then AMF/QSV) H.264 low-latency encode.

**Success criteria**

- [ ] HW encode works on your GPU  
- [ ] Encode latency in target ballpark  
- [ ] Honest error if no encoder  

---

### Phase 6 — Video Streaming + Useful Desktop

**Objective**  
Host encode → client see desktop. Combined with controller path.

**Success criteria**

- [ ] Connect by IP → see host desktop  
- [ ] Play a game or open a browser movie on host; client sees it  
- [ ] Controller still prioritized  

---

### Phase 7 — Audio Streaming

**Objective**  
System audio loopback → Opus → client (critical for movies).

**Success criteria**

- [ ] Hear host audio continuously  
- [ ] No impact on input latency  

---

### Phase 8 — Keyboard + Mouse + Session Hardening

**Objective**  
Real remote-desktop-style control; clean session lifecycle.

**Deliverables**

- KBM capture/inject  
- Host “Allow remote input” toggle  
- State machine: Idle → Listening / Connecting → Streaming → Ended  
- No zombie ViGEm pads  
- Quality presets  

**Success criteria**

- [ ] Control host desktop from client (open apps, scrub video, etc.)  
- [ ] View-only mode works when input disabled  
- [ ] 30+ min stable session  
- [ ] Forced disconnect cleans up  

*Note: KBM can start as a thin Phase 8; if you need desktop earlier, a minimal mouse can ship in Phase 6.*

---

### Phase 9 — Testing UI Polish (still no rooms)

**Objective**  
Make the IP-connect UX pleasant for daily use — not product marketing fluff.

**Deliverables**

- Big host IP + copy  
- Recent hosts list  
- Overlay (FPS, ping, bitrate, input latency)  
- First-run checklist: Tailscale, ViGEm, encoder  
- Presets + errors in human language  

**Success criteria**

- [ ] You can start a session without opening a terminal  
- [ ] Failures tell you what to fix  

---

### Phase 10 — Performance Pass

**Objective**  
Hit latency budgets; congestion drops video before input.

**Success criteria**

- [ ] Input stable under video stress  
- [ ] Long session no memory leak  

---

### Phase 11 — Packaging `.exe`

**Objective**  
Installer + portable build for your machines / friends who can type an IP.

**Success criteria**

- [ ] Install on clean PC → Host listen → Client IP connect → play/watch  

---

### Phase 12 — Dogfood / Private Alpha (IP-based)

**Objective**  
Real usage: games, movie nights, “use my PC from laptop.”

**Success criteria**

- [ ] Multiple real sessions without you debugging live  
- [ ] Written list of P0 bugs  

---

### Phase 13 — FUTURE: Room Codes & Backend (not now)

**Objective**  
When IP typing is annoying enough:

- Go service issues 6-digit codes  
- Backend only exchanges **host endpoint** (still P2P media)  
- Same stream stack underneath  

**Do not start this phase until Phases 6–8 feel good.**

---

## 15. Build Order Cheat Sheet

```text
Phase 0   Scope lock (IP connect, no rooms)
Phase 1   Tauri shell Host/Client + IP field
Phase 2   Xbox 360 controller path ★
Phase 3   listen / connect(ip) transport
Phase 4   Desktop capture
Phase 5   HW encode
Phase 6   Video stream (games + movies visible)
Phase 7   Audio (movies usable)
Phase 8   KBM + session harden (remote desktop usable)
Phase 9   Testing UI polish
Phase 10  Perf
Phase 11  .exe packaging
Phase 12  Dogfood
Phase 13  Rooms backend — LATER ONLY
```

**Shortest path to “this is useful”:**  
2 → 3 → 6 → 7 → 8  

That gets you: type Tailscale IP → see PC → hear it → control with pad and mouse.

---

## 16. Future (keep in mind)

| Area | Direction |
|------|-----------|
| Connect UX | Room codes, friends, QR, PIN accept |
| Networking | Replace Tailscale with ICE/QUIC; keep same session code |
| Capture | Game window only, multi-monitor picker |
| Codecs | HEVC / AV1 |
| Controllers | Rumble, more pads, DS4 virtual |
| Platforms | Linux, Deck, Android |
| Auth | Session PIN, device pairing |

---

## 17. What “Done” Means for V1 Testing

On two Windows PCs on the same Tailscale network:

1. Host opens LANPlay → **Start Host** → copies `100.x.y.z`  
2. Client opens LANPlay → pastes IP → **Connect**  
3. Client sees host desktop and hears audio  
4. Client controller appears as Xbox 360 on host (games / multiplayer)  
5. Client mouse/keyboard can drive host (desktop, movies, browsing)  
6. Stop either side → clean teardown  

No room codes. No cloud. No matchmaking.

If that loop works, LANPlay is already useful. Room codes are lipstick after that.

---

## 18. Immediate Next Step

Still **no product code until you start a phase.**

1. Finish Phase 0 (Tailscale ping + ViGEm check on your boxes)  
2. Say **“Start Phase 1”** for the Host/Client + IP shell  
3. Then Phase 2 controllers  

---

*Document version: 1.1 — Tailscale IP connect; no rooms; general desktop stream (games + remote use + movies).*
