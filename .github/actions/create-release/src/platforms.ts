import { isRecord } from "./guards";
import type { PlatformDefinition } from "./types";

const isPlatformDefinition = (value: unknown): value is PlatformDefinition => {
  if (!isRecord(value)) return false;
  return typeof value.name === "string" && typeof value.archive_ext === "string";
};

export const parsePlatforms = (input: string | undefined): PlatformDefinition[] => {
  if (!input) return [];

  try {
    const parsed = JSON.parse(input);
    const list = Array.isArray(parsed)
      ? parsed
      : isRecord(parsed) && Array.isArray(parsed.platform)
      ? parsed.platform
      : isRecord(parsed) && Array.isArray(parsed.platforms)
      ? parsed.platforms
      : [];

    return list.filter(isPlatformDefinition);
  } catch {
    return [];
  }
};

export const renderPlatformLines = (platforms: PlatformDefinition[]): string[] => {
  return platforms.map((platform) => {
    const label = platform.label?.trim() || platform.name;
    const archive = `bun-docs-mcp-proxy-${platform.name}.${platform.archive_ext}`;
    return `- **${label}**: \`${archive}\``;
  });
};
