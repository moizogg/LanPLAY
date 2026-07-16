# Bundled ViGEm redistributables

These files are **not** committed (binaries). CI and developers fetch them with:

```powershell
powershell -File tools/fetch-vigem-redist.ps1
```

Expected after fetch:

| File | Purpose |
|------|---------|
| `ViGEmClient.dll` | Loaded by LANPlay automatically (no PATH setup) |
| `ViGEmBus_Setup.exe` or `.msi` | One-click driver install from Host UI (UAC once) |
| `THIRD_PARTY_VIGEM.txt` | Attribution |

Users never need to open GitHub for ViGEm.
