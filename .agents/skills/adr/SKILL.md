---
name: adr
description: Use when a change picks between viable designs, when an earlier architecture decision is revisited, amended or reversed, or when the user asks for an ADR or architecture decision record.
---

# Writing an architecture decision record

## Overview

An ADR in `docs/adr/` records one decision the user made, short enough that the user reads
all of it. The substance is the user's: the decision, the reasons, the alternatives weighed,
the costs accepted. The prose is the agent's: clear and complete sentences, however terse the
user was. The codebase shows what was built, never why, and supplies nothing for the record.

Chat carries the file name, a concise statement of what was written or changed, and whatever
needs the user's attention, not the record itself. An inferred sentence is named in a few
words, not quoted.

## Process

1. **Collect from the conversation:** the decision, its reasons, the alternatives the user
   weighed, and what the decision costs. A decision made before the conversation is collected
   the same way: ask the user, do not reconstruct it from code or git history.
2. **Write the draft** to `docs/adr/NNNN-short-title.md` from [template.md](template.md).
   `NNNN` is the next unused four-digit number. The title states the decision ("Use
   GtkSourceView for the editor"). Status is `draft`; the date is today, unless the user
   named another day for the decision. Each claim traces back to something the user said.
   A sentence that only follows from what they said starts with `[inferred]` so they can
   strike it. A part the user has not supplied is a one-line placeholder in brackets, not a
   guess, also when the user asked for a finished record and is away.
3. **Report in a few lines:** the file as `docs/adr/NNNN-short-title.md`, what the record
   says in a sentence, the placeholders, the inferred sentences, and any question the draft
   raised. Put all questions in one message. After a change, say what changed and why.
4. **Finish on the user's word.** Their edits stand. Their answers replace the placeholders.
   When they say the record or a change to it stands, remove the remaining `[inferred]`
   markers; when they say the decision is accepted or rejected, set the status, and the
   date to that day unless they name another. The ADR goes in the same pull request as
   the change it belongs to, or in its own when it records a decision made earlier.

## Shape

Each part is as long as what the user said, and no longer:

| Part | Size |
|---|---|
| Lead paragraph | One or two sentences: the kind of decision and what called for it |
| Decision | One or two sentences, then the reasons, a sentence each |
| Consequences | One bullet per thing future work must respect or that was given up, one sentence each |
| Context | One paragraph; left out when it would repeat the lead or the reasons |
| Alternatives considered | One `###` per alternative the user weighed, one or two sentences: what it was and why not |

A template section with nothing to hold is removed, not left as a stub. The app and
`docs/prd.md` supply the names of things, and nothing more. Source files, functions, crates
and numbers read from the code stay in the code.

## Status and date

- `draft` until the user has reviewed the file and said the decision stands.
- `accepted` or `rejected` once they have.
- The date is the day the user decided: today, unless they name another day.
- `superseded by ADR-NNNN` when a later ADR replaces it.

## Amending and superseding

- **Amend** when the broad decision still holds but a detail changed. Change the affected
  sentences and add a dated line under `## Amendments`, created on the first amendment.
- **Supersede** when the decision is revoked and a new one takes its place. Write a new ADR
  and set the old one's status to `superseded by ADR-NNNN`. Its text otherwise stays as it was.

## Common mistakes

- Filling gaps from `src/`, the PRD or git history instead of leaving a placeholder and asking.
- Listing alternatives the user never considered, to make the record look thorough.
- Pasting the ADR into chat, in full or in part, instead of pointing at the file.
- Recording implementation detail the user would skim past.
