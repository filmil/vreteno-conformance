#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Bazel's workspace status: the commits the report names.
#
# STABLE_GIT_COMMIT is this repository's commit. STABLE_TXHDL_COMMIT is
# the TxHDL commit the core is built from: $TXHDL_COMMIT when the run
# overrides the module (the daily workflow does), otherwise the commit
# MODULE.bazel pins.
set -u
echo "STABLE_GIT_COMMIT $(git rev-parse HEAD 2>/dev/null || echo unknown)"
if [ -n "${TXHDL_COMMIT:-}" ]; then
  echo "STABLE_TXHDL_COMMIT ${TXHDL_COMMIT}"
else
  pin=""
  in_txhdl=0
  while IFS= read -r line; do
    case "${line}" in
      *'module_name = "txhdl"'*) in_txhdl=1 ;;
      *'commit = "'*)
        if [ "${in_txhdl}" = 1 ]; then
          pin="${line#*\"}"
          pin="${pin%%\"*}"
          in_txhdl=0
        fi
        ;;
    esac
  done < MODULE.bazel
  echo "STABLE_TXHDL_COMMIT ${pin:-unknown}"
fi
