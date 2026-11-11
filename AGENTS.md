- always write unit tests for features and functions that can break. Always use Haiku 5.5 for writing simple unit tests,
  only use better models for complex functions with many edge cases.

## Before saying a task is done

Run `npm run build`, `npm test`, `cargo check --all-targets` and `cargo test` (in `src-tauri`) and make sure they pass.
Never report work as finished without running them; say which ones you did not run.

## Tauri bridge

`#[tauri::command]` functions and `invoke("name")` calls are linked only by a string, so no compiler checks them.
When adding, renaming or removing a command, change every side: the definition, the registration in
`src-tauri/src/lib.rs`, the wrapper in `src/shared/lib/audio.ts` and the permissions in `src-tauri/capabilities/`.

## Stores

When changing a Zustand store action, search for `\.actionName\b`, not only for imports: it is also called as
`getState().actionName(...)` and `library.actionName(...)`.

## Structure

- `src/shared/` holds only code used by three or more areas; code for one feature or page lives in it.
- Tests sit next to the file they test.
- Imports use the `@/` alias with the file extension, e.g. `@/shared/lib/ui/time.ts`.

## No dead code

No new `cargo check` warnings, no unused exports (`npx knip`), no commented-out code.

## Git

- Work on a branch; `master` takes changes only through a PR with passing CI.
- Commit messages follow the existing history: `feat(scope): …`, `fix(scope): …`, `chore: …`.
