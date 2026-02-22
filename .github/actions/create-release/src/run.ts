import type { Toolkit } from "actions-toolkit";
import * as fs from "fs/promises";
import * as path from "path";
import { collectArtifacts } from "./artifacts";
import { resolveConfig } from "./config";
import { buildReleaseNotes } from "./release-notes";
import type { GitHubClient, ReleaseInputs, ReleaseOutputs } from "./types";

export const run = async (tools: Toolkit<ReleaseInputs, ReleaseOutputs>): Promise<void> => {
  const config = resolveConfig(tools);
  if (!config) return;

  const github = tools.github as GitHubClient;

  const { artifactFiles, checksumsPath, checksums } = await collectArtifacts({
    github,
    exec: tools.exec,
    owner: config.owner,
    repo: config.repo,
    runId: config.runId,
    artifactsDir: config.artifactsDir,
    tmpDir: config.tmpDir,
  });

  if (artifactFiles.length === 0) {
    tools.exit.failure("No release artifacts found for this workflow run.");
    return;
  }

  const notes = buildReleaseNotes({
    version: config.version,
    repoFull: config.repoFull,
    checksums,
    platforms: config.platforms,
  });

  const release = await github.repos.createRelease({
    owner: config.owner,
    repo: config.repo,
    tag_name: config.version,
    name: `Release ${config.version}`,
    body: notes,
    draft: config.draft,
    prerelease: config.prerelease,
  });

  const assets = [...artifactFiles, checksumsPath];
  for (const filePath of assets) {
    const name = path.basename(filePath);
    const data = await fs.readFile(filePath);

    const contentType = name.endsWith(".zip")
      ? "application/zip"
      : name.endsWith(".tar.gz")
      ? "application/gzip"
      : "text/plain";

    await github.repos.uploadReleaseAsset({
      owner: config.owner,
      repo: config.repo,
      release_id: release.data.id,
      name,
      data,
      headers: {
        "content-type": contentType,
        "content-length": data.length,
      },
    });
  }

  tools.outputs.release_id = String(release.data.id);
  tools.outputs.release_url = String(release.data.html_url);

  tools.log.success(`Created release ${config.version} and uploaded ${assets.length} assets.`);
};
