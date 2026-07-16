import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  AppMode,
  CaptureSnapshot,
  CaptureStatus,
  ClientStatus,
  ClientVideoSnapshot,
  ControllerStats,
  EncoderOption,
  HostStatus,
  ResolutionPreset,
  TailscaleInfo,
  VideoSettings,
  VigemBundleStatus,
} from "./types";

const DEFAULT_VIDEO: VideoSettings = {
  outputIndex: 0,
  fps: 30,
  bitrateKbps: 20000,
  resolutionMode: "auto",
  maxEdge: 1920,
  width: 1920,
  height: 1080,
  encoder: "openh264",
};

const RECENT_IPS_KEY = "lanplay.recentHostIps";

function loadRecentIps(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_IPS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed)
      ? parsed.filter((x): x is string => typeof x === "string").slice(0, 5)
      : [];
  } catch {
    return [];
  }
}

function saveRecentIp(ip: string) {
  const next = [ip, ...loadRecentIps().filter((x) => x !== ip)].slice(0, 5);
  localStorage.setItem(RECENT_IPS_KEY, JSON.stringify(next));
}

function stateBadgeClass(state: string): string {
  switch (state) {
    case "listening":
    case "streaming":
      return "bg-emerald-500/15 text-emerald-300 ring-emerald-500/30";
    case "connecting":
    case "waiting_approval":
      return "bg-amber-500/15 text-amber-200 ring-amber-500/30";
    case "error":
      return "bg-rose-500/15 text-rose-300 ring-rose-500/30";
    default:
      return "bg-slate-500/15 text-slate-300 ring-slate-500/30";
  }
}

