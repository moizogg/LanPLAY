# Encoders (Windows)

## Active

| Backend | Status | Notes |
|---------|--------|--------|
| **Auto** | **Default** | Prefers hardware MF H.264, else OpenH264 |
| **Hardware H.264 (MF)** | **Active** | Media Foundation HW MFT → NVENC/AMF/QSV silicon when present |
| **OpenH264** | **Fallback** | Software; always available |

## Sunshine alignment

Sunshine uses **native NVENC/AMF/QSV SDKs** with GPU-resident textures.  
LANPlay v1 HW path uses **Windows Media Foundation hardware MFT** + low-latency `ICodecAPI` flags:

- `CODECAPI_AVLowLatencyMode`
- CBR mean bitrate
- Force keyframe on demand

Still converts BGRA→NV12 on CPU before MF (full GPU path is next). That alone is a large win vs pure OpenH264 encode.

## Settings (defaults)

- Encoder: **auto**
- Long edge: **1920**
- ~**25 Mbps**, **60 FPS** target
- Bilinear scale when downscaling

## Next (true Sunshine-class)

1. DXGI texture → D3D11 VideoProcessor NV12 → MF/D3D manager (no full Map)
2. Native `nvEncodeAPI64.dll` session (ULL preset, 1-frame VBV)
3. Client DXVA decode
