#!/usr/bin/env bash
# Deploys the build to a host.
set -euo pipefail

TARGET=${1:-staging}
export RETRIES=3
readonly LOG_DIR=/var/log/deploy
HOST=web1 PORT=22
hosts[0]=web1
declare -a extra=(web2 web3)
local_only() { :; }

log() {
  local message=$1
  echo "[$(date +%T)] $message" >&2
}

function build {
  inner() {
    count=0
  }
  make -C "$ROOT" all
}

function upload() {
  for host in "$@"; do
    scp out.tar "$host:"
  done
}

if [[ -n $TARGET ]]; then
  log "deploying to $TARGET"
  build
  upload "$TARGET"
fi
