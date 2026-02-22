import * as fs from "fs/promises";
import * as path from "path";
import { ensureDir, hashFile, normalizeArtifactDisplayPath, removePath, toBuffer, walk } from "./fs-utils";
import { isArtifact } from "./guards";
import type { GitHubClient } from "./types";

type ExecFn = (commandLine: string, args?: string[]) => Promise<number>;

type CollectArtifactsParams = {
  github: GitHubClient;
  exec: ExecFn;
  owner: string;
  repo: string;
  runId: number;
  artifactsDir: string;
  tmpDir: string;
};

type CollectArtifactsResult = {
  artifactFiles: string[];
  checksumsPath: string;
  checksums: string;
};

export const collectArtifacts = async ({
  github,
  exec,
  owner,
  repo,
  runId,
  artifactsDir,
  tmpDir,
}: CollectArtifactsParams): Promise<CollectArtifactsResult> => {
  await removePath(artifactsDir);
  await ensureDir(artifactsDir);
  await ensureDir(tmpDir);

  const response = await github.paginate(
    "GET /repos/{owner}/{repo}/actions/runs/{run_id}/artifacts",
    { owner, repo, run_id: runId, per_page: 100 },
  );

  for (const item of response) {
    if (!isArtifact(item) || item.expired) continue;
    const artifact = item;

    const zipPath = path.join(tmpDir, `${artifact.name}.zip`);
    const destDir = path.join(artifactsDir, artifact.name);

    const response = await github.actions.downloadArtifact({
      owner,
      repo,
      artifact_id: artifact.id,
      archive_format: "zip",
    });

    await fs.writeFile(zipPath, toBuffer(response.data));
    await ensureDir(destDir);

    await exec("unzip", ["-q", zipPath, "-d", destDir]);
  }

  const artifactFiles = (await walk(artifactsDir))
    .filter((filePath) => filePath.endsWith(".tar.gz") || filePath.endsWith(".zip"))
    .sort();

  const checksumLines: string[] = [];
  for (const filePath of artifactFiles) {
    const display = normalizeArtifactDisplayPath(artifactsDir, filePath);
    checksumLines.push(`${await hashFile(filePath)}  ${display}`);
  }

  const checksumsPath = path.join(artifactsDir, "SHA256SUMS");
  await fs.writeFile(checksumsPath, `${checksumLines.join("\n")}\n`, "utf8");
  const checksums = (await fs.readFile(checksumsPath, "utf8")).trimEnd();

  return { artifactFiles, checksumsPath, checksums };
};
