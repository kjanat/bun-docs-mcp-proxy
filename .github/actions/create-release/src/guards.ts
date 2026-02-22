import type { Artifact, JsonRecord } from "./types";

export const isRecord = (value: unknown): value is JsonRecord => typeof value === "object" && value !== null;

export const getPayloadInputs = (payload: unknown): JsonRecord => {
  if (!isRecord(payload)) return {};
  const inputs = payload.inputs;
  return isRecord(inputs) ? inputs : {};
};

export const toInputString = (value: unknown): string | undefined => {
  if (typeof value === "string") {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : undefined;
  }
  if (typeof value === "number" || typeof value === "boolean" || typeof value === "bigint") {
    const coerced = String(value).trim();
    return coerced.length > 0 ? coerced : undefined;
  }
  return undefined;
};

export const parseBool = (value: unknown): boolean => {
  if (typeof value === "boolean") return value;
  if (typeof value === "string") return value.trim().toLowerCase() === "true";
  if (typeof value === "number") return value === 1;
  return false;
};

export const normalizeTag = (value: string | undefined): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  if (trimmed.startsWith("refs/tags/")) return trimmed.slice("refs/tags/".length);
  if (trimmed.startsWith("refs/")) return undefined;
  return trimmed;
};

export const isArtifact = (value: unknown): value is Artifact => {
  if (!isRecord(value)) return false;
  return typeof value.id === "number" && typeof value.name === "string" && typeof value.expired === "boolean";
};
