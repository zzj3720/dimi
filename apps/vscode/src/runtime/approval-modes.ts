import type { JsonObject, PermissionMode } from "@dimi-agent/dimi-sdk";

const APPROVAL_MODES_METADATA_KEY = "vscode_approval_modes";

export interface ApprovalModes {
  readonly yolo: boolean;
  readonly afk: boolean;
}

export function readApprovalModes(
  metadata: Readonly<Record<string, unknown>> | undefined,
): ApprovalModes | undefined {
  const value = metadata?.[APPROVAL_MODES_METADATA_KEY];
  if (typeof value !== "object" || value === null || Array.isArray(value)) return undefined;
  const yolo = Reflect.get(value, "yolo");
  const afk = Reflect.get(value, "afk");
  if (typeof yolo !== "boolean" && typeof afk !== "boolean") return undefined;
  return {
    yolo: typeof yolo === "boolean" ? yolo : false,
    afk: typeof afk === "boolean" ? afk : false,
  };
}

export function approvalModesMetadata(modes: ApprovalModes): JsonObject {
  return {
    [APPROVAL_MODES_METADATA_KEY]: {
      yolo: modes.yolo,
      afk: modes.afk,
    },
  };
}

export function permissionForApprovalModes(modes: ApprovalModes): PermissionMode {
  if (modes.afk) return "auto";
  return modes.yolo ? "yolo" : "manual";
}

/** Global yolo config is authoritative when a session attaches; afk is per-session. */
export function withGlobalYoloMode(modes: ApprovalModes, yoloMode: boolean): ApprovalModes {
  return modes.yolo === yoloMode ? modes : { yolo: yoloMode, afk: modes.afk };
}
