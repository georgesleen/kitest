Closes #

## Purpose

## Description

## Testing

## Definition of done

See [docs/workflow.md](https://github.com/georgesleen/kitest/blob/main/docs/workflow.md).

- [ ] The body above says `Closes #<N>`.
- [ ] `nix develop -c make fmt-check lint stubs-check test` passes.
- [ ] The branch is pushed by name, and `git ls-remote` shows the local `HEAD`.
- [ ] A separate reviewer reviewed the PR, every finding is addressed, and the MERGE verdict is posted on the PR.
- [ ] Out-of-scope findings are new linked issues.
- [ ] Decisions are recorded in the issue or in `docs/decisions.md`.
- [ ] AI-written commits carry the `Co-Authored-By: Claude` trailer.
- [ ] The checks are green, and the PR is handed to George, who alone merges.
- [ ] After George merges: the worktree and the local and remote branches are removed, and the issue is closed.
