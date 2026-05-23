#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
git -C "$repo_root" config core.hooksPath .githooks

template_dir="$repo_root/.git-template"
git config --global init.templateDir "$template_dir"

echo "Configured Git hooks path to .githooks for $repo_root"
echo "Configured global init.templateDir to $template_dir"
