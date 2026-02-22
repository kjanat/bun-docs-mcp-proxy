import type { Toolkit } from "actions-toolkit";
import * as os from "os";
import * as path from "path";
import { getPayloadInputs, normalizeTag, parseBool, toInputString } from "./guards";
import { parsePlatforms } from "./platforms";
import type { ReleaseConfig, ReleaseInputs, ReleaseOutputs } from "./types";

const resolveVersion = (payloadTag: unknown, inputTag: unknown, contextRef: string): string | undefined => {
  const payload = normalizeTag(toInputString(payloadTag));
  if (payload) return payload;

  const input = normalizeTag(toInputString(inputTag));
  if (input) return input;

  const refTag = normalizeTag(contextRef);
  if (refTag) return refTag;

  return undefined;
};

export const resolveConfig = (
  tools: Toolkit<ReleaseInputs, ReleaseOutputs>,
): ReleaseConfig | null => {
  const payloadInputs = getPayloadInputs(tools.context.payload);
  const version = resolveVersion(payloadInputs.tag, tools.inputs.tag, tools.context.ref);

  if (!version) {
    tools.exit.failure("No tag found for release.");
    return null;
  }

  const draft = parseBool(payloadInputs.draft ?? tools.inputs.draft);
  const prerelease = parseBool(payloadInputs.prerelease ?? tools.inputs.prerelease);

  const { owner, repo } = tools.context.repo;
  const repoFull = `${owner}/${repo}`;

  const runId = Number(process.env.GITHUB_RUN_ID ?? "");
  if (!Number.isFinite(runId)) {
    tools.exit.failure("Missing/invalid GITHUB_RUN_ID (cannot list run artifacts).");
    return null;
  }

  const workspace = tools.workspace || process.cwd();
  const artifactsDir = path.join(workspace, "artifacts");
  const tmpBase = process.env.RUNNER_TEMP || os.tmpdir();
  const tmpDir = path.join(tmpBase, `release-${Date.now()}`);

  const platforms = parsePlatforms(toInputString(tools.inputs.matrix_json), (msg) => tools.log.warn(msg));

  return {
    version,
    draft,
    prerelease,
    owner,
    repo,
    repoFull,
    runId,
    artifactsDir,
    tmpDir,
    platforms,
  };
};
