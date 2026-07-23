#!/bin/bash

set -e

TGZ=$(npm pack 2>/dev/null | tail -1)
COMMIT=$(git rev-parse --short HEAD)

NEW="${TGZ%.tgz}-${COMMIT}.tgz"

mv "$TGZ" "$NEW"

echo "$NEW"
