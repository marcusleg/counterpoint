---
name: adr
description: Use when a change picks between viable designs, when an earlier architecture decision is revisited, amended or reversed, or when the user asks for an ADR or architecture decision record.
---

# Writing an architecture decision record

## Overview

An ADR in `docs/adr/` records one decision the user made, in the user's words, short enough
that the user reads all of it. The record holds only what the user said or approved. The
codebase shows what was built, never why, and supplies nothing for the record.

The user reviews the whole ADR in chat before it exists as a file. Urgency ("just write it")
moves the review earlier, never past the user. When the user cannot answer, the draft waits in
chat and nothing is written.

## Process

1. **Collect from the conversation:** the decision, its reasons, the alternatives the user
   weighed, and what the decision costs. Where one of these is missing, ask for it. Put all
   questions in one message. A decision made before the conversation is collected the same
   way: ask the user, do not reconstruct it from code or git history.
2. **Draft the ADR in chat** using [template.md](template.md) and the budget below. Each
   sentence traces back to something the user said. A sentence that only follows from what
   they said is marked `[inferred]` so they can strike it; the marker goes when they keep
   it. Ask whether the draft may be written as is.
3. **Write the approved text** to `docs/adr/NNNN-short-title.md`, unchanged. `NNNN` is the
   next unused four-digit number. The title states the decision ("Use GtkSourceView for the
   editor"). The ADR goes in the same pull request as the change it belongs to, or in its
   own when it records a decision made earlier.

## Budget

A finished ADR fits on one screen: at most 200 words below the header table.

| Part | Size |
|---|---|
| Lead paragraph | One or two sentences: the kind of decision and what called for it |
| Decision | One or two sentences, then the reasons in at most three sentences |
| Consequences | At most three bullets, one sentence each: what future work must respect, what was given up |
| Context | One paragraph, at most four sentences; left out when it would repeat the lead or the reasons |
| Alternatives considered | At most three, one `###` each, one or two sentences: what it was and why not |

Things are named as the user names them. Source files, functions, crates and numbers read
from the code stay in the code.

## Status and date

- `draft` while the user has not yet accepted the decision. The date cell stays empty.
- `accepted` or `rejected` once they have, with the date the user decided.
- `superseded by ADR-NNNN` when a later ADR replaces it.

## Amending and superseding

- **Amend** when the broad decision still holds but a detail changed. Change the affected
  sentences and add a dated line under `## Amendments`, created on the first amendment. The
  changed text goes through the same chat review.
- **Supersede** when the decision is revoked and a new one takes its place. Write a new ADR
  and set the old one's status to `superseded by ADR-NNNN`. Its text otherwise stays as it was.

## Common mistakes

- Filling gaps from `src/`, the PRD or git history instead of asking the user.
- Listing alternatives the user never considered, to make the record look thorough.
- Writing the file first and offering the user a review afterwards.
- Recording implementation detail the user would skim past.
- Marking a decision `accepted` on a date the user did not name.
