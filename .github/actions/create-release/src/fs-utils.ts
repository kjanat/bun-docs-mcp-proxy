import { mkdirP, rmRF } from "@actions/io";
import * as crypto from "crypto";
import * as fs from "fs/promises";
import * as path from "path";

export const removePath = async (targetPath: string): Promise<void> => {
  await rmRF(targetPath);
};

export const ensureDir = async (targetPath: string): Promise<void> => {
  await mkdirP(targetPath);
};

export const hashFile = async (filePath: string): Promise<string> => {
  const buffer = await fs.readFile(filePath);
  return crypto.createHash("sha256").update(buffer).digest("hex");
};

export const walk = async (dir: string): Promise<string[]> => {
  const entries = await fs.readdir(dir, { withFileTypes: true });
  const files: string[] = [];

  for (const entry of entries) {
    if (entry.name === ".tmp") continue;
    const fullPath = path.join(dir, entry.name);

    if (entry.isDirectory()) {
      files.push(...(await walk(fullPath)));
    } else {
      files.push(fullPath);
    }
  }

  return files;
};

export const toBuffer = (data: unknown): Buffer => {
  if (Buffer.isBuffer(data)) return data;
  if (data instanceof ArrayBuffer) return Buffer.from(data);
  if (ArrayBuffer.isView(data)) {
    return Buffer.from(data.buffer, data.byteOffset, data.byteLength);
  }
  if (typeof data === "string") return Buffer.from(data);

  throw new TypeError("Unsupported artifact response type for buffer conversion.");
};

export const normalizeArtifactDisplayPath = (artifactsDir: string, filePath: string): string => {
  const rel = path.relative(artifactsDir, filePath);
  const parts = rel.split(path.sep);
  return parts.length > 1 ? parts.slice(1).join("/") : rel.split(path.sep).join("/");
};
