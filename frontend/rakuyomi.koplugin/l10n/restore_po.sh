#!/bin/bash

POT_FILE="templates/koreader.pot"

if [ ! -f "$POT_FILE" ]; then
    echo "ERROR: not found $POT_FILE"
    exit 1
fi

find . -type f -name "koreader.mo" -print0 | while IFS= read -r -d '' mofile; do
    pofile="${mofile%.mo}.po"
    echo "Converting $mofile -> $pofile"
    
    tmp_po="$(mktemp)"
    msgunfmt "$mofile" > "$tmp_po"

    tmp_merged="$(mktemp)"
    msgmerge --quiet "$tmp_po" "$POT_FILE" > "$tmp_merged"

    mv "$tmp_merged" "$pofile"

    rm "$tmp_po"
done
