#!/bin/bash
set -eu

cargo run --bin dune -- "$@" ~/Games/cryo-dune/pc-3.7-cd/DUNE.DAT
