#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
make clean
make
make test
PATH=/usr/bin:/bin:/usr/sbin:/sbin make dist
tests/test_dist.sh
echo 'Full release suite passed.'
