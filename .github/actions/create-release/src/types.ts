import type { InputType } from "actions-toolkit/lib/inputs";
import type { OutputType } from "actions-toolkit/lib/outputs";

export type JsonRecord = Record<string, unknown>;

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

export type GitHubClient = {
  paginate: <T>(route: unknown, params: unknown) => Promise<T[]>;
  actions: {
    listWorkflowRunArtifacts: unknown;
    downloadArtifact: (params: {
      owner: string;
      repo: string;
      artifact_id: number;
      archive_format: "zip";
    }) => Promise<{ data: unknown }>;
  };
  repos: {
    createRelease: (params: {
      owner: string;
      repo: string;
      tag_name: string;
      name: string;
      body: string;
      draft: boolean;
      prerelease: boolean;
    }) => Promise<{ data: { id: number; html_url: string } }>;
    uploadReleaseAsset: (params: {
      owner: string;
      repo: string;
      release_id: number;
      name: string;
      data: Buffer;
      headers: { "content-type": string; "content-length": number };
    }) => Promise<unknown>;
  };
};
