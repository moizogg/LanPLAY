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
}

export interface ClientStatus {
  state: SessionState;
  hostIp: string | null;
  controlPort: number;
  mediaPort: number;
  message: string;
}
