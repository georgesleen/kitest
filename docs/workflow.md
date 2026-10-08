# Ticket workflow

This is the definition of done for a ticket. One owner, a person or an agent,
carries an issue from claim to a reviewed, green PR, and cleans up after
George merges it.

`main` is protected. A change reaches it only through a PR with linear history,
a passing `gate` check, and one approving review. Admins are exempt from the
review rule.

Agents open PRs as `georgesleen` until a bot identity exists (#6), so GitHub
cannot provide the required independent approval from the PR author. Do not
enable required code-owner reviews until that identity exists.

## Commit messages

Commit messages are one terse line in the form `<system>: <imperative
message>`, with a lowercase affected area such as `github`, `scope`, `kicad`,
`sim`, `docs`, `nix`, or `python`. AI-authored commits also include the
required `Co-Authored-By` trailer.

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
4. **Review.** Every PR needs one review by a separate reviewer. The owner
   addresses each finding with new commits, and the reviewer reviews again
   until its verdict is MERGE. GitHub does not let the author approve their
   own PR, and agents open PRs as georgesleen. Until a bot identity exists, the
   review is a reviewer agent's final verdict, posted on the PR:

   ```sh
   gh pr review <PR> --comment --body "<verdict and findings>"
   ```

   Every blocking finding cites its evidence: a doc link, a test, or a
   command and its output. A finding without evidence is a question to
   investigate, not a required change. The owner confirms the evidence
   before acting on the finding.

5. **Merge.** Only George merges. Once CI is green and the MERGE verdict is
   posted, the owner hands the PR to George. He merges it as an admin, since
   no one can approve the PR yet:

   ```sh
   gh pr merge <PR> --rebase --delete-branch --admin
   ```

   After the merge, the owner removes the worktree and the local branch,
   deletes the remote branch if it survives, and checks that the issue
   closed through `Closes #<N>`:

   ```sh
   git worktree remove worktrees/issue-<N>
   git branch -D issue-<N>-<slug>
   gh api -X DELETE repos/georgesleen/kitest/git/refs/heads/issue-<N>-<slug>
   gh issue view <N> --json state
   ```
6. **Out-of-scope findings** become new issues that link back to this one.
7. **Decisions** made during the work go into the issue. A lasting decision
   goes into `docs/decisions.md`.
8. **Commit trailer.** A commit that an AI wrote or co-wrote ends with
   `Co-Authored-By: Claude <noreply@anthropic.com>`. A commit that George wrote
   alone has no trailer.
9. **Report evidence, not prose.** At the hand-off, an agent reports the PR
   URL, the reviewer's verdict, and the check runs. After the merge, it
   reports the merge commit on `main` and the issue state.

## Roles

- **Orchestrator:** splits work into issues, assigns owners, and arranges
  reviews. It does not implement, and it never merges.
- **Implementer:** owns one issue end to end, through the definition of done.
  It never merges; it hands the PR to George.
- **Pair:** supports George while he writes the code himself. It explains,
  designs, and reviews, but does not write the source.
- **Reviewer:** reviews a PR it did not write, and returns a verdict with
  findings.
