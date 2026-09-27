---
name: release
description: Use when releasing, cutting, tagging or publishing a new version of Counterpoint, bumping the version number, or when a Release workflow run or GitHub release needs attention.
---

# Releasing Counterpoint

## Overview

Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml`, which builds a Flatpak bundle and
an RPM and publishes a GitHub release. The tag is public the moment it is pushed and the
tag/version check runs only after the builds, so everything is verified before tagging.
Versions follow [Semantic Versioning](https://semver.org).

## 1. Choose the version

```sh
git fetch origin --tags
git describe --tags --abbrev=0 origin/main   # last release; fails if there is none
git log --oneline <last-tag>..origin/main
git diff <last-tag>..origin/main -- docs/prd.md
```

If no tag exists, the version in `Cargo.toml` has never shipped: release it as it is (skip the
bump in step 2). Otherwise pick the bump from what changed since the last tag. `docs/prd.md`
records every user-visible change, so its diff is the main evidence.

| Change since the last tag | Bump |
|---|---|
| Incompatible: a requirement removed or changed so existing use breaks, settings or state files no longer read, a supported Fedora or runtime dropped | MAJOR |
| New user-visible behaviour: a new requirement ID, a new option | MINOR |
| Only fixes, performance, docs, packaging, dependency updates | PATCH |
| Nothing since the last tag | No release; tell the user |

- Before 1.0.0, an incompatible change bumps MINOR instead of MAJOR. Going to 1.0.0 is the
  user's decision; never pick it yourself.
- Pre-releases use a semver suffix, such as `0.3.0-rc.1`. The workflow marks any version
  containing `-` as a pre-release. Don't use `+build` metadata.

Tell the user the proposed version and the reason in one line, and let them confirm or change
it. A version the user named is used as given, unless nothing changed since the last tag: then
say so and release only if they confirm.

Urgency ("quickly", "just push it") never skips the checks, the dry run or the questions below.

## 2. Release pull request

AGENTS.md applies: work on a branch named `release-X.Y.Z` from an up-to-date `origin/main`.

1. Set `version` in `Cargo.toml`, then run `cargo update -p counterpoint --offline` so
   `Cargo.lock` matches. The release builds use `--locked` and fail on a stale lock file.
2. Add the release to `data/de.marcusleg.Counterpoint.metainfo.xml`, newest first, creating
   `<releases>` after `<content_rating>` if it is missing:
   `<release version="X.Y.Z" date="YYYY-MM-DD"/>` (the date is today's date).
3. Check that the build containers are current. The RPM is built on the older of the two
   supported Fedora releases, which are listed by:
   ```sh
   curl -s 'https://bodhi.fedoraproject.org/releases/?state=current&rows_per_page=50' \
     | python3 -c 'import json,sys; print(sorted({r["version"] for r in json.load(sys.stdin)["releases"] if r["id_prefix"]=="FEDORA"}, key=int))'
   ```
   If the oldest listed Fedora is newer than the `fedora:NN` container of the `rpm` job, move
   that container to the older of the two newest Fedoras, and update every Fedora version
   named in README.md (Install lists the two, newest first; Releases names the container).

   The Flatpak uses the GNOME runtime in `build-aux/de.marcusleg.Counterpoint.yml`
   (`runtime-version`). If `flatpak remote-info flathub org.gnome.Platform/x86_64/<version>`
   prints an `End-of-life:` line, tell the user and, if they agree, move the manifest, the
   `gnome-NN` image of the `flatpak` job and README.md to a supported version.
4. Run the development checks from README.md (fmt, clippy, tests under `dev/headless.sh`).
5. Commit as `Release X.Y.Z`, push, and open the PR with `gh pr create`.
6. Dry-run the packaging on the branch. The release job is skipped for untagged runs:
   ```sh
   gh workflow run release.yml --ref release-X.Y.Z
   gh run list --workflow release.yml --event workflow_dispatch --commit "$(git rev-parse HEAD)"
   gh run watch <run-id> --exit-status
   ```
   The new run can take a few seconds to appear in the list.
   Fix any failure on the branch before going further.
7. Once CI and the dry run pass, ask the user whether to merge. On a yes:
   `gh pr merge --squash --delete-branch`, then delete the local branch.

## 3. Tag

```sh
git switch main && git pull --ff-only
cargo pkgid | sed 's/.*[#@]//'      # must print X.Y.Z
gh run list --workflow ci.yml --commit "$(git rev-parse HEAD)"   # completed and green; watch it if not
```

Ask the user before pushing the tag. Then:

```sh
git tag -a vX.Y.Z -m "Counterpoint X.Y.Z"
git push origin vX.Y.Z
```

## 4. Verify

Watch the Release run for the tag until it finishes, then check the release:

```sh
gh run list --workflow release.yml --event push --commit "$(git rev-parse HEAD)"
gh run watch <run-id> --exit-status
gh release view vX.Y.Z --json name,isPrerelease,assets -q '{name, isPrerelease, assets: [.assets[].name]}'
```

Expect the name `Counterpoint X.Y.Z`, `isPrerelease` true exactly when the version has a
suffix, and two assets: `counterpoint-X.Y.Z-x86_64.flatpak` and `counterpoint-X.Y.Z-1.x86_64.rpm`.
Report the release URL to the user.

## When the Release run fails

| Situation | Action |
|---|---|
| Failed and no release was created | Fix it in a PR. Then, with the user's consent, delete the tag (`git push origin :refs/tags/vX.Y.Z && git tag -d vX.Y.Z`) and tag the fixed `main` again |
| Release exists but is wrong or incomplete | Never move a published tag. Tell the user and ship the fix as the next PATCH |

## Common mistakes

- Committing the version bump directly to `main`.
- Forgetting `Cargo.lock`: the tag check passes, but every build fails on `--locked`.
- Tagging before the PR is merged, or tagging a local `main` that is behind `origin/main`.
- Reading "release a new version" as permission to push the tag without asking.
- Changing `docs/prd.md` for the bump alone: a version bump is not a behaviour change.
