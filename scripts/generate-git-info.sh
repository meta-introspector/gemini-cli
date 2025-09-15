#!/usr/bin/env bash
set -euo pipefail
COMMIT_HASH=$1
mkdir -p packages/generated
echo "export const GIT_COMMIT_INFO = { commitHash: '$COMMIT_HASH' };" > packages/generated/git-commit.ts
