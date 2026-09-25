## Agent Commit Guidelines

Agents generating commits **must** follow the commit message rules from
`CONTRIBUTING.md` and use the `.git-commit-template` as the canonical format.

Agents must read:

1. The “Commit Messages” section in `CONTRIBUTING.md`.  
2. The `.git-commit-template` file.  
3. The rules in this AGENTS.md file.

### Required structure

Subject line:

```
CRATE_OR_AREA: what changed (one line)
```

Body must include:

- 1–3 sentences describing the issue being fixed.  
- 1–3 sentences describing how the change was implemented.  
- 1–2 sentences explaining why this approach fixes the issue.

### Test plan

Every commit ends with:

```
Test plan:
- Why existing tests cover this change AND/OR
- What new tests were added AND/OR
- Any manual testing that was performed
```

If the agent cannot run tests:

```
Test plan:
- Unable to run tests in this environment. Developer must verify.
```

### One–commit rule

This repository strictly follows a **one commit per branch / one commit per PR** model.

Agents must:

- Maintain **exactly one commit** on the branch.  
- **Amend** the commit when making changes during review.  
- Update the commit message to reflect the final, complete understanding of the
  change.  
- Never produce commits like `v2`, `v3`, `fixup!`, “address review comments”, or
  messages referencing earlier iterations.  
- Never leave stale or outdated descriptions; rewrite the message so that it
  reads as if the commit was authored perfectly the first time.

This ensures reviewers see a clean, final commit that accurately represents the
change without iteration noise.

### Additional rules

- Do **not** generate placeholder messages like “WIP”.  
- Do **not** include unrelated files in a commit.  
- Keep commit text concise, accurate, and human-readable.  
- Prefer crate-level `CRATE_OR_AREA` names unless the change spans multiple areas.

## Repository working notes

This section is for general notes on working in the repository. Add further
operational guidance here as it arises.

### Formatting

**Always** format the repository with the official formatter, and run it across
the **entire repository** before every commit:

```
nix fmt .
```

`nix fmt .` formats **all** code in the repo — every language — in one pass. It
is the single source of truth for formatting, defined once in `flake.nix` and
easily replicated locally and in CI. Never reach for a language-specific
formatter by hand (`cargo fmt`, `nixpkgs-fmt`, alejandra, etc.) or shape code
manually to approximate it — those will diverge from what CI enforces.

`nix fmt` is backed by `treefmt-nix`, which centralises every language's
formatter (currently the pinned fenix `rustfmt` with `rustfmt.toml` for Rust,
`nixpkgs-fmt` for `.nix` files, and so on). When a new language is added to the
repo, its formatter is registered in the `treefmtEval` block in `flake.nix` —
not invoked ad hoc — so `nix fmt .` keeps working uniformly:

- `flake.nix` defines the `formatter` output used by `nix fmt`, and the
  `treefmtEval` block is where every language's formatter is configured.  
- CI runs `nix flake check`, which includes the `formatting` check
  (`treefmtEval.config.build.check self`), so any formatting drift fails the  
  build.

## Testing a running hearthd

When you need to exercise running behaviour — the HTTP API, engine startup, or
config loading at runtime — start hearthd manually rather than relying on a
systemd service.

### Configuration

Any config you write for testing must use the gitignored `.local.toml` suffix
so it is never committed (`*.local.toml` is in `.gitignore`). Do **not** copy
`config.toml.example` wholesale — build a minimal config: a small base that lets
hearthd start, plus only the options for the feature you are adding or testing.
`config.toml.example` is a reference for shape and available keys, not a
starting point.

The default path `/etc/hearthd/config.toml` does not exist in a dev
environment, so always pass your file explicitly.

### Running

Either build a debug binary and run it:

```
cargo build
./target/debug/hearthd --config hearthd.local.toml
```

or run the packaged binary from the flake:

```
nix run . -- --config hearthd.local.toml
```

If your environment can keep processes running in the background (e.g. a
dedicated shell/task that persists), start hearthd in the background rather
than blocking on it, and keep control of its PID:

```
./target/debug/hearthd --config hearthd.local.toml &
PID=$!
echo "hearthd PID: $PID"
```

**Always note the PID at startup.** hearthd binds the HTTP port (default
`127.0.0.1:8565`), so a stray process left behind will make the next run fail
to bind. When you are finished, kill the recorded PID:

```
kill $PID
```

**Never use `pkill` (or any heuristic process matching) to stop hearthd.** It
can kill the wrong hearthd — e.g. one you or another agent started — or kill a
short-lived process the pattern was never meant to target. Only kill the PID
you recorded at startup.

### Querying the API

Use `curl`, which is provided by the devshell, against the HTTP API to verify
the daemon is actually serving. The server listens on `127.0.0.1:8565` by
default.

```
curl http://127.0.0.1:8565/v1/ping
curl http://127.0.0.1:8565/v1/info
curl http://127.0.0.1:8565/v1/state
```

Commands address a node by alias, by the slugged name its integration
gave it, or by its id as `/v1/state` shows them, and use POST:

```
curl -X POST http://127.0.0.1:8565/v1/nodes/<name>/command \
  -H 'content-type: application/json' \
  -d '{"endpoint": 1, "command": {"command": "OnOffOn"}}'
```

Attribute writes name the cluster, the attribute's field and its value as
`/v1/state` shows them:

```
curl -X POST http://127.0.0.1:8565/v1/nodes/<name>/write \
  -H 'content-type: application/json' \
  -d '{"endpoint": 1, "write": {"cluster": "Thermostat", "attribute": "system_mode", "value": "Cool"}}'
```
