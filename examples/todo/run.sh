#!/usr/bin/env bash
set -e

PORT=${1:-8080}
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

echo "=========================================================="
echo "⚡ Starting RHP TODO Example Application"
echo "📂 App file: ${SCRIPT_DIR}/app.rhp"
echo "🌐 Port:     ${PORT}"
echo "=========================================================="

cd "${ROOT_DIR}"
cargo run -- serve "${SCRIPT_DIR}/app.rhp" --port "${PORT}"
