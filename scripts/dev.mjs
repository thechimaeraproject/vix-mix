#!/usr/bin/env node
// `dev` — the one entry point for this repository: the dev contract (https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/dev-commands.md).
// The registry is scripts/dev/tasks.mjs; the runner is @chimaeraproject/tooling/dev.
import { fileURLToPath } from "node:url";
import { runDev } from "@chimaeraproject/tooling/dev";

const root = fileURLToPath(new URL("..", import.meta.url));
process.exit(await runDev({ root, registryPath: new URL("./dev/tasks.mjs", import.meta.url), argv: process.argv.slice(2) }));
