---
paths:
  - ".github/workflows/release.yml"
  - ".github/workflows/scorecard.yml"
  - ".github/zizmor.yml"
---

# Release workflow security boundary

`.github/workflows/release.yml` is split into prep jobs (no publish credentials, untrusted-code-may-run) and publish jobs (credentials present, no untrusted code). This split exists to close the TanStack-class supply-chain pattern: when `id-token: write` + `NODE_AUTH_TOKEN` / `VSCE_PAT` / `OVSX_PAT` / `CARGO_REGISTRY_TOKEN` sit in the same job as `npm ci`, `pnpm install`, `npm install -g`, or `cargo` verify-build, a single compromised dependency or transitive `build.rs` can exfiltrate the token. The split forces a clean handoff via uploaded artifacts.

## Job layout (do not collapse)

```
build               ── matrix-built binaries, ED25519 signing key only
publish-crates      ── cargo publish --no-verify, CARGO_REGISTRY_TOKEN
release             ── needs: build; persist-credentials: false; tag push via URL-encoded GH_TOKEN
npm-prep            ── needs: build; npm ci + npm pack + manifest.tsv; NO tokens
npm-publish         ── needs: [npm-prep, release]; downloads tarballs; id-token: write + NODE_AUTH_TOKEN
vscode-prep         ── needs: build; pnpm install + pnpm build + pnpm package; NO tokens
vscode-publish      ── needs: [vscode-prep, release]; downloads .vsix; VSCE_PAT + OVSX_PAT
```

If a future PR wants to "simplify" by re-merging a `prep` and `publish` pair, reject it: the boundary is load-bearing, not stylistic.

## Hard rules

- **No `npm ci` / `npm install` / `pnpm install` in any job that holds a publish token.** Move install work into the matching `*-prep` job that has only `contents: read`.
- **Every `npm publish` carries `--ignore-scripts`.** Tarball lifecycle scripts (prepack/prepare/postpack/prepublishOnly) never execute on the privileged runner. Lifecycle work that must run pre-pack happens in `npm-prep` (e.g., `npm run publish:prepare`, `npm run create-npm-dirs`).
- **`npm install -g npm@<X.Y.Z>` is pinned, never `npm@latest`.** A compromised `latest` tag would land in the privileged job without a commit to `release.yml`. Same rule for `@vscode/vsce@<X.Y.Z>` and `ovsx@<X.Y.Z>` versions in `vscode-publish` (mirror `editors/vscode/pnpm-lock.yaml`).
- **`npm-publish`'s `Install pinned npm` step uses a two-step bootstrap: `npm install -g --ignore-scripts npm@10.9.8` then `npm install -g --ignore-scripts npm@<pinned-target>`.** Node 22 GitHub runners ship a bundled npm with a broken dependency tree; installing `npm@11.x` (or any 11.x-pinned target) directly fails with `Cannot find module 'promise-retry'`. The `npm@10.9.8` step repairs the tree before the pinned target lands. This is independent of `npm@latest` pinning rule above; pinning the second step does NOT subsume the two-step requirement. Refactoring tip: if a future hardening PR proposes collapsing to one step "now that we pin", reject. Incident v2.71.0 -> v2.71.1: the prep/publish split PR collapsed it and the entire `Publish to npm` job failed before any tarball reached the registry.
- **`cargo publish` uses `--no-verify`.** The verify-build runs `build.rs` of every transitive dependency with `CARGO_REGISTRY_TOKEN` in env; `--no-verify` skips that compile entirely. The tag's commit was already CI'd cleanly on `main`; the verify-build was a redundant safety net.
- **`publish-crates` runs `setup-rust` with `cache-key: release-publish-crates`.** Dedicated cache scope so a poisoned workspace cache from a build job cannot influence `cargo package`.
- **`actions/checkout` carries `persist-credentials: false`** on every job EXCEPT where the job actually pushes git refs. The single exception is the `release` job's rolling `v1` tag push, which uses an explicit `https://x-access-token:${GH_TOKEN}@github.com/${GITHUB_REPOSITORY}` URL to scope `GITHUB_TOKEN` to one step.
- **`expected_names[]` in `npm-publish` is load-bearing.** The 18-row TSV manifest produced by `npm-prep` is validated against (a) row count = 18, (b) per-row expected name in publish order, (c) per-row version equal to tag-derived `VERSION`, (d) the tarball's own `package/package.json` (`tar -xOf <file> package/package.json | node -e ...`) matching the manifest row. Adding or removing a platform target requires updating BOTH the pack loops in `npm-prep` AND the `expected_names` array AND the `EXPECTED=18` constant.
- **Job name `npm-publish` and workflow filename `release.yml` are part of npm trusted-publishing config.** Renaming either silently breaks OIDC. The `vscode-publish` job similarly relies on the workflow path for any future OIDC-style publishing.
- **`shell: bash` on the `npm-publish` "Publish all tarballs" step is load-bearing.** The expected_names array lookup `${expected_names[$index]}` is 0-indexed in bash and 1-indexed in zsh; the explicit shell pin prevents a runner default change from silently breaking row matching.

## Background: TanStack chain

The headline TanStack-class attack chain has two ingredients: a `pull_request_target` workflow that runs fork-controlled code (writes to the base-repo cache), AND a publish workflow that restores that cache and publishes through OIDC. We close ingredient #1 by gating `dependabot-auto-merge.yml`'s `pull_request_target` to `github.actor == 'dependabot[bot]'` with no checkout. Ingredient #2 is closed by this split: the publish jobs do not restore the workspace cache (only `publish-crates` does, on a dedicated scope), they only download artifacts produced by `npm-prep` / `vscode-prep`.

## When adding a new privileged-publish step

1. Decide which `*-prep` job builds the artifact. If it's brand-new (e.g., adding `homebrew-prep` / `homebrew-publish`), create both jobs with the same pattern.
2. The prep job carries `contents: read` only and produces a versioned artifact via `actions/upload-artifact`.
3. The publish job downloads the artifact, runs the publish CLI pinned to a specific version with `--ignore-scripts`, and exits.
4. Cross-check with `zizmor --persona auditor --no-online-audits --format plain --config .github/zizmor.yml --min-confidence medium`: the new layout must produce 0 medium / 0 high findings.

Validation cheat-sheet (run before pushing release.yml edits):

```bash
actionlint .github/workflows/release.yml
zizmor --persona auditor --no-online-audits --format plain --config .github/zizmor.yml --min-confidence medium .github/workflows/release.yml
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"
```
