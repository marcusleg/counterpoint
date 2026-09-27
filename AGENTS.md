# AGENTS.md

## Workflow

- Never commit directly to `main`. Create a branch before making code changes.
- When the work is done, push the branch and open a pull request against `main` with `gh pr create`.
- Wait for the CI checks to pass before merging a pull request that changes code. CI only runs
  for the paths listed in `.github/workflows/ci.yml`, so a pull request that only changes
  documentation has no checks to wait for.
- Merge pull requests with rebase and merge by default. Squash and merge is also allowed.
- Once the pull request is merged, delete the branch both locally and on the remote.

## Product requirements

- `docs/prd.md` describes the product as built. When a change adds, removes or changes
  user-visible behaviour, update the matching requirements in the same pull request.
- Keep requirement IDs stable: give a new requirement the next unused number in its list, and
  delete a removed one without renumbering the rest.
