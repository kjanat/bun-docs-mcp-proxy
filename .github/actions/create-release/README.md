# Create release

Composite action used by the release workflow to download its build artifacts,
generate `SHA256SUMS`, and upload the archives and checksums to a GitHub
release. It creates the release as a draft and publishes it after every asset
has been uploaded, unless the caller requests a draft.

The action runs `src/index.ts` directly with Bun. Dependencies and the lockfile
are managed by the Bun workspace at the repository root; no build step is
required.

## Inputs and outputs

| Input          | Purpose                                                      |
| -------------- | ------------------------------------------------------------ |
| `tag`          | Release tag; defaults to the current GitHub ref.             |
| `draft`        | Leave the completed release as a draft; defaults to `false`. |
| `prerelease`   | Mark the release as a prerelease; defaults to `false`.       |
| `github_token` | Token with `contents: write`; defaults to `github.token`.    |
| `matrix_json`  | Optional platform definitions used in the release notes.     |

Outputs are `release_id` and `release_url`.

## Local checks

Run from the repository root:

```bash
bun install --frozen-lockfile
bun run typecheck
```

Executing `src/index.ts` requires the GitHub Actions context and can create and
publish a release. Use the typecheck command for local validation.
