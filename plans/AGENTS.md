# plans/AGENTS.md

This directory holds planning documents in markdown. It contains a mix of
work in progress, plans that have already been fully implemented, and plans
that are outdated or no longer accurate — nothing here is guaranteed to
reflect current code.

**Do not read files in this directory unless you are actively working on a
specific plan, or trying to understand the original implementation of a
specific feature.** Do not scan or summarize this directory as general
project context, and do not treat its contents as current documentation.

## Planning

When planning work, the output is always a new plan file written to this
directory — not code changes. Implementation happens later, in a new session
with fresh context, using the plan file's content as the spec.

Before writing the plan, ask as many questions as necessary to build a strong
shared understanding of the goals with the user — do not guess at intent or
fill gaps with assumptions. Capture that shared understanding in the plan
file itself, so a fresh session reading only the plan has the same context.

## Executing a plan

While implementing a plan, new information or learnings may surface that
warrant deviating from it. Before proceeding with any deviation, stop and
confirm it with the user. When a deviation is approved, update the plan file
itself to reflect the change so it stays an accurate spec.

## Finishing a plan

Once work on a plan appears complete, review the plan and the resulting
changes in their entirety:

- If the implementation deviated from the plan, confirm each deviation was
  approved and that the plan file was updated to reflect it.
- Clean up the plan file: remove back-and-forth historical narration (earlier
  drafts, rejected approaches, revision commentary) that would no longer help
  someone reading it later to understand what was actually built. The final
  plan file should read as an accurate account of the implementation, not a
  transcript of how it was arrived at.
