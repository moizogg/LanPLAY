# Why Sunshine feels smooth and LANPlay feels laggy

This is **not mainly “potato PC”**. On localhost LANPlay still pays a **software video pipeline** tax Sunshine almost never pays on a real gaming host.

## What you feel

When you move the mouse and the picture lags / blurs / FPS drops, that is almost always:

**mouse → host desktop → capture → encode → network → decode → present**

(not just “input UDP delay”). Input itself is ~few ms. **Video is the heavy part.**

## Sunshine pipeline (what makes it snappy)

From `Sunshine-master` (Windows path):

```
DXGI GPU texture
  → stay on GPU (shared D3D11 textures)
  → GPU shader RGB→NV12
  → NVENC / AMF / QSV (hardware encode, ultra-low-latency)
  → only compressed bits hit CPU
  → RTP + FEC + paced UDP
  → Moonlight HW decode + present
```

Key Sunshine choices:

| Area | Sunshine |
|------|----------|
| Capture | DXGI into **GPU** textures (`display_vram.cpp`) |
| Color convert | **GPU HLSL** shaders |
| Encode | **NVENC/AMF/QSV**, no B-frames, no lookahead, ~1-frame VBV |
| CPU work | Mostly bitstream only |
| Cursor | GPU-blended from DXGI pointer shape |
| Recovery | On-demand IDR + ref invalidation, FEC |
| Protocol | Full GameStream / Moonlight stack |

SW encode in Sunshine is a **fallback** they keep trying to escape.

## LANPlay pipeline today (what hurts)

```
DXGI → Map staging → full-frame BGRA on CPU
  → bilinear scale on CPU
  → BGRA→YUV on CPU (OpenH264)
  → OpenH264 software encode (CPU)
  → raw UDP fragments (no FEC)
  → client OpenH264 software decode (CPU)
  → CPU present (minifb)
```

| Stage | Cost (typical) | Effect |
|-------|----------------|--------|
| GPU→CPU Map | 2–10+ ms | Always |
| Scale + YUV | several ms | Soft picture if res low |
| **OpenH264 encode** | **15–50+ ms** (you measured ~40–50) | FPS collapse, mush |
| Decode SW | several ms | Extra lag |
| No FEC / simple UDP | packet loss → corrupt/blur frames | “sometimes blur” |
| Soft encode under load | drops frames | stuttery mouse on video |

So mouse feels laggy because **you see the old frame** until the next expensive encode finishes — not because the pad/KBM packet is slow.

## Gap list (Sunshine capabilities we do not have yet)

### Must-have for Sunshine-like feel

1. **Hardware encode (NVENC / AMF / QSV)** — biggest win  
2. **GPU path**: capture texture → GPU convert → HW encoder (no full-frame Map)  
3. **Low-latency encode presets**: ULL, no B-frames, no lookahead, small VBV  
4. **Client HW decode** (DXVA / D3D11) if possible  

### Important polish

5. FEC / better packetization (or at least retransmit/IDR on loss)  
6. DXGI cursor plane like Sunshine (we draw OS cursor in software — OK-ish)  
7. Capture/encode concurrency (don’t block capture while encode runs)  
8. Proper bitrate adaptive under load  

### Already roughly OK / secondary

- UDP input path for pad/KBM  
- Relative mouse + hide local cursor  
- Settings for res/bitrate/fps  
- Stream window present (better than JPEG UI)

## Potato PC?

| Machine | Software OpenH264 @ 1080p | HW NVENC @ 1080p |
|---------|---------------------------|------------------|
| Weak CPU | Bad (encode dominates) | Often still fine |
| Strong CPU | Better, still worse than Sunshine | Excellent |
| NVIDIA + NVENC | Still limited by CPU path | **Sunshine-class** |

So: **weak CPU makes our current design much worse**, but **even a strong PC will feel worse than Sunshine** until we leave the CPU encode path.

## Priority order (to match Sunshine)

1. **NVENC (then AMF/QSV)** on host — replace OpenH264 for real streams  
2. Keep DXGI surface on GPU into encoder  
3. Client: prefer HW decode  
4. FEC / recovery  
5. Only then more UI polish  

Until (1)+(2), Settings knobs only **trade** sharpness vs lag (lower res/FPS = less mush, not Sunshine-smooth).

## Critical detail for Intel HD 4000

Sunshine does **not** use Windows Media Foundation hardware MFTs for QSV.
It uses **FFmpeg `h264_qsv` / Media SDK** with **D3D11 zero-copy** textures
(`display_vram.cpp` → GPU NV12 convert → QSV).

LANPlay’s “hardware” path is **Media Foundation H.264 MFT**. On many HD 4000
driver installs that MFT is **missing**, even though Sunshine QSV works.
Result: LANPlay falls back to **OpenH264 CPU** → mush; Sunshine stays smooth.

### What we improved without full GPU zero-copy

- Map GPU→CPU **only on encode ticks** (not every DXGI frame)
- Soft software profile ~**960p30** when HW MFT unavailable
- Fast nearest scale + frame skip under load
- Re-encode last desktop + fresh cursor when DXGI is idle (mouse stays live)
- Probe UI explains MF vs Sunshine/QSV

### Still required for real Sunshine parity

- **FFmpeg/libavcodec `h264_qsv`** (or NVENC/AMF SDK) with D3D11 frames
- GPU scale/color convert (no full-frame Map)
- Client DXVA/D3D11 decode
