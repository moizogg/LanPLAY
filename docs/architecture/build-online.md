# Building LANPlay online (no local MSVC)

## Why not Gitpod?

| Environment | Good for | Windows Tauri `.exe` |
|-------------|----------|----------------------|
| **Gitpod** | Linux apps, web | Poor — Linux container; Windows cross-compile for Tauri is painful |
| **GitHub Codespaces** | Linux/dev | Same issue unless you use a Windows environment |
| **GitHub Actions `windows-latest`** | CI + artifacts | **Best** — real Windows, MSVC, WebView2-ready |

**Rule:** I write code on your machine; **GitHub Actions compiles the `.exe`.**

## Workflow

File: `.github/workflows/ci.yml`

On every push to `main` / `master` (and manual dispatch):

1. Install Node + Rust on `windows-latest`
2. `npm ci` + frontend build
3. `cargo test --workspace`
4. `npm run tauri build`
5. Upload artifact **`lanplay-windows`**:
   - `lanplay.exe`
   - NSIS setup `.exe` (if bundle succeeded)

## What you do

1. Push repo to GitHub  
2. Wait for Actions  
3. Download artifact  
4. Run on Windows  

## What I do

- Write / change code in the monorepo  
- Keep CI green  
- Never block on your local Visual Studio install  

## Local UI-only checks (no Rust link)

If you want to glance at the React shell without Tauri:

```powershell
cd apps/desktop
npm install
npm run dev
```

This is **browser-only** — Tauri `invoke` calls will fail in a normal browser.  
Real Host/Client shell needs the built or `tauri dev` app (or CI artifact).
