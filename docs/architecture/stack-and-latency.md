# Stack choice vs latency (LANPlay)

## Short answer

**Rust + Tauri + React is a strong stack for low-latency streaming.**  
Rewriting the whole product in C++ would **not** meaningfully reduce lag for your architecture, and would cost a lot of time.

## Where lag actually comes from

| Stage | Typical cost | Language impact |
|-------|--------------|-----------------|
| Network RTT (Tailscale / internet) | **5–80+ ms** | Almost none |
| GPU capture | 1–3 ms | Native code only (Rust or C++) |
| HW encode (NVENC/AMF/QSV) | 2–8 ms | Vendor SDK — same from Rust/C++ |
| Decode + present | 1–8 ms | Same |
| Controller inject (ViGEm) | **&lt;1 ms** | Same C API either way |
| UI (React) | Not on the hot path | Irrelevant if streaming is in Rust |

If input or video feels laggy, it is almost always **network, encoder presets, vsync, or packet prioritization** — not “we used Rust instead of C++.”

## Why current stack is “goated” for LANPlay

1. **Hot path is native Rust** (and C++ only where we statically link ViGEmClient). React never touches gamepad packets or encode loops.
2. **Rust** matches C++ for this class of systems work: sockets, threads, unsafe FFI, zero-cost abstractions when written carefully.
3. **Tauri** keeps a small binary and RAM vs Electron — good for a gaming utility.
4. **React** only for menus / status — fine, and faster to ship UX.
5. **Sunshine is C++** because it grew that way historically — not because C++ is required for low latency. Their controller path is still **ViGEm**, same as us.

## When C++ would matter

- You already have a huge C++ codebase (you don’t).
- You need a library that only has a C++ API with no C boundary (rare).
- Extreme micro-optimizations after measuring (almost never first bottleneck).

We **do** compile ViGEmClient’s C++ sources **into** the Rust app (static link), Sunshine-style. That’s the right place for C++, not rewriting the UI.

## Reliability

Reliability comes from:

- Correct packet design (latest-state input)
- Input channel priority over video
- Clean ViGEm plug/unplug
- Driver packaging
- Measuring latency stages  

…not from “everything is one C++ binary.”

## Decision

| Layer | Choice |
|-------|--------|
| App shell / UI | Tauri + React |
| Session, net, video, audio, input | Rust |
| ViGEmClient | Static C++ (vendored) linked into Rust |
| ViGEmBus | Official kernel driver, one-time install |

**Do not rewrite LANPlay in pure C++ for lag.** Optimize the pipeline and packaging instead.
