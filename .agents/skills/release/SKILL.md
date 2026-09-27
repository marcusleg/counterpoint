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

A release has three phases:

1. **Prepare**, read-only: work out the version, check the build containers, draft the notes.
2. **Ask once**: every decision goes to the user in a single round of questions. Their approval
   covers the whole release: pull request, merge, tag and published notes.
3. **Release**: run to the published release without further questions. Stop only for the
   situations in [Stop and ask](#stop-and-ask).

## 1. Prepare

Change nothing in this phase: no branch, no edits, no pushes.

The version, Fedora and runtime checks below don't depend on each other: start all three at
once (parallel tool calls in one turn) and draft the notes once the version check returns.

### Version

```sh
git fetch origin --tags
git describe --tags --abbrev=0 origin/main   # last release; fails if there is none
git log --oneline <last-tag>..origin/main
git diff <last-tag>..origin/main -- docs/prd.md
```

If no tag exists, the version in `Cargo.toml` has never shipped: propose it as it is. Otherwise
pick the bump from what changed since the last tag. `docs/prd.md` records every user-visible
change, so its diff is the main evidence.

| Change since the last tag | Bump |
|---|---|
| Incompatible: a requirement removed or changed so existing use breaks, settings or state files no longer read, a supported Fedora or runtime dropped | MAJOR |
| New user-visible behaviour: a new requirement ID, a new option, a new way to install | MINOR |
| Only fixes, performance, docs, dependency updates, packaging changes users don't notice | PATCH |

When changes fall into several rows, the highest bump wins.

- Before 1.0.0, an incompatible change bumps MINOR instead of MAJOR. Going to 1.0.0 is the
  user's decision; never propose it yourself.
- Pre-releases use a semver suffix, such as `0.3.0-rc.1`. The workflow marks any version
  containing `-` as a pre-release. Don't use `+build` metadata.
- A version the user named is used as given.

### Build containers

The RPM is built on the older of the two supported Fedora releases, which are listed by:

```sh
curl -s 'https://bodhi.fedoraproject.org/releases/?state=current&rows_per_page=50' \
  | python3 -c 'import json,sys; print(sorted({r["version"] for r in json.load(sys.stdin)["releases"] if r["id_prefix"]=="FEDORA"}, key=int))'
```

If the oldest listed Fedora is newer than the `fedora:NN` container of the `rpm` job, the
`fedora:NN` containers in `release.yml` and `ci.yml` move to the older of the two newest
Fedoras. This follows from the support policy and needs no question.

The Flatpak uses the GNOME runtime in `build-aux/de.marcusleg.Counterpoint.yml`
(`runtime-version`). If `flatpak remote-info flathub org.gnome.Platform/x86_64/<version>`
prints an `End-of-life:` line, moving to a newer runtime is a question for the user.

### Release notes

Draft them as described in [Release notes](#release-notes).

## 2. Ask once

Put every decision to the user in one round. Use the AskUserQuestion tool where it is available,
with the full draft notes in the question text or a preview; otherwise ask in one message with
numbered questions:

- **Version:** the proposed version and the reason in one line, with the alternatives.
- **Release notes:** the draft, to approve or edit.
- **GNOME runtime:** only if it is end-of-life, whether to move to a newer one.
- **Nothing to release:** only if nothing changed since the last tag, whether to release at all.

Mention a Fedora container move as information, not as a question.

State that approving starts the release and that it runs through to the published release
without further questions unless something fails. Urgency ("quickly", "just push it") never
skips this round or the checks.

## 3. Release

Work on a branch named `release-X.Y.Z` from an up-to-date `origin/main` (AGENTS.md: never
commit to `main`). If `origin/main` moved since the preparation (`git rev-parse origin/main`
after a fetch differs from the commit the preparation looked at), [stop and ask](#stop-and-ask).

Workflow runs take minutes. Start every watch below as a background command where the harness
supports one and wait for its completion notice instead of polling; elsewhere run it in the
foreground. The exit status is the result, so the progress output goes to `/dev/null`; after a
failure, read only `gh run view <run-id> --log-failed`.

1. If the approved version differs from the one in `Cargo.toml`, set `version` there and run
   `cargo update -p counterpoint --offline` so `Cargo.lock` matches. The release builds use
   `--locked` and fail on a stale lock file. `cargo pkgid | sed 's/.*[#@]//'` must now print
   X.Y.Z.
2. Add the release to `data/de.marcusleg.Counterpoint.metainfo.xml`, newest first, creating
   `<releases>` after `<content_rating>` if it is missing:
   `<release version="X.Y.Z" date="YYYY-MM-DD"/>` (today's date).
3. Apply the container changes from the preparation. A Fedora move changes the `fedora:NN`
   containers in `release.yml` and `ci.yml` and every Fedora version named in README.md
   (Install lists the two, newest first; Releases names the container). An approved runtime
   move changes the manifest, the `gnome-NN` image of the `flatpak` job and README.md.
4. Commit as `Release X.Y.Z`, push, and open the PR with `gh pr create`. Its description
   records the approved notes under a `## Release notes` heading. Note the commit's hash
   (`git rev-parse HEAD`) for step 8. The development checks don't run locally: the PR's CI
   runs the same ones.
5. Start the packaging dry run on the branch; the release job is skipped for untagged runs.
   `gh workflow run release.yml --ref release-X.Y.Z` prints the new run's URL, which ends in
   its run id.
6. Wait for the dry run and the PR's CI in one background command. Both are already running,
   so watching them one after the other takes no longer than watching them side by side:
   ```sh
   gh run watch <run-id> --exit-status >/dev/null || echo "dry run failed"
   gh pr checks release-X.Y.Z --watch --fail-fast >/dev/null || echo "CI failed"
   ```
   A commit that touches none of the paths `ci.yml` runs on (only the metainfo changed) gets no
   CI: `gh pr checks` then prints `no checks reported` and exits 1, which is not a failure, and
   the dry run alone decides.
7. When both pass, merge with `gh pr merge --rebase --delete-branch` (AGENTS.md: rebase and
   merge by default) and delete the local branch.
8. Check that `main` holds exactly the tested release commit:
   ```sh
   git fetch origin
   git rev-parse 'origin/main^{tree}' '<release-commit>^{tree}'   # must print one hash twice
   ```
   The rebase merge gives the commit a new hash but keeps its tree, which the PR's CI and the
   dry run have checked, so `main`'s own CI run isn't awaited. A different tree means `main`
   moved: [stop and ask](#stop-and-ask).
9. Tag the fetched `origin/main` and push the tag:
   ```sh
   git tag -a vX.Y.Z -m "Counterpoint X.Y.Z" origin/main
   git push origin vX.Y.Z
   ```
10. Watch the tag's Release run in the background, then check the release:
    ```sh
    gh run list --workflow release.yml --event push --commit "$(git rev-parse 'vX.Y.Z^{commit}')"
    gh run watch <run-id> --exit-status >/dev/null
    gh release view vX.Y.Z --json name,isPrerelease,assets -q '{name, isPrerelease, assets: [.assets[].name]}'
    ```
    The run can take a few seconds to appear in `gh run list`. Expect the name
    `Counterpoint X.Y.Z`, `isPrerelease` true exactly when the version has a suffix, and two
    assets: `counterpoint-X.Y.Z-x86_64.flatpak` and `counterpoint-X.Y.Z-1.x86_64.rpm`.
11. The workflow publishes GitHub's generated notes, a list of pull request titles. Replace
    them with the approved notes, written to a temporary file outside the repository:
    `gh release edit vX.Y.Z --notes-file <notes-file>`.
12. Report the release URL and what was done, in a few lines.

## Stop and ask

The approval covers the release as prepared. Stop, report what happened, and propose a fix in
these cases:

- `origin/main` moved after the preparation: the version or notes may no longer fit.
- CI or the dry run fails. `main` was green, so the failure is unexpected, and
  fixing it means changing more than the release. The one exception: when `gh run view <id>
  --log-failed` shows a network or download error, rerun the failed jobs once
  (`gh run rerun <id> --failed`) and stop only if they fail again.
- The Release run fails, or the release is wrong or incomplete:

  | Situation | Proposal |
  |---|---|
  | Failed and no release was created | Fix it in a PR, delete the tag (`git push origin :refs/tags/vX.Y.Z && git tag -d vX.Y.Z`) and tag the fixed `main` again |
  | Release exists but is wrong or incomplete | Never move a published tag; ship the fix as the next PATCH |

- Anything else the approved plan does not cover.

## Release notes

The notes are for people who use Counterpoint, not for its developers. Their shape:

```markdown
<One or two sentences: what this release means for a writer using Counterpoint.>

## Features

- <Something new a user can do, or existing behaviour that now works differently.>

## Fixes

- <Something that went wrong before and now works, described as the user saw it.>

**Full Changelog**: <link>
```

A section with no bullets is left out.

- **Sources:** the `docs/prd.md` diff and the commits since the last tag. For a first release,
  use the PRD's requirements and describe what the app does. README.md may supply facts, such
  as the supported Fedora versions.
- **Bullets:** one sentence each, for one change a user notices while writing, configuring,
  installing or updating. Name things as the app does (Sparring, Ghostwriting, Preferences) and
  put button and menu labels in bold. The most noticeable change comes first, and several
  commits about one feature make one bullet.
- **Length:** at most eight bullets per section. A first release covers the handful of things
  that define the app, not every requirement.
- **Left out:** anything a user cannot notice. That covers changes to CI, tests, the README,
  the PRD, `AGENTS.md` and agent skills, and refactoring. Also left out: pull request numbers,
  authors, source file names and crate names.
- **Features or Fixes:** a bug fix goes under Fixes; everything else a user notices goes under
  Features. For a first release, everything is a feature.
- **Only internal changes:** when no change is noticeable, both sections are left out and the
  introduction says that nothing changes when you use Counterpoint.
- **Full Changelog link:** `https://github.com/marcusleg/counterpoint/compare/<last-tag>...vX.Y.Z`,
  or `https://github.com/marcusleg/counterpoint/commits/vX.Y.Z` for a first release.

## Common mistakes

- Asking questions one at a time during the release instead of in the single round.
- Committing the version bump directly to `main`.
- Forgetting `Cargo.lock`: the tag check passes, but every build fails on `--locked`.
- Tagging before the PR is merged, or tagging anything but the fetched `origin/main`.
- Pushing the tag before the user approved the release.
- Changing `docs/prd.md` for the bump alone: a version bump is not a behaviour change.
- Leaving GitHub's generated pull request list as the release notes.
