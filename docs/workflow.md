# Ticket workflow

This is the definition of done for a ticket. One owner, a person or an agent,
carries an issue from claim to merge and cleanup.

`main` is protected. A change reaches it only through a PR whose `gate` check
passes, with linear history. Admins follow the same rules.

## Issues and the roadmap

`docs/roadmap.md` holds long-range direction. Issues hold work that can close.
An issue names its roadmap section with one `area:` label; work outside the
roadmap takes `area:infra`.

## Definition of done

1. **Claim.** Assign the issue to yourself, or comment on it. Then create the
   branch `issue-<N>-<slug>` and a worktree for it, as two commands:

   ```sh
   git branch issue-<N>-<slug> main
   git worktree add worktrees/issue-<N> issue-<N>-<slug>
   ```

   Do not use `git worktree add -b`. That form has hung and left a recursive
   copy of the repo.
2. **Implement.** Run every command in the nix dev shell, with
   `CARGO_TARGET_DIR` set to the main checkout's `target/` (`.envrc` sets it
   under direnv). The gate is the command CI runs:

   ```sh
   nix develop -c make fmt-check lint stubs-check test
   ```

   Push the branch by name, then confirm that the remote head is your `HEAD`:

   ```sh
   git push origin issue-<N>-<slug>
   git ls-remote origin refs/heads/issue-<N>-<slug>
   ```

3. **Open a PR** from the template. Its body contains `Closes #<N>`; the
   `issue-link` check fails without it.
4. **Review.** A separate reviewer reviews the PR. The owner addresses each
   finding with new commits, and the reviewer reviews again until it approves.
5. **Merge** once CI is green, with `gh pr merge <PR> --rebase --delete-branch`.
   Remove the worktree before the merge, then delete the local branch and, if
   it survives, the remote branch. The issue closes through `Closes #<N>`.
6. **Out-of-scope findings** become new issues that link back to this one.
7. **Decisions** made during the work go into the issue. A lasting decision
   goes into `docs/decisions.md`.
8. **Commit trailer.** A commit that an AI wrote or co-wrote ends with
   `Co-Authored-By: Claude <noreply@anthropic.com>`. A commit that George wrote
   alone has no trailer.
9. **Report evidence, not prose.** An agent reports the PR URL, the merge commit
   on `main`, and the issue state.

## Roles

- **Orchestrator:** splits work into issues, assigns owners, and arranges
  reviews. It does not implement.
- **Implementer:** owns one issue end to end, through the definition of done.
- **Pair:** supports George while he writes the code himself. It explains,
  designs, and reviews, but does not write the source.
- **Reviewer:** reviews a PR it did not write, and returns a verdict with
  findings.
