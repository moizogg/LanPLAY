# Bundled ViGEm redistributables

These files are **not** committed (binaries). CI and developers fetch them with:

```powershell
powershell -File tools/fetch-vigem-redist.ps1
```

Expected after fetch:

| File | Purpose |
|------|---------|
| `ViGEmBus_Setup.exe` or `.msi` | One-click driver install from Host UI (UAC once) |
| `THIRD_PARTY_VIGEM.txt` | Attribution |

**ViGEmClient is no longer a DLL** — it is compiled into `lanplay.exe` from `third-party/ViGEmClient`.

Users never need to open GitHub for ViGEm.
