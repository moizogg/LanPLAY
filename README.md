# LANPlay

Low-latency desktop streaming over Tailscale — games, remote desktop, movies.

**Phase 2 (current):** remote Xbox 360 controller over Tailscale IP  
- Host / Client modes + Tailscale IP connect  
- Client: XInput → UDP packets  
- Host: UDP → ViGEm virtual Xbox 360 (Sunshine-style)  
- No video/audio stream yet  

See [`docs/architecture/LANPlay-Full-Plan.md`](docs/architecture/LANPlay-Full-Plan.md) for the full roadmap.

---

## Repo layout

```text
apps/desktop/          Tauri 2 + React + TypeScript UI
packages/
  shared/              Types, ports, errors
  protocol/            Wire protocol version
  networking/          Transport trait + Phase 1 stub
  controllers/         (Phase 2+)
  video/               (Phase 4+)
  audio/               (Phase 7+)
  input/               KBM (Phase 8+)
  overlay/             Metrics model
docs/
```

---

## Prerequisites (Windows)

1. **Node.js** 20+ ([nodejs.org](https://nodejs.org))
2. **Rust** stable ([rustup.rs](https://rustup.rs)) — MSVC toolchain  
3. **Visual Studio 2022 Build Tools** with workload **Desktop development with C++**  
   (MSVC + Windows SDK)  
4. **WebView2** — usually already on Windows 10/11  
5. **Tailscale** (optional for Phase 1 IP detection) — [tailscale.com](https://tailscale.com)

---

## Run (dev)

```powershell
cd apps/desktop
npm install
npm run tauri dev
```

This opens the LANPlay window. You can:

| Mode | Actions |
|------|---------|
| **Host** | See Tailscale IP, **Start Host** / **Stop Host**, allow remote input toggle |
| **Client** | Enter host IP (`100.x.y.z`), **Connect** / **Disconnect**, recent IPs |

Phase 2 opens a **real UDP controller path** on port `47801`.

### Virtual gamepad (no separate GitHub downloads)

LANPlay **bundles** ViGEm:

| Piece | User experience |
|-------|-----------------|
| **ViGEmClient** | **Compiled into `lanplay.exe`** (static, Sunshine-style) — no separate DLL |
| ViGEmBus **driver** | Setup ships in `vigem\` — Host UI **Install gamepad support** (one-time UAC) |

Windows does not allow a normal app to load a kernel driver with zero install. We hide that: **one button inside LANPlay**, users never visit GitHub.

CI runs `tools/fetch-vigem-redist.ps1` so release builds include both files.

### Quick controller test

1. Host: if prompted, **Install gamepad support** once → **Start Host** → copy Tailscale IP  
2. Client: paste IP → **Connect** → plug Xbox/XInput pad  
3. Host: Game Controllers panel or any game should see an extra Xbox 360 pad

---

## Build `.exe` (recommended: GitHub Actions)

You do **not** need Visual Studio / MSVC on your PC for Phase 1.

**Gitpod / Codespaces (Linux) cannot produce a normal Windows Tauri `.exe` easily.**  
Use **GitHub Actions `windows-latest`** instead — free online Windows builder.

### Online build steps

1. Create a GitHub repo and push this project:
   ```powershell
   cd D:\Coding\LanPLAY
   git init
   git add .
   git commit -m "Phase 1: LANPlay desktop shell"
   git branch -M main
   git remote add origin https://github.com/YOUR_USER/LanPLAY.git
   git push -u origin main
   ```
2. Open the repo → **Actions** → workflow **CI** → wait for green  
3. Download artifact **`lanplay-windows`** (contains `lanplay.exe` + NSIS installer)

You can also re-run builds via **Actions → CI → Run workflow**.

### CI is optimized to be faster

| Before (slow) | Now |
|---------------|-----|
| `cargo test` (full **debug** compile) + `tauri build` (full **release** compile) = **2× work** | **One** release compile only |
| NSIS installer every time | **exe only** by default (`--no-bundle`) |
| Rebuild ViGEm every run | **Cached** after first fetch |
| Rust crates recompiled from scratch | **rust-cache** shared key `lanplay-win-release` |

**First** green run after cache clear can still be ~15–25 min.  
**Later** runs with a warm cache are often **much** shorter (often ~5–12 min if only a few files changed).

Optional (Actions → Run workflow):
- `full_installer` — also build NSIS setup  
- `run_tests` — cargo test (slow; second compile)

### Local build (optional, needs MSVC)

```powershell
cd apps/desktop
npm install
npm run tauri build
```

Outputs (typical for this monorepo — workspace `target/` at **repo root**):

- `target/release/lanplay.exe`
- `target/release/bundle/nsis/LANPlay_*_x64-setup.exe`

---

## Workspace (Rust packages)

From repo root (with MSVC available):

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cargo test --workspace
cargo check --workspace
```

---

## Default ports

| Port  | Role            |
|-------|-----------------|
| 47800 | Control / session |
| 47801 | Media + input   |

---

## Next phase

**Phase 3 — Networking polish:** session handshake, better errors, disconnect detection.  
Then video capture/encode.
