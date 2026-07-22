#!/bin/bash
set -eu

cargo run --bin dune -- "$@" --skip-intro ~/Games/cryo-dune/pc-3.7-cd/DUNE.DAT
