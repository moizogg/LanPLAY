# Encoders (Windows)

## Active

| Backend | Status | Notes |
|---------|--------|--------|
| **Auto** | **Default** | FFmpeg QSV/NVENC/AMF → MF MFT → FFmpeg x264 → OpenH264 |
| **QSV (FFmpeg)** | **Active** | `h264_qsv` — Sunshine-class Intel path (HD 4000+) |
| **NVENC / AMF (FFmpeg)** | **Active** | `h264_nvenc` / `h264_amf` when GPU present |
| **Hardware H.264 (MF)** | **Fallback** | Media Foundation HW MFT if registered |
| **FFmpeg libx264** | **Soft fallback** | ultrafast + zerolatency (if ffmpeg present) |
| **OpenH264** | **Last resort** | Software; always available |

## Sunshine alignment

Sunshine uses **FFmpeg / native NVENC/AMF/QSV** with GPU-resident textures.

LANPlay now prefers the **same FFmpeg encoder names** (`h264_qsv`, `h264_nvenc`, `h264_amf`) via a bundled `ffmpeg.exe` process, fed by a **D3D11 Video Processor** path:

```
DXGI desktop (GPU)
  → D3D11 VPP scale + RGB→NV12 (GPU)
  → Map encode-sized NV12 only
  → ffmpeg stdin → QSV/NVENC/AMF
  → Annex-B → LPVD
```

Fallback if VPP unavailable: CPU BGRA path (older behavior).

Media Foundation MFT remains a secondary path when FFmpeg is missing or the codec fails.

## Bundle

```powershell
pwsh -File tools/fetch-ffmpeg.ps1
```

CI caches `apps/desktop/src-tauri/resources/ffmpeg/ffmpeg.exe` into portable builds.

Override: `LANPLAY_FFMPEG=C:\path\to\ffmpeg.exe`

## Settings (defaults)

- Encoder: **auto**
- Long edge: **1280**, **30 FPS**, ~**8 Mbps** (raise when on HW)
- Soft path still clamps ~960p30 if only OpenH264 survives

## Next (full Sunshine-class)

1. In-process libavcodec + DXGI → D3D11 → QSV zero-copy (no Map / pipe)
2. Client DXVA decode
3. FEC / recovery polish
