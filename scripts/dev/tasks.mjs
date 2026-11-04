// The task registry: what `dev` can run in this repository. DATA — nothing here runs, imports
// a runner or touches the filesystem. A command's exe is a token: {node} {npm} {python} {uv},
// or `{program}` for anything else the runner should find on PATH — so `{cargo}` is the
// toolchain's cargo (a bare `cargo` is not a token and the runner reports it unavailable).
// The contract: https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/dev-commands.md
//
// vix-mix is a Rust workspace; every gate but the docs is a cargo command. `check` is the push
// gate (kit.repo.json gates.push) and what CI runs on both legs: fmt, clippy, test and
// check-docs, in that order, stopping at the first failure.

/** Sections print in this order. */
export const SECTIONS = ["Gate", "Cargo", "Docs"];

/** Retired ids answer with a pointer to their replacement. @type {Map<string, string>} */
export const RETIRED = new Map();

export const TASKS = [
  {
    id: "check",
    section: "Gate",
    title: "the push gate: fmt, clippy, test, check-docs",
    needs: ["fmt", "clippy", "test", "check-docs"],
    commands: [],
  },
  {
    id: "fmt",
    section: "Cargo",
    title: "cargo fmt --all --check",
    commands: [{ exe: "{cargo}", args: ["fmt", "--all", "--check"] }],
  },
  {
    id: "clippy",
    section: "Cargo",
    title: "cargo clippy --workspace --all-targets -- -D warnings",
    commands: [{ exe: "{cargo}", args: ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"] }],
  },
  {
    id: "test",
    section: "Cargo",
    title: "cargo test --workspace",
    commands: [{ exe: "{cargo}", args: ["test", "--workspace"] }],
  },
  {
    id: "check-docs",
    section: "Docs",
    title: "the documentation standard (kit-check-docs)",
    commands: [{ exe: "{node}", args: ["node_modules/@chimaeraproject/tooling/bin/kit-check-docs.mjs", "--root", "."] }],
  },
];
