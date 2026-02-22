import type { Toolkit } from "actions-toolkit";
import type { InputType } from "actions-toolkit/lib/inputs";
import type { OutputType } from "actions-toolkit/lib/outputs";

export type JsonRecord = Record<string, unknown>;

/** Octokit client type extracted from actions-toolkit's Toolkit class. */
export type GitHubClient = Toolkit["github"];

export type ReleaseInputs = InputType & {
  tag?: string;
  draft?: string;
  prerelease?: string;
  github_token?: string;
  matrix_json?: string;
};

export type ReleaseOutputs = OutputType & {
  release_id?: string;
  release_url?: string;
};

export type Artifact = {
  id: number;
  name: string;
  expired: boolean;
};

export type ReleaseNotesParams = {
  version: string;
  repoFull: string;
  checksums: string;
  platforms: PlatformDefinition[];
};

export type PlatformDefinition = {
  name: string;
  archive_ext: string;
  label?: string;
};

export type ReleaseConfig = {
  version: string;
  draft: boolean;
  prerelease: boolean;
  owner: string;
  repo: string;
  repoFull: string;
  runId: number;
  artifactsDir: string;
  tmpDir: string;
  platforms: PlatformDefinition[];
};
