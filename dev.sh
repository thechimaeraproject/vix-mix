#!/bin/sh
# One entry point for this repository: the dev contract (https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/dev-commands.md).
# Dispatch lives in scripts/dev.mjs; `exec` hands it this process, exit code included.
exec "${KIT_NODE:-node}" "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/scripts/dev.mjs" "$@"
