#!/bin/sh
# Materialize the parse5 stubs into node_modules. bun's `file:` overrides are
# only reliably materialized on clean installs: when an ancestor node_modules
# (e.g. the repo-tooling one at the workspace root) already satisfies "parse5",
# bun skips the override and the bundler falls back to the real parse5 there,
# bloating libs.js by ~115 KB.
set -e
for stub in parse5 parse5-htmlparser2-tree-adapter; do
  dest="node_modules/$stub"
  rm -rf "$dest"
  cp -r "stubs/$stub" "$dest"
done
