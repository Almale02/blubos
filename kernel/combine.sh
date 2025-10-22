#!/bin/bash
git ls-files -z '*.rs' | while IFS= read -r -d '' f; do printf "// ===== FILE: %s
                                       =====\n\n" "$f"; cat "$f"; printf "\n\n"; done > all_rust.rs
