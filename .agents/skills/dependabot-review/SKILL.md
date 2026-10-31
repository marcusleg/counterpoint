---
name: dependabot-review
description: Use when reviewing a Dependabot pull request that was not merged automatically, such as a major update, a minor update of a direct crate below 1.0, or a gtk-rs update, or when deciding whether such an update is safe to merge.
---

# Reviewing a Dependabot update

Dependabot's patch and minor updates merge on their own once CI passes. The rest wait for a
review, because they can break things that CI passing does not prove.

## Review

- Read the changelog and release notes for every version between the old one and the new one,
  not just the newest. Look for breaking changes, removals, changed defaults, deprecations and
  raised requirements.
- Work out which of them affect Counterpoint, and how.
- Check the new version against what the project has to keep supporting:
  - the `rust-version` in `Cargo.toml`;
  - the GTK, libadwaita and GtkSourceView versions on the older of the two supported Fedora
    releases and in the GNOME runtime of the Flatpak.
- The gtk-rs crates (`gtk4`, `libadwaita`, `sourceview5`) only work together. Review them as one
  update.
- Note what CI does not cover, such as the release builds in `release.yml`.
- If the update needs code changes, make them in the same pull request. If it changes
  user-visible behaviour, update `docs/prd.md` too.

## Report

1. A verdict: merge, merge after the changes you made, or hold.
2. The breaking changes that affect Counterpoint, each with a link to its source.
3. What you checked that CI did not.

Leave the merge to the user.
