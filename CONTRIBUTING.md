## Commit Messages

We use a structured commit style to keep history clear and reviewable.
Each branch or pull request must contain **exactly one commit** representing the
final, polished change. This means you should update and rewrite your commit
message as the branch evolves instead of adding new commits.

### Subject line

Use:

```
CRATE_OR_AREA: what changed (one line)
```

`CRATE_OR_AREA` is usually a crate name (e.g. `hearthd_config`, `hearthd_core`).  
If the change spans multiple crates, use the most specific shared area  
(e.g. `config`, `scheduler`, `docs`).

Example:

```
hearthd_config: log file paths explicitly when deserializing
```

### Body

Write the body as short prose with the following structure:

- 1–3 sentences describing the **issue or behaviour** this commit is fixing.  
- 1–3 sentences describing **how** the change was implemented.  
- 1–2 sentences explaining **why** this implementation fixes the issue.

Reference example:

```
The procedural macros wrapped config field values in toml::Spanned<T>,
which only tracks byte spans within a file. The merge conflict detection
code manually constructed MergeConflictLocation objects by combining the
span from the Spanned<T> value with the source_info parameter (which
tracked the file currently being processed). This worked correctly but
required repetitive boilerplate code at every conflict detection site.

This commit introduces hearthd_config::Located<T> to replace toml::Spanned<T>.
Located<T> bundles the value, byte span (Range<usize>), and complete source
information (SourceInfo with file path and content) together. During TOML
deserialization, Located<T> reads from toml::Spanned<T> and creates a
placeholder SourceInfo. After loading each config file, a generated
attach_source_info() method walks through all Located<T> fields and attaches
the actual file's SourceInfo.

This eliminates the manual MergeConflictLocation construction boilerplate by
adding a to_conflict_location() method to Located<T>. The proc-generated merge
code now calls value.to_conflict_location() instead of manually extracting
span and source_info and combining them. This reduces code duplication and
makes the data flow clearer—each value knows where it came from.
```

### Test plan

Every commit message ends with a `Test plan:` block:

```
Test plan:
- All existing tests pass.
- Added new tests for <feature>.
- Verified error messages via snapshot tests.
- Manually tested <workflow>.
```

### One–commit rule

This repository follows a **one commit per branch / one commit per PR** policy.

- Do not create `fixup!` commits, `v2`, `v3`, or iterative commits.
- Instead, **amend** your single commit as your work changes.
- Before opening or updating a PR, ensure the single commit contains the
  final, polished message and complete description of the change.

### Template

The `.git-commit-template` file at the repository root provides a ready-to-use
skeleton. Enable it locally with:

```
git config commit.template "$(git rev-parse --show-toplevel)/.git-commit-template"
```
