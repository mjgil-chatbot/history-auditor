#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME="history-auditor-builder"
IMAGE_TAG="latest"

docker build -t "$IMAGE_NAME:$IMAGE_TAG" .
