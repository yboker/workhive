# TypeScript packages

Each `packages/<name>/` is a deep module: its root files are the package's entry points and its subfolders are private implementation. Import a package only through one of its root files; `lib/` is the conventional implementation folder and `tests/` holds co-located tests and fixtures. A package may expose several small entry points—do not create a giant barrel that re-exports an entire subtree through `index.ts`.

Copy this shape when adding a package:

```text
packages/<name>/
├── index.ts
├── lib/
│   └── impl.ts
└── tests/
    └── <name>.test.ts
```

Run the check with:

```bash
npm run lint:boundaries
```

## Rules

**Entry-point boundary.** Code outside a package may import its root entry points, but not files in its subfolders. A package may expose more than one root entry point when that keeps its interface small and focused.

**Intra-package freedom.** Files inside one package may import that package's implementation freely. This keeps the seam at the package interface instead of forcing internal code through unnecessary wrappers.

**Tests through entry points.** Tests may import package entry points and their own `tests/` fixtures, but they exercise implementation through the same interface as callers and may not import any package's private subfolder. The `tests/` folder itself is private to tests.

**No cycles.** Keep the package dependency graph acyclic. `dependency-cruiser` reports cycles as errors rather than allowing a shallow or tangled interface to spread across packages.
