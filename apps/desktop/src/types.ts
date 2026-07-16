export type AppMode = "host" | "client";

export type SessionState =
  | "idle"
  | "listening"
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
