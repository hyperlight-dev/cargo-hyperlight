# Releasing `cargo-hyperlight`

Releases are fully automated by the [`publish`](../.github/workflows/publish.yml) workflow.
Pushing a tag of the form `vX.Y.Z` to `hyperlight-dev/cargo-hyperlight` will:

1. Run the full [CI](../.github/workflows/ci.yml) suite (tests, spell check, lint).
2. Validate that the tag version matches the `version` field in [`Cargo.toml`](../Cargo.toml).
3. Publish the crate to [crates.io](https://crates.io/crates/cargo-hyperlight).
4. Create a GitHub Release with auto-generated release notes.

The crates.io publish uses trusted publishing via
[`rust-lang/crates-io-auth-action`](https://github.com/rust-lang/crates-io-auth-action),
so no long-lived registry token is needed.

## Prerequisites

- Write access to `hyperlight-dev/cargo-hyperlight` (to push tags).
- `main` is green in CI.
- All changes intended for the release are already merged into `main`.

## Steps

### 1. Bump the crate version

Open a pull request against `main` that bumps `version` in [`Cargo.toml`](../Cargo.toml).

```toml
[package]
name = "cargo-hyperlight"
version = "X.Y.Z"
```

Then refresh the lockfile and sanity-check the build:

```sh
cargo check
just fmt
just clippy
just test
```

Make sure `Cargo.lock` is included in the commit, since the version bump changes it.

Get the pull request reviewed and merged.

### 2. Verify the release candidate

Once the version bump is merged, confirm CI on `main` is green and that the
package builds exactly as it will be published:

```sh
git switch main
git pull upstream main
cargo publish --dry-run
```

### 3. Tag and push

Tag the merge commit on `main` and push the tag to the upstream repository:

```sh
git tag -a vX.Y.Z -m "vX.Y.Z"
git push upstream vX.Y.Z
```

> The tag **must** be `v` followed by the exact `Cargo.toml` version
> (for example `v0.1.14`). The workflow only triggers on tags matching
> `v[0-9]+.[0-9]+.[0-9]+`, and the `validate` job fails if the tag and the
> manifest version disagree.

### 4. Watch the workflow

Follow the run under the repository's **Actions** tab (or with
`gh run watch`). When it finishes, verify:

- The new version appears on <https://crates.io/crates/cargo-hyperlight>.
- A GitHub Release for `vX.Y.Z` was created with generated notes.
- `cargo install cargo-hyperlight` picks up the new version.

## Troubleshooting

**The workflow did not start.**
The tag does not match `v[0-9]+.[0-9]+.[0-9]+`. Pre-release suffixes such as
`v1.0.0-rc1` are not supported by the trigger.

**`validate` failed with a version mismatch.**
The tag and `Cargo.toml` disagree. Delete the tag, fix the version, and retag:

```sh
git push upstream :refs/tags/vX.Y.Z
git tag -d vX.Y.Z
```

**`publish` failed after CI passed.**
crates.io versions are immutable and cannot be re-published. If the upload
partially succeeded, bump to the next patch version and start over. If it
failed before upload (for example a transient registry error), re-running the
failed jobs from the Actions UI is safe.

**The GitHub Release is missing but the crate published.**
Only the `release` job failed. Create the release manually:

```sh
gh release create vX.Y.Z --title vX.Y.Z --generate-notes
```
