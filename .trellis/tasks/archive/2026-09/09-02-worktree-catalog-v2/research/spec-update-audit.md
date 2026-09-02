# Spec Update Audit

- Added `.trellis/spec/mt-project/backend/worktree-catalog-contract.md` with the executable parser, authority, generation, path, compatibility, and destructive-cleanup contracts established by this child.
- Did not edit `.trellis/spec/mt-project/backend/index.md` or the shared quality files because active task `00-bootstrap-guidelines` owns those uncommitted paths. The bootstrap task should add the contract to its index when it consolidates the package specs.
- Updated child and parent planning artifacts to reflect the repository's actual rollback mechanism: unchanged compatibility APIs plus code revert for this schema-free child, rather than an invented one-off runtime feature gate.