export default function App() {
  const [mode, setMode] = useState<AppMode>("host");
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [tailscale, setTailscale] = useState<TailscaleInfo | null>(null);
  const [host, setHost] = useState<HostStatus | null>(null);
  const [client, setClient] = useState<ClientStatus | null>(null);
  const [stats, setStats] = useState<ControllerStats | null>(null);
  const [vigemBundle, setVigemBundle] = useState<VigemBundleStatus | null>(
    null,
  );
  const [capture, setCapture] = useState<CaptureStatus | null>(null);
  const [desktopCapture, setDesktopCapture] = useState<CaptureSnapshot | null>(
    null,
  );
  const [clientVideo, setClientVideo] = useState<ClientVideoSnapshot | null>(
    null,
  );
  const [hostIp, setHostIp] = useState("");
  const [controlPort, setControlPort] = useState(47800);
  const [mediaPort, setMediaPort] = useState(47801);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [recentIps, setRecentIps] = useState<string[]>(() => loadRecentIps());
  const [videoSettings, setVideoSettings] =
    useState<VideoSettings>(DEFAULT_VIDEO);
  const [encoderOptions, setEncoderOptions] = useState<EncoderOption[]>([]);
  const [resPresets, setResPresets] = useState<ResolutionPreset[]>([]);
  const [settingsSaved, setSettingsSaved] = useState<string | null>(null);
  const [settingsDirty, setSettingsDirty] = useState(false);

  /** Session/controller metrics only — safe to poll often (no external CLI). */
  const refreshLive = useCallback(async () => {
    try {
      const [h, c, st, cap, desk, vid] = await Promise.all([
        invoke<HostStatus>("get_host_status"),
        invoke<ClientStatus>("get_client_status"),
        invoke<ControllerStats>("get_controller_stats"),
        invoke<CaptureStatus>("get_input_capture"),
        invoke<CaptureSnapshot>("get_capture_stats"),
        invoke<ClientVideoSnapshot>("get_client_video"),
      ]);
      setHost(h);
      setClient(c);
      setStats(st);
      setCapture(cap);
      setDesktopCapture(desk);
      setClientVideo(vid);
      setControlPort(h.controlPort);
      setMediaPort(h.mediaPort);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  /** Heavier: Tailscale CLI + ViGEm probe (cached on Rust side; still not every tick). */
  const refreshSlow = useCallback(async (freshTailscale = false) => {
    try {
      const [info, ts, vb] = await Promise.all([
        invoke<AppInfo>("get_app_info"),
        invoke<TailscaleInfo>("get_tailscale_info", { fresh: freshTailscale }),
        invoke<VigemBundleStatus>("get_vigem_bundle_status"),
      ]);
      setAppInfo(info);
      setTailscale(ts);
      setVigemBundle(vb);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadSettings = useCallback(async () => {
    try {
      const [vs, enc, presets] = await Promise.all([
        invoke<VideoSettings>("get_video_settings"),
        invoke<EncoderOption[]>("get_encoder_options"),
        invoke<ResolutionPreset[]>("get_resolution_presets"),
      ]);
      setVideoSettings(vs);
      setEncoderOptions(enc);
      setResPresets(presets);
      setSettingsDirty(false);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const refresh = useCallback(async () => {
    await Promise.all([refreshLive(), refreshSlow(false), loadSettings()]);
  }, [refreshLive, refreshSlow, loadSettings]);

  useEffect(() => {
    void refresh();
    // Live metrics only — do NOT spawn tailscale every 500ms
    const liveId = window.setInterval(() => {
      void refreshLive();
    }, 500);
    // Occasional slow refresh (Tailscale / ViGEm status)
    const slowId = window.setInterval(() => {
      void refreshSlow(false);
    }, 10_000);
    return () => {
      window.clearInterval(liveId);
      window.clearInterval(slowId);
    };
  }, [refresh, refreshLive, refreshSlow]);

  function patchVideo(patch: Partial<VideoSettings>) {
    setVideoSettings((prev) => ({ ...prev, ...patch }));
    setSettingsDirty(true);
    setSettingsSaved(null);
  }

  async function onSaveSettings() {
    setBusy(true);
    setError(null);
    setSettingsSaved(null);
    try {
      const saved = await invoke<VideoSettings>("set_video_settings", {
        settings: videoSettings,
      });
      setVideoSettings(saved);
      setSettingsDirty(false);
      setSettingsSaved(
        hostListening
          ? "Saved. Stop Host and Start Host again to apply encode settings."
          : "Saved. Applied on next Start Host.",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onResetSettings() {
    setBusy(true);
    setError(null);
    try {
      const saved = await invoke<VideoSettings>("set_video_settings", {
        settings: DEFAULT_VIDEO,
      });
      setVideoSettings(saved);
      setSettingsDirty(false);
      setSettingsSaved("Reset to defaults.");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  function applyPreset(presetId: string) {
    const p = resPresets.find((x) => x.id === presetId);
    if (!p) return;
    patchVideo({
      resolutionMode: p.mode,
      width: p.width,
      height: p.height,
      maxEdge: p.maxEdge,
    });
  }

  async function onStartHost() {
    setBusy(true);
    setError(null);
    try {
      const status = await invoke<HostStatus>("start_host");
      setHost(status);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onStopHost() {
    setBusy(true);
    setError(null);
    try {
      const status = await invoke<HostStatus>("stop_host");
      setHost(status);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onToggleInput(allow: boolean) {
    try {
      const status = await invoke<HostStatus>("set_allow_remote_input", {
        allow,
      });
      setHost(status);
    } catch (e) {
      setError(String(e));
    }
  }

  async function onConnect() {
    setBusy(true);
    setError(null);
    try {
      const status = await invoke<ClientStatus>("connect_client", {
        hostIp: hostIp.trim(),
        controlPort,
        mediaPort,
      });
      setClient(status);
      saveRecentIp(hostIp.trim());
      setRecentIps(loadRecentIps());
    } catch (e) {
      setError(String(e));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function onDisconnect() {
    setBusy(true);
    setError(null);
    try {
      const status = await invoke<ClientStatus>("disconnect_client");
      setClient(status);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function copyIp() {
    if (!tailscale?.ip) return;
    try {
      await navigator.clipboard.writeText(tailscale.ip);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setError("Could not copy IP to clipboard.");
    }
  }

  async function onToggleCapture() {
    setError(null);
    try {
      const status = await invoke<CaptureStatus>("toggle_input_capture");
      setCapture(status);
    } catch (e) {
      setError(String(e));
    }
  }

  async function onSetCapture(active: boolean) {
    setError(null);
    try {
      const status = await invoke<CaptureStatus>("set_input_capture", {
        active,
      });
      setCapture(status);
    } catch (e) {
      setError(String(e));
    }
  }

  async function onRespondJoin(accept: boolean) {
    setBusy(true);
    setError(null);
    try {
      const status = await invoke<HostStatus>("respond_to_join", { accept });
      setHost(status);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onInstallVigem() {
    setBusy(true);
    setError(null);
    try {
      const msg = await invoke<string>("install_vigem_driver");
      await refresh();
      setError(null);
      // Show success in the same banner style as soft info
      alert(msg);
    } catch (e) {
      setError(String(e));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  const hostListening =
    host?.state === "listening" || host?.state === "streaming";
  const clientActive =
    client?.state === "connecting" ||
    client?.state === "waiting_approval" ||
    client?.state === "streaming";

  return (
    <div className="min-h-full bg-[radial-gradient(ellipse_at_top,_#122033_0%,_#070b12_55%)]">
      <div className="mx-auto flex min-h-full max-w-3xl flex-col gap-6 px-6 py-8">
        <header className="flex items-start justify-between gap-4">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.2em] text-cyan-400/80">
              Phase {appInfo?.phase ?? 2}
            </p>
            <h1 className="mt-1 text-3xl font-bold tracking-tight text-white">
              LANPlay
            </h1>
            <p className="mt-1 max-w-md text-sm text-slate-400">
              After Accept: keyboard/mouse only while LANPlay is focused on
              the client. Controller is sent to host (local pad held
              best-effort).
            </p>
          </div>
          <div className="rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-right text-xs text-slate-400">
            <div>v{appInfo?.version ?? "…"}</div>
            <div>protocol {appInfo?.protocolVersion ?? "…"}</div>
          </div>
        </header>

        <div className="inline-flex w-fit rounded-xl border border-white/10 bg-black/30 p-1">
          {(["host", "client", "settings"] as const).map((m) => (
            <button
              key={m}
              type="button"
              onClick={() => setMode(m)}
              className={`rounded-lg px-5 py-2 text-sm font-semibold capitalize transition ${
                mode === m
                  ? "bg-cyan-500 text-slate-950 shadow"
                  : "text-slate-300 hover:text-white"
              }`}
            >
              {m}
            </button>
          ))}
        </div>

        {error && (
          <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 px-4 py-3 text-sm text-rose-200">
            {error}
          </div>
        )}

        {/* Live controller metrics */}
        <section className="grid grid-cols-2 gap-3 rounded-2xl border border-white/10 bg-white/[0.03] p-4 sm:grid-cols-4">
          <Metric
            label="Role"
            value={stats?.role ?? "—"}
          />
          <Metric
            label="Packets"
            value={stats ? String(stats.packets) : "—"}
          />
          <Metric
            label="Input latency"
            value={
              stats && stats.role === "host"
                ? `${stats.inputLatencyMs.toFixed(1)} ms`
                : "—"
            }
          />
          <Metric
            label="Pad / ViGEm"
            value={`${stats?.padConnected ? "pad●" : "pad○"} ${stats?.vigemOk ? "vigem●" : "vigem○"}`}
          />
        </section>

        {mode === "settings" ? (
          <section className="space-y-5 rounded-2xl border border-white/10 bg-white/[0.03] p-6 shadow-xl shadow-black/30">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h2 className="text-lg font-semibold text-white">
                  Video &amp; encode settings
                </h2>
                <p className="mt-1 text-sm text-slate-400">
                  Sunshine-style host knobs. Changes apply on{" "}
                  <span className="text-slate-200">Start Host</span> (restart
                  host if already running).
                </p>
              </div>
              {settingsDirty && (
                <span className="rounded-full bg-amber-500/15 px-3 py-1 text-xs font-medium text-amber-200 ring-1 ring-amber-500/30">
                  unsaved
                </span>
              )}
            </div>

            {/* Resolution */}
            <div className="space-y-3 rounded-xl border border-white/10 bg-black/30 p-4">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                Resolution
              </p>
              <label className="block text-sm text-slate-300">
                Preset
                <select
                  className="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-3 py-2 text-sm text-white"
                  value={
                    videoSettings.resolutionMode === "auto"
                      ? videoSettings.maxEdge >= 1920
                        ? "auto-1920"
                        : videoSettings.maxEdge <= 960
                          ? "auto-960"
                          : "auto-1280"
                      : resPresets.find(
                          (p) =>
                            p.mode === "fixed" &&
                            p.width === videoSettings.width &&
                            p.height === videoSettings.height &&
                            p.id !== "custom",
                        )?.id ?? "custom"
                  }
                  onChange={(e) => applyPreset(e.target.value)}
                >
                  {resPresets.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                </select>
              </label>

              <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
                <label className="block text-sm text-slate-300">
                  Mode
                  <select
                    className="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-3 py-2 text-sm text-white"
                    value={videoSettings.resolutionMode}
                    onChange={(e) =>
                      patchVideo({
                        resolutionMode:
                          e.target.value === "fixed" ? "fixed" : "auto",
                      })
                    }
                  >
                    <option value="auto">Auto (max edge)</option>
                    <option value="fixed">Fixed size</option>
                  </select>
                </label>
                {videoSettings.resolutionMode === "auto" ? (
                  <label className="block text-sm text-slate-300">
                    Max edge (px)
                    <input
                      type="number"
                      min={320}
                      max={3840}
                      step={2}
                      className="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-3 py-2 font-mono text-sm text-white"
                      value={videoSettings.maxEdge}
                      onChange={(e) =>
                        patchVideo({
                          maxEdge: Number(e.target.value) || 1280,
                        })
                      }
                    />
                  </label>
                ) : (
                  <>
                    <label className="block text-sm text-slate-300">
                      Width
                      <input
                        type="number"
                        min={160}
                        max={3840}
                        step={2}
                        className="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-3 py-2 font-mono text-sm text-white"
                        value={videoSettings.width}
                        onChange={(e) =>
                          patchVideo({
                            width: Number(e.target.value) || 1280,
                          })
                        }
                      />
                    </label>
                    <label className="block text-sm text-slate-300">
                      Height
                      <input
                        type="number"
                        min={160}
                        max={2160}
                        step={2}
                        className="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-3 py-2 font-mono text-sm text-white"
                        value={videoSettings.height}
                        onChange={(e) =>
                          patchVideo({
                            height: Number(e.target.value) || 720,
                          })
                        }
                      />
                    </label>
                  </>
                )}
              </div>
              <p className="text-[11px] text-slate-600">
                Default is sharp 1080p-class @ 20 Mbps. If CPU melts, drop to
                auto-1280 or 30 FPS. Stream window shows full encode res.
              </p>
            </div>

            {/* FPS + bitrate */}
            <div className="space-y-3 rounded-xl border border-white/10 bg-black/30 p-4">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                Frame rate &amp; bitrate
              </p>
              <div className="grid grid-cols-2 gap-3">
                <label className="block text-sm text-slate-300">
                  Target FPS
                  <input
                    type="number"
                    min={5}
                    max={240}
                    className="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-3 py-2 font-mono text-sm text-white"
                    value={videoSettings.fps}
                    onChange={(e) =>
                      patchVideo({ fps: Number(e.target.value) || 30 })
                    }
                  />
                </label>
                <label className="block text-sm text-slate-300">
                  Bitrate (kbps)
                  <input
                    type="number"
                    min={500}
                    max={100000}
                    step={500}
                    className="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-3 py-2 font-mono text-sm text-white"
                    value={videoSettings.bitrateKbps}
                    onChange={(e) =>
                      patchVideo({
                        bitrateKbps: Number(e.target.value) || 8000,
                      })
                    }
                  />
                </label>
              </div>
              <div className="flex flex-wrap gap-2">
                {[15, 30, 60, 90, 120].map((f) => (
                  <button
                    key={f}
                    type="button"
                    onClick={() => patchVideo({ fps: f })}
                    className={`rounded-lg px-2.5 py-1 text-xs font-medium ring-1 transition ${
                      videoSettings.fps === f
                        ? "bg-cyan-500/20 text-cyan-200 ring-cyan-500/40"
                        : "bg-white/5 text-slate-300 ring-white/10 hover:bg-white/10"
                    }`}
                  >
                    {f} FPS
                  </button>
                ))}
              </div>
              <div className="flex flex-wrap gap-2">
                {[2000, 5000, 8000, 12000, 20000, 40000].map((b) => (
                  <button
                    key={b}
                    type="button"
                    onClick={() => patchVideo({ bitrateKbps: b })}
                    className={`rounded-lg px-2.5 py-1 text-xs font-medium ring-1 transition ${
                      videoSettings.bitrateKbps === b
                        ? "bg-cyan-500/20 text-cyan-200 ring-cyan-500/40"
                        : "bg-white/5 text-slate-300 ring-white/10 hover:bg-white/10"
                    }`}
                  >
                    {b >= 1000 ? `${b / 1000} Mbps` : `${b} kbps`}
                  </button>
                ))}
              </div>
            </div>

            {/* Encoder */}
            <div className="space-y-3 rounded-xl border border-white/10 bg-black/30 p-4">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                Encoder
              </p>
              <div className="space-y-2">
                {encoderOptions.map((enc) => (
                  <label
                    key={enc.id}
                    className={`flex cursor-pointer items-start gap-3 rounded-lg border px-3 py-2.5 transition ${
                      videoSettings.encoder === enc.id
                        ? "border-cyan-500/40 bg-cyan-500/10"
                        : "border-white/10 bg-black/20 hover:border-white/20"
                    } ${!enc.available ? "opacity-70" : ""}`}
                  >
                    <input
                      type="radio"
                      name="encoder"
                      className="mt-1"
                      checked={videoSettings.encoder === enc.id}
                      onChange={() => patchVideo({ encoder: enc.id })}
                    />
                    <span className="min-w-0 flex-1">
                      <span className="flex flex-wrap items-center gap-2 text-sm font-medium text-white">
                        {enc.name}
                        {enc.hardware && (
                          <span className="rounded bg-violet-500/20 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-violet-200">
                            HW
                          </span>
                        )}
                        {!enc.available && (
                          <span className="rounded bg-slate-500/30 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-slate-300">
                            soon
                          </span>
                        )}
                      </span>
                      <span className="mt-0.5 block text-xs text-slate-500">
                        {enc.detail}
                      </span>
                    </span>
                  </label>
                ))}
              </div>
            </div>

            {/* Display */}
            <div className="space-y-3 rounded-xl border border-white/10 bg-black/30 p-4">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                Capture
              </p>
              <label className="block text-sm text-slate-300">
                Display output index
                <input
                  type="number"
                  min={0}
                  max={7}
                  className="mt-1 w-full max-w-[8rem] rounded-lg border border-white/10 bg-slate-900 px-3 py-2 font-mono text-sm text-white"
                  value={videoSettings.outputIndex}
                  onChange={(e) =>
                    patchVideo({
                      outputIndex: Number(e.target.value) || 0,
                    })
                  }
                />
                <span className="mt-1 block text-[11px] text-slate-600">
                  0 = primary monitor. Change if you stream a second display.
                </span>
              </label>
            </div>

            {settingsSaved && (
              <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/10 px-4 py-3 text-sm text-emerald-200">
                {settingsSaved}
              </div>
            )}

            <div className="flex flex-wrap gap-3">
              <button
                type="button"
                disabled={busy || !settingsDirty}
                onClick={() => void onSaveSettings()}
                className="rounded-xl bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400 disabled:opacity-50"
              >
                Save settings
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => void onResetSettings()}
                className="rounded-xl border border-white/10 bg-white/5 px-5 py-2.5 text-sm font-medium text-slate-200 transition hover:bg-white/10 disabled:opacity-50"
              >
                Reset defaults
              </button>
            </div>
          </section>
        ) : mode === "host" ? (
          <section className="space-y-4 rounded-2xl border border-white/10 bg-white/[0.03] p-6 shadow-xl shadow-black/30">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-lg font-semibold text-white">Host mode</h2>
              {host && (
                <span
                  className={`rounded-full px-3 py-1 text-xs font-medium ring-1 ${stateBadgeClass(host.state)}`}
                >
                  {host.state}
                </span>
              )}
            </div>

            <div className="rounded-xl border border-white/10 bg-black/30 p-4">
              <p className="text-xs uppercase tracking-wide text-slate-500">
                Your Tailscale IP
              </p>
              <div className="mt-2 flex flex-wrap items-center gap-3">
                <code className="text-2xl font-semibold tracking-tight text-cyan-300">
                  {tailscale?.ip ?? "— not detected —"}
                </code>
                <button
                  type="button"
                  disabled={!tailscale?.ip}
                  onClick={() => void copyIp()}
                  className="rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-xs font-medium text-slate-200 transition hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  {copied ? "Copied" : "Copy"}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    void refreshLive();
                    void refreshSlow(true);
                  }}
                  className="rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-xs font-medium text-slate-200 transition hover:bg-white/10"
                >
                  Refresh
                </button>
              </div>
              <p className="mt-2 text-xs text-slate-500">
                {tailscale?.detail ?? "Checking Tailscale…"}
              </p>
              <div className="mt-3 rounded-lg border border-white/10 bg-black/40 px-3 py-2 text-xs text-slate-400">
                <p className="font-medium text-slate-300">
                  Virtual gamepad (built into LANPlay)
                </p>
                <p className="mt-1">
                  {vigemBundle?.driverReady
                    ? "Ready — remote pads inject as Xbox 360."
                    : vigemBundle?.detail ??
                      "Checking bundled ViGEm support…"}
                </p>
                <p className="mt-1 text-slate-500">
                  Client lib:{" "}
                  {vigemBundle?.clientDllFound
                    ? "built into app ✓"
                    : "n/a"}{" "}
                  · Driver setup:{" "}
                  {vigemBundle?.driverSetupFound ? "bundled ✓" : "missing"}
                </p>
                {!vigemBundle?.driverReady &&
                  vigemBundle?.driverSetupFound && (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void onInstallVigem()}
                      className="mt-2 rounded-lg bg-amber-500 px-3 py-1.5 text-xs font-semibold text-slate-950 hover:bg-amber-400 disabled:opacity-50"
                    >
                      Install gamepad support (one-time)
                    </button>
                  )}
                <p className="mt-1 text-[10px] text-slate-600">
                  Windows requires a one-time driver install (UAC). You do not
                  download anything from GitHub — it ships with LANPlay.
                </p>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3 text-sm">
              <label className="space-y-1">
                <span className="text-xs text-slate-500">Control port</span>
                <input
                  type="number"
                  value={controlPort}
                  readOnly
                  className="w-full rounded-lg border border-white/10 bg-black/40 px-3 py-2 text-slate-300"
                />
              </label>
              <label className="space-y-1">
                <span className="text-xs text-slate-500">
                  Input UDP port (Phase 2)
                </span>
                <input
                  type="number"
                  value={mediaPort}
                  readOnly
                  className="w-full rounded-lg border border-white/10 bg-black/40 px-3 py-2 text-slate-300"
                />
              </label>
            </div>

            <label className="flex cursor-pointer items-center gap-3 rounded-xl border border-white/10 bg-black/20 px-4 py-3 text-sm">
              <input
                type="checkbox"
                checked={host?.allowRemoteInput ?? true}
                onChange={(e) => void onToggleInput(e.target.checked)}
                className="size-4 accent-cyan-500"
              />
              <span>
                Allow remote input
                <span className="block text-xs text-slate-500">
                  Off = ignore controller packets (no ViGEm updates)
                </span>
              </span>
            </label>

            {host?.pendingJoin && (
              <div className="rounded-xl border border-amber-400/40 bg-amber-500/10 p-4">
                <p className="text-sm font-semibold text-amber-100">
                  Join request
                </p>
                <p className="mt-1 text-sm text-amber-50/90">
                  <span className="font-mono text-cyan-200">
                    {host.pendingJoin.clientName}
                  </span>{" "}
                  (
                  <span className="font-mono">{host.pendingJoin.peerIp}</span>)
                  wants to connect.
                </p>
                <div className="mt-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void onRespondJoin(true)}
                    className="rounded-lg bg-emerald-500 px-4 py-2 text-sm font-semibold text-slate-950 hover:bg-emerald-400 disabled:opacity-50"
                  >
                    Accept
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void onRespondJoin(false)}
                    className="rounded-lg bg-rose-500/90 px-4 py-2 text-sm font-semibold text-white hover:bg-rose-500 disabled:opacity-50"
                  >
                    Reject
                  </button>
                </div>
              </div>
            )}

            <p className="text-sm text-slate-400">{host?.message}</p>
            {host?.sessionActive && (
              <p className="text-xs text-emerald-400/90">
                Session active with accepted client.
              </p>
            )}

            <div className="rounded-xl border border-white/10 bg-black/30 p-4 text-sm">
              <p className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                Capture + encode (Phase 5)
              </p>
              <p className="mt-1 text-slate-300">
                {desktopCapture?.active ? (
                  <>
                    <span className="font-mono text-cyan-300">
                      {desktopCapture.width}×{desktopCapture.height}
                    </span>
                    {" → "}
                    <span className="font-mono text-cyan-200">
                      {desktopCapture.encodeWidth}×{desktopCapture.encodeHeight}
                    </span>{" "}
                    · cap{" "}
                    <span className="font-mono text-emerald-300">
                      {desktopCapture.fps.toFixed(1)}
                    </span>
                    {" / enc "}
                    <span className="font-mono text-emerald-300">
                      {desktopCapture.encodeFps.toFixed(1)} FPS
                    </span>
                  </>
                ) : (
                  <span className="text-slate-500">Not running</span>
                )}
              </p>
              {desktopCapture?.active && (
                <p className="mt-1 font-mono text-xs text-slate-400">
                  capture {desktopCapture.lastCaptureMs.toFixed(2)} ms · encode{" "}
                  {desktopCapture.lastEncodeMs.toFixed(2)} ms ·{" "}
                  {desktopCapture.bitrateKbps} kbps · frames{" "}
                  {desktopCapture.frames}/{desktopCapture.encodedFrames}
                </p>
              )}
              <p className="mt-1 text-xs text-slate-500">
                Encoder: {desktopCapture?.encoder ?? "—"}
              </p>
              <p className="mt-1 text-xs text-slate-500">
                {desktopCapture?.detail ?? "Start Host to begin capture+encode."}
              </p>
              <p className="mt-1 text-[11px] text-slate-600">
                H.264 encoded in memory — not streamed yet (Phase 6). Tune res /
                FPS / bitrate / encoder in the{" "}
                <button
                  type="button"
                  className="text-cyan-400/90 underline-offset-2 hover:underline"
                  onClick={() => setMode("settings")}
                >
                  Settings
                </button>{" "}
                tab.
              </p>
            </div>

            <div className="flex flex-wrap gap-3">
              {!hostListening ? (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void onStartHost()}
                  className="rounded-xl bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400 disabled:opacity-50"
                >
                  Start Host
                </button>
              ) : (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void onStopHost()}
                  className="rounded-xl bg-rose-500/90 px-5 py-2.5 text-sm font-semibold text-white transition hover:bg-rose-500 disabled:opacity-50"
                >
                  Stop Host
                </button>
              )}
            </div>
          </section>
        ) : (
          <section className="space-y-4 rounded-2xl border border-white/10 bg-white/[0.03] p-6 shadow-xl shadow-black/30">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-lg font-semibold text-white">Client mode</h2>
              {client && (
                <span
                  className={`rounded-full px-3 py-1 text-xs font-medium ring-1 ${stateBadgeClass(client.state)}`}
                >
                  {client.state}
                </span>
              )}
            </div>

            <label className="block space-y-1">
              <span className="text-xs uppercase tracking-wide text-slate-500">
                Host Tailscale IP
              </span>
              <input
                type="text"
                value={hostIp}
                onChange={(e) => setHostIp(e.target.value)}
                placeholder="100.x.y.z"
                disabled={clientActive}
                className="w-full rounded-xl border border-white/10 bg-black/40 px-4 py-3 font-mono text-lg text-cyan-100 outline-none ring-cyan-500/40 placeholder:text-slate-600 focus:ring-2 disabled:opacity-60"
              />
            </label>

            {recentIps.length > 0 && !clientActive && (
              <div className="flex flex-wrap gap-2">
                <span className="w-full text-xs text-slate-500">Recent</span>
                {recentIps.map((ip) => (
                  <button
                    key={ip}
                    type="button"
                    onClick={() => setHostIp(ip)}
                    className="rounded-lg border border-white/10 bg-black/30 px-3 py-1 font-mono text-xs text-slate-300 hover:border-cyan-500/40 hover:text-cyan-200"
                  >
                    {ip}
                  </button>
                ))}
              </div>
            )}

            <div className="grid grid-cols-2 gap-3 text-sm">
              <label className="space-y-1">
                <span className="text-xs text-slate-500">Control port</span>
                <input
                  type="number"
                  value={controlPort}
                  onChange={(e) =>
                    setControlPort(Number(e.target.value) || 47800)
                  }
                  disabled={clientActive}
                  className="w-full rounded-lg border border-white/10 bg-black/40 px-3 py-2 text-slate-200 disabled:opacity-60"
                />
              </label>
              <label className="space-y-1">
                <span className="text-xs text-slate-500">Input UDP port</span>
                <input
                  type="number"
                  value={mediaPort}
                  onChange={(e) =>
                    setMediaPort(Number(e.target.value) || 47801)
                  }
                  disabled={clientActive}
                  className="w-full rounded-lg border border-white/10 bg-black/40 px-3 py-2 text-slate-200 disabled:opacity-60"
                />
              </label>
            </div>

            <p className="text-sm text-slate-400">{client?.message}</p>
            <p className="text-xs text-slate-500">
              Local XInput:{" "}
              {client?.localPadConnected ? "pad detected" : "no pad on index 0"}
            </p>

            {client?.state === "waiting_approval" && (
              <p className="rounded-lg border border-amber-400/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-100">
                Waiting for the host to Accept or Reject your join request…
              </p>
            )}

            {/* Phase 6: remote desktop preview */}
            {clientActive && (
              <div className="overflow-hidden rounded-xl border border-white/10 bg-black/50">
                <div className="flex items-center justify-between border-b border-white/10 px-3 py-2">
                  <p className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                    Remote video (thumb) · main view = Stream window
                  </p>
                  <p className="font-mono text-[11px] text-slate-400">
                    {clientVideo?.active
                      ? `${clientVideo.width}×${clientVideo.height} · ${clientVideo.fps.toFixed(1)} FPS · ${clientVideo.frames}f`
                      : "idle"}
                  </p>
                </div>
                <div className="relative flex min-h-[200px] items-center justify-center bg-black">
                  {clientVideo?.jpegBase64 ? (
                    <img
                      src={`data:image/jpeg;base64,${clientVideo.jpegBase64}`}
                      alt="Host stream"
                      className="max-h-[360px] w-full object-contain"
                    />
                  ) : (
                    <p className="px-4 py-12 text-center text-sm text-slate-500">
                      {clientVideo?.detail ??
                        "After Accept a Stream window opens — click it to control (Moonlight-style)."}
                    </p>
                  )}
                </div>
                <p className="border-t border-white/5 px-3 py-2 text-[11px] text-slate-600">
                  {clientVideo?.detail ?? "—"} · pkts{" "}
                  {clientVideo?.packets ?? 0} · hellos{" "}
                  {clientVideo?.hellosSent ?? 0} · host video :{mediaPort + 1}
                </p>
              </div>
            )}

            {client?.state === "streaming" && (
              <div
                className={`rounded-xl border p-4 ${
                  capture?.active
                    ? "border-emerald-400/40 bg-emerald-500/10"
                    : "border-white/15 bg-black/30"
                }`}
              >
                <p className="text-sm font-semibold text-white">
                  Input capture{" "}
                  {capture?.active ? (
                    <span className="text-emerald-300">ON</span>
                  ) : (
                    <span className="text-slate-400">OFF</span>
                  )}
                </p>
                <p className="mt-1 text-xs text-slate-400">
                  {capture?.hint ??
                    "Moonlight-style: capture sends mouse/keyboard to host."}
                </p>
                <p className="mt-1 text-[11px] text-slate-500">
                  Hotkey: Ctrl+Shift+Alt+Z = release capture · Click Capture to
                  control host again
                </p>
                <div className="mt-3 flex flex-wrap gap-2">
                  {capture?.active ? (
                    <button
                      type="button"
                      onClick={() => void onSetCapture(false)}
                      className="rounded-lg bg-amber-500 px-4 py-2 text-sm font-semibold text-slate-950 hover:bg-amber-400"
                    >
                      Release capture
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => void onSetCapture(true)}
                      className="rounded-lg bg-cyan-500 px-4 py-2 text-sm font-semibold text-slate-950 hover:bg-cyan-400"
                    >
                      Capture input
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => void onToggleCapture()}
                    className="rounded-lg border border-white/15 bg-white/5 px-4 py-2 text-sm text-slate-200 hover:bg-white/10"
                  >
                    Toggle
                  </button>
                </div>
              </div>
            )}

            <div className="flex flex-wrap gap-3">
              {!clientActive ? (
                <button
                  type="button"
                  disabled={busy || !hostIp.trim()}
                  onClick={() => void onConnect()}
                  className="rounded-xl bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400 disabled:opacity-50"
                >
                  Request to join
                </button>
              ) : (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void onDisconnect()}
                  className="rounded-xl bg-rose-500/90 px-5 py-2.5 text-sm font-semibold text-white transition hover:bg-rose-500 disabled:opacity-50"
                >
                  Disconnect
                </button>
              )}
            </div>
          </section>
        )}

        <footer className="mt-auto space-y-1 text-xs text-slate-600">
          <p>
            Phase 6: video via HELLO punch. Start Host may ask once for
            Firewall / UAC — allow LANPlay so clients can reach you.
          </p>
          <p>
            Encode plan:{" "}
            <span className="font-mono text-slate-500">
              {videoSettings.resolutionMode === "auto"
                ? `auto≤${videoSettings.maxEdge}`
                : `${videoSettings.width}×${videoSettings.height}`}{" "}
              @ {videoSettings.fps}fps / {videoSettings.bitrateKbps}kbps /{" "}
              {videoSettings.encoder}
            </span>
          </p>
        </footer>
      </div>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-white/5 bg-black/20 px-3 py-2">
      <p className="text-[10px] uppercase tracking-wide text-slate-500">
        {label}
      </p>
      <p className="mt-0.5 truncate font-mono text-sm text-slate-200">{value}</p>
    </div>
  );
}
