# Moonlight client input → LANPlay mapping

Implemented Moonlight-inspired patterns (not a port of Qt/SDL):

| Moonlight | LANPlay |
|-----------|---------|
| Capture active flag | `CaptureState` + UI Capture/Release |
| Relative mouse while captured | Cursor hide + recenter deltas |
| `raiseAllKeys` on ungrab | Empty KBM packet → host edge KEYUP |
| Ungrab combo | Ctrl+Shift+Alt+Z |
| Click to re-capture | Capture input button |
| Gamepad always (optional background) | Pad always sent while session live |
| Exclusive local mute | Best-effort HID exclusive hold |

Host inject remains Sunshine-style (ViGEm + SendInput).
