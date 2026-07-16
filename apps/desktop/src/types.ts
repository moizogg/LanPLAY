export type AppMode = "host" | "client" | "settings";

export type SessionState =
  | "idle"
  | "listening"
  | "waiting_approval"
  | "connecting"
  | "streaming"
  | "error";

export interface AppInfo {
  name: string;
  version: string;
  protocolVersion: number;
  phase: number;
}

export interface TailscaleInfo {
  ip: string | null;
  available: boolean;
  detail: string;
}

export interface PendingJoinInfo {
  peerIp: string;
  clientName: string;
}

export interface HostStatus {
  state: SessionState;
  controlPort: number;
  mediaPort: number;
  allowRemoteInput: boolean;
  message: string;
  vigemOk: boolean;
  packetsReceived: number;
  inputLatencyMs: number;
  lastSeq: number;
  virtualPadActive: boolean;
  pendingJoin: PendingJoinInfo | null;
  sessionActive: boolean;
}

export interface ClientStatus {
  state: SessionState;
  hostIp: string | null;
  controlPort: number;
  mediaPort: number;
  message: string;
  localPadConnected: boolean;
  packetsSent: number;
  lastSeq: number;
}

export interface ControllerStats {
  role: string;
  packets: number;
  lastSeq: number;
  inputLatencyMs: number;
  padConnected: boolean;
  vigemOk: boolean;
  detail: string;
}

export interface VigemBundleStatus {
  clientDllFound: boolean;
  clientDllPath: string | null;
  driverSetupFound: boolean;
  driverSetupPath: string | null;
  driverReady: boolean;
  detail: string;
}

/** Moonlight-style client input capture */
export interface CaptureStatus {
  active: boolean;
  hint: string;
}

/** Phase 4–5 host desktop capture + encode stats */
export interface CaptureSnapshot {
  active: boolean;
  frames: number;
  encodedFrames: number;
  width: number;
  height: number;
  encodeWidth: number;
  encodeHeight: number;
  fps: number;
  encodeFps: number;
  lastCaptureMs: number;
  lastEncodeMs: number;
  bitrateKbps: number;
  encoder: string;
  detail: string;
}

/** Host video / encode settings (Settings tab, Sunshine-style) */
export type ResolutionMode = "auto" | "fixed";

export interface VideoSettings {
  outputIndex: number;
  fps: number;
  bitrateKbps: number;
  resolutionMode: ResolutionMode;
  maxEdge: number;
  width: number;
  height: number;
  encoder: string;
}

export interface EncoderOption {
  id: string;
  name: string;
  available: boolean;
  hardware: boolean;
  detail: string;
}

export interface ResolutionPreset {
  id: string;
  label: string;
  mode: ResolutionMode;
  width: number;
  height: number;
  maxEdge: number;
}
