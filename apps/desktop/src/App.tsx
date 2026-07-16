import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  AppMode,
  ClientStatus,
  HostStatus,
  TailscaleInfo,
} from "./types";

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
  const [hostIp, setHostIp] = useState("");
  const [controlPort, setControlPort] = useState(47800);
  const [mediaPort, setMediaPort] = useState(47801);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [recentIps, setRecentIps] = useState<string[]>(() => loadRecentIps());

  const refresh = useCallback(async () => {
    try {
      const [info, ts, h, c] = await Promise.all([
        invoke<AppInfo>("get_app_info"),
        invoke<TailscaleInfo>("get_tailscale_info"),
        invoke<HostStatus>("get_host_status"),
        invoke<ClientStatus>("get_client_status"),
      ]);
      setAppInfo(info);
      setTailscale(ts);
      setHost(h);
      setClient(c);
      setControlPort(h.controlPort);
      setMediaPort(h.mediaPort);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

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

  const hostListening = host?.state === "listening" || host?.state === "streaming";
  const clientActive =
    client?.state === "connecting" || client?.state === "streaming";

  return (
    <div className="min-h-full bg-[radial-gradient(ellipse_at_top,_#122033_0%,_#070b12_55%)]">
      <div className="mx-auto flex min-h-full max-w-3xl flex-col gap-6 px-6 py-8">
        <header className="flex items-start justify-between gap-4">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.2em] text-cyan-400/80">
              Phase {appInfo?.phase ?? 1}
            </p>
            <h1 className="mt-1 text-3xl font-bold tracking-tight text-white">
              LANPlay
            </h1>
            <p className="mt-1 max-w-md text-sm text-slate-400">
              Low-latency desktop stream over Tailscale. Connect with a host IP
              — no room codes yet.
            </p>
          </div>
          <div className="rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-right text-xs text-slate-400">
            <div>v{appInfo?.version ?? "…"}</div>
            <div>protocol {appInfo?.protocolVersion ?? "…"}</div>
          </div>
        </header>

        <div className="inline-flex w-fit rounded-xl border border-white/10 bg-black/30 p-1">
          {(["host", "client"] as const).map((m) => (
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

        {mode === "host" ? (
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
                  onClick={() => void refresh()}
                  className="rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-xs font-medium text-slate-200 transition hover:bg-white/10"
                >
                  Refresh
                </button>
              </div>
              <p className="mt-2 text-xs text-slate-500">
                {tailscale?.detail ?? "Checking Tailscale…"}
              </p>
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
                <span className="text-xs text-slate-500">Media port</span>
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
                  Off = view-only when streaming (Phase 8+)
                </span>
              </span>
            </label>

            <p className="text-sm text-slate-400">{host?.message}</p>

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
                  onChange={(e) => setControlPort(Number(e.target.value) || 47800)}
                  disabled={clientActive}
                  className="w-full rounded-lg border border-white/10 bg-black/40 px-3 py-2 text-slate-200 disabled:opacity-60"
                />
              </label>
              <label className="space-y-1">
                <span className="text-xs text-slate-500">Media port</span>
                <input
                  type="number"
                  value={mediaPort}
                  onChange={(e) => setMediaPort(Number(e.target.value) || 47801)}
                  disabled={clientActive}
                  className="w-full rounded-lg border border-white/10 bg-black/40 px-3 py-2 text-slate-200 disabled:opacity-60"
                />
              </label>
            </div>

            <p className="text-sm text-slate-400">{client?.message}</p>

            <div className="flex flex-wrap gap-3">
              {!clientActive ? (
                <button
                  type="button"
                  disabled={busy || !hostIp.trim()}
                  onClick={() => void onConnect()}
                  className="rounded-xl bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400 disabled:opacity-50"
                >
                  Connect
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
            Phase 1 shell only — no real video/audio/controller streaming yet.
          </p>
          <p>
            Next: Phase 2 controllers (Xbox 360 / ViGEm), then real IP sockets.
          </p>
        </footer>
      </div>
    </div>
  );
}
