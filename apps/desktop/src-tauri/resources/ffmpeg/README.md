# FFmpeg (Sunshine-class encode)

LANPlay uses **FFmpeg** for hardware H.264:

- `h264_qsv` — Intel Quick Sync (HD 4000 / Iris / Arc) — same family as Sunshine
- `h264_nvenc` — NVIDIA
- `h264_amf` — AMD
- `libx264` — software ultrafast+zerolatency fallback

## Fetch

```powershell
pwsh -File tools/fetch-ffmpeg.ps1
```

This downloads Gyan **essentials** build into `ffmpeg.exe` here.

Or set `LANPLAY_FFMPEG` to any ffmpeg.exe on PATH.

CI downloads this automatically for nightly builds.
