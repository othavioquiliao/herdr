# herdr

Terminal workspace manager for AI coding agents. Rust + ratatui.

## Principles

- **State is separated from runtime.** `AppState` is pure data, testable without PTYs or async. `PaneState` is separate from `PaneRuntime`. Workspace logic doesn't need real terminals.
- **Render is pure.** `compute_view()` handles geometry and mutations. `render()` takes `&AppState` and only draws. Never mutate state during render.
- **No god objects.** If a module is doing too many things, split it. `app/` is already split into state, actions, and input. Keep it that way.
- **Platform code is isolated.** OS-specific behavior lives in `src/platform/`. Core modules don't have `#[cfg(target_os)]`.
- **Detection is decoupled.** The detector reads a screen snapshot, never touches the parser or viewport state.

## Testing

```bash
just check              # formatting + unit tests
just test               # unit tests
just test-integration   # LLM-based integration tests (needs pi + tmux)
just test-all           # check + integration tests
just clean-tests        # kill orphaned test tmux sessions
```

Default flow: run `just check` before committing.

`just test-all` includes an experimental LLM-driven end-to-end test pass. Do not run it unless Can asks for it explicitly.

Unit tests live next to the code (`#[cfg(test)] mod tests`). If you add behavior to `AppState` or `Workspace`, it should be testable with `AppState::test_new()` and `Workspace::test_new()` — no PTYs.

Integration tests are markdown specs in `tests/integration/specs/`. A pi agent executes them against herdr in an isolated tmux server. See `tests/integration/system.md` for the test agent prompt.

## Conventions

- Conventional commits, lowercase, no emojis.
- Rust: no `unwrap()` in production code. `tracing` for logging. `#[allow]` only with a comment explaining why.
- Don't bypass checks. If tests fail, fix them before committing.
- Don't add dependencies without a reason. Check if the existing deps cover it first.

## Releases

Before cutting a release, draft the upcoming notes under `## Unreleased` in `CHANGELOG.md`. The release script promotes that section into the versioned entry.

Default release flow:

```bash
just check
just release 0.x.y
```

`just release 0.x.y` prepares the changelog entry, bumps `Cargo.toml`, runs tests, commits, tags, and pushes. GitHub Actions builds the binaries after the tag is pushed.
