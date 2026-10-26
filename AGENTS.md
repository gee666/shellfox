Keep the app design minimal and slightly old-school, with compact controls and no unnecessary text.

Every feature you build or work on - make sure it is fully supported across all the supported OS: Mac, Windows, Linux, Android, iOS, headless clients, unless it is eplicitely agreed with the user.


# Code quality bar

- Remove dead code, unused imports, and unused dependencies. Wire intended functionality into the application instead of leaving disconnected code.
- Keep module responsibilities and dependency boundaries clear. Follow the existing Electron, React, TypeScript, and C#/.NET conventions. Preserve the separation between the main process, preload bridge, renderer, shared contracts, and native helpers. Keep privileged operations out of the renderer and expose only narrow, validated IPC APIs.
- Avoid duplicated logic and unnecessary abstractions. Prefer simple, consistent solutions over one-off patterns.
- Fix root causes, not symptoms. Avoid brittle workarounds. Make deployment-specific values and operational settings configurable, and never hardcode secrets.
- Keep TypeScript strict and use types that model valid application states. Validate external data at runtime, including IPC messages, persisted data, and native helper responses. Use the project's Zod schemas where appropriate. Do not bypass type safety with `any`, unsafe assertions, or suppressed type errors. Apply the same care to C# nullability and native protocol contracts.
- Keep functions and classes focused. Split files by responsibility before they exceed 350 lines. Exceed this limit only when splitting would clearly hurt readability or cohesion, and explain why.
- Remove stale or irrelevant comments and docs. Keep useful ones accurate, short, clear, and human-friendly. Explain non-obvious decisions rather than restating code. Apply the unslop and write-prompts skills when available.
- Add or update tests for changed behavior. Run `pnpm typecheck` and relevant tests before considering work complete. Use `pnpm test:unit` for Vitest coverage, `pnpm test:native` for native changes, and the relevant Playwright, Windows, terminal, storage, or packaging checks for affected integrations. Run lint checks if configured. If a required check cannot run on the current platform or lacks prerequisites, report it rather than treating it as passed.
- Make the solution as simple as possible without losing reliability or quality. Ask whether there is a simpler, more natural solution.
