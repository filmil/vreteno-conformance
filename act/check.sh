#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Passes when the result file named by $1 has the status $2: PASS for
# a test the device should pass, FAIL for a known failure. A known
# failure that starts to pass fails this check, so that the entry in
# the list of known failures is removed when the bug is fixed.
# Shell builtins only: the test needs no tool from the machine.
set -u
want="${2:-PASS}"
IFS= read -r first < "$1" || { echo "cannot read $1"; exit 1; }
case " ${first} " in
  *" status=${want} "*) exit 0 ;;
esac
echo "expected status=${want}; the result is:"
while IFS= read -r line; do echo "${line}"; done < "$1"
exit 1
