# Homebrew Distribution

This directory holds the source-of-truth Homebrew formula template for
workhive. The tap itself is a separate repository; this document describes how
to publish and update it.

## Prerequisites

- The `yboker/workhive` repository must be **public**. Homebrew downloads
  release tarballs anonymously; private repositories will fail both `brew
  install` and direct release downloads.
- At least one GitHub release published by `.github/workflows/release.yml`
  (triggered by pushing a `v*` tag that matches the workspace version in
  `Cargo.toml`).

## One-time tap setup

1. Create a public repository named `homebrew-workhive` under the `yboker`
   account. Homebrew resolves `brew tap yboker/workhive` to
   `github.com/yboker/homebrew-workhive`.
2. In that repository, create a `Formula/` directory.
3. Copy `packaging/homebrew/workhive.rb` from this repository into
   `Formula/workhive.rb` in the tap.

## Publishing / updating a release

1. Push a version tag (for example `v2.0.1`) and wait for the release
   workflow to publish the GitHub release with three tarballs and their
   `.sha256` files:
   - `workhive-<version>-aarch64-apple-darwin.tar.gz`
   - `workhive-<version>-x86_64-apple-darwin.tar.gz`
   - `workhive-<version>-x86_64-unknown-linux-gnu.tar.gz`
2. In the tap's `Formula/workhive.rb`, replace:
   - every `REPLACE_VERSION` with the release version (without the `v`
     prefix);
   - each `REPLACE_SHA256_*` with the hex digest from the matching
     `.tar.gz.sha256` release asset (first field of the file).
3. Commit and push the tap.

## Installing

```bash
brew install yboker/workhive/workhive
# or, in two steps:
brew tap yboker/workhive
brew install workhive
```

Verify:

```bash
workhive --version
```
