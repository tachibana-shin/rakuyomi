#!/bin/bash
# GOOGLE_TRANSLATE.sh - Drop-in replacement for AI_TRANSLATE.sh
# Translate gettext .po using Google Translate free API (client=gtx)
# Requires: curl, jq, python3 (for URL encode)

set -euo pipefail

LANG_CODE="$1"
TEMPLATE_FILE="templates/koreader.pot"

TRANSLATED_FILE="$LANG_CODE/koreader.po"
UNTRANSLATED_FILE="$LANG_CODE/untranslated.po"
UPDATED_TRANSLATED_FILE="$LANG_CODE/updated_translated.po"

mkdir -p "$LANG_CODE"

INPUTFILE=
OUTPUTFILE=

# ----------------------------------------
# Determine mode (NEW LANGUAGE or UPDATE)
# ----------------------------------------
if [[ ! -f "$TRANSLATED_FILE" && ! -f "$UNTRANSLATED_FILE" ]]; then
    # New language — use template
    cp "$TEMPLATE_FILE" "$UNTRANSLATED_FILE"
    INPUTFILE="$UNTRANSLATED_FILE"
    OUTPUTFILE="$TRANSLATED_FILE"

elif [[ -f "$TRANSLATED_FILE" && -f "$UNTRANSLATED_FILE" ]]; then
    # Update mode
    INPUTFILE="$UNTRANSLATED_FILE"
    OUTPUTFILE="$UPDATED_TRANSLATED_FILE"

elif [[ -f "$TRANSLATED_FILE" && -f "$UPDATED_TRANSLATED_FILE" ]]; then
    echo "Already translated: $LANG_CODE"
    exit 0

else
    echo "Error: invalid translation state for $LANG_CODE"
    exit 1
fi

# ----------------------------------------
# Google Translate helper
# ----------------------------------------
gtranslate() {
    TEXT="$1"

    # URL-encode using python
    ESCAPED=$(python3 - <<EOF
import urllib.parse
print(urllib.parse.quote("""$TEXT"""))
EOF
)

    JSON=$(curl -s \
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl=$LANG_CODE&dt=t&q=$ESCAPED")

    echo "$JSON" | jq -r '.[0][0][0]'
}

# ----------------------------------------
# Process PO file
# ----------------------------------------
echo "Translating $INPUTFILE → $OUTPUTFILE (Google Translate)"
rm -f "$OUTPUTFILE"
touch "$OUTPUTFILE"

CURRENT_MSGID=""
READING_MSGID=0

while IFS= read -r line || [[ -n "$line" ]]; do

    # Detect msgid "....."
    if [[ "$line" =~ ^msgid\ \"(.*)\"$ ]]; then
        CURRENT_MSGID="${BASH_REMATCH[1]}"
        READING_MSGID=1
        echo "$line" >> "$OUTPUTFILE"
        continue
    fi

    # If it's a msgstr — we replace it by translated version
    if [[ "$line" =~ ^msgstr\ \"(.*)\"$ ]]; then
        if [[ "$READING_MSGID" -eq 1 ]]; then
            TRANS=$(gtranslate "$CURRENT_MSGID")
            echo "msgstr \"$TRANS\"" >> "$OUTPUTFILE"
            READING_MSGID=0
        else
            # Safety fallback
            echo "$line" >> "$OUTPUTFILE"
        fi
        continue
    fi

    # Copy other lines unchanged (comments, blank lines…)
    echo "$line" >> "$OUTPUTFILE"

done < "$INPUTFILE"

echo "Done. Output saved: $OUTPUTFILE"
