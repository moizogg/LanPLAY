# LANPlay

Low-latency desktop streaming over Tailscale — games, remote desktop, movies.

**Phase 1 (current):** desktop shell only  
- Host / Client modes  
- Detect + copy Tailscale IP  
- Start Host / Connect by IP stubs  
- No real stream yet  

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

Phase 1 uses a **stub transport** — Connect succeeds without a real network session so the UI can be tested.

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

### Local build (optional, needs MSVC)

```powershell
cd apps/desktop
npm install
npm run tauri build
```

Outputs (typical):

- `apps/desktop/src-tauri/target/release/lanplay.exe`
- `apps/desktop/src-tauri/target/release/bundle/nsis/LANPlay_*_x64-setup.exe`

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

**Phase 2 — Controllers:** client XInput → host ViGEm Xbox 360.
