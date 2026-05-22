# @fallow-cli/fallow-node

Native Node.js bindings for fallow’s main analyses.

## Install

```bash
npm install @fallow-cli/fallow-node   # or: pnpm/yarn/bun add @fallow-cli/fallow-node
```

## API

- `detectDeadCode(options?)`
- `detectCircularDependencies(options?)`
- `detectBoundaryViolations(options?)`
- `detectDuplication(options?)`
- `computeComplexity(options?)`
- `computeHealth(options?)`

All functions are async and return the same JSON-shaped report contracts that the CLI emits for `--format json`.

Enum-like option values use lowercase CLI-style strings such as `"mild"`, `"cyclomatic"`, `"handle"`, and `"low"`.

Rejected promises throw a `FallowNodeError` with:

- `message`
- `exitCode`
- optional `code`
- optional `help`
- optional `context`
