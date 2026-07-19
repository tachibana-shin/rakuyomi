#!/bin/bash
# GOOGLE_TRANSLATE.sh - Drop-in replacement for AI_TRANSLATE.sh
# Translate gettext .po using DeepL first, with LibreTranslate and Google fallback
# Requires: curl, jq

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
TRANSLATION_DELAY="${TRANSLATION_DELAY:-0}"
MAX_JOBS="${RAKUYOMI_TRANSLATE_JOBS:-8}"
TRANSLATION_PROVIDER="${TRANSLATION_PROVIDER:-deepl}"
TRANSLATION_API_URL="${TRANSLATION_API_URL:-https://translate.argosopentech.com/translate}"
DEEPL_API_URL="${DEEPL_API_URL:-https://api-free.deepl.com/v2/translate}"
DEEPL_API_KEY="${DEEPL_API_KEY:-}"
DEEPL_TARGET_LANG="${DEEPL_TARGET_LANG:-${LANG_CODE}}"

gtranslate_deepl() {
    local TEXT="$1"

    if [[ -z "$DEEPL_API_KEY" ]]; then
        echo ""
        return
    fi

    local TARGET_LANG
    TARGET_LANG="${DEEPL_TARGET_LANG//_/-}"
    TARGET_LANG="${TARGET_LANG// /-}"

    local JSON
    JSON=$(curl -s -X POST "$DEEPL_API_URL" \
        -H "Authorization: DeepL-Auth-Key $DEEPL_API_KEY" \
        -H "Content-Type: application/x-www-form-urlencoded" \
        --data-urlencode "text=$TEXT" \
        --data-urlencode "source_lang=EN" \
        --data-urlencode "target_lang=$TARGET_LANG" 2>/dev/null)

    local TRANS
    TRANS=$(echo "$JSON" | jq -r '.translations[0].text // empty' 2>/dev/null)

    if [[ -z "$TRANS" || "$TRANS" == "null" ]]; then
        echo ""
    else
        echo "$TRANS"
    fi
}

gtranslate_google() {
    local TEXT="$1"
    local ESCAPED
    ESCAPED=$(echo -n "$TEXT" | jq -sRr @uri)

    local JSON
    JSON=$(curl -s -A "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36" \
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl=${LANG_CODE}&dt=t&q=${ESCAPED}" 2>/dev/null)

    local TRANS
    TRANS=$(echo "$JSON" | jq -r '.[0][0][0]' 2>/dev/null)

    if [[ -z "$TRANS" || "$TRANS" == "null" ]]; then
        echo ""
    else
        echo "$TRANS"
    fi
}

gtranslate_libretranslate() {
    local TEXT="$1"

    local PAYLOAD
    PAYLOAD=$(jq -nc --arg q "$TEXT" --arg source "en" --arg target "$LANG_CODE" --arg format "text" \
        '{q:$q, source:$source, target:$target, format:$format}' 2>/dev/null)

    local JSON
    JSON=$(curl -s -X POST "$TRANSLATION_API_URL" \
        -H "Content-Type: application/json" \
        --data-binary "$PAYLOAD" 2>/dev/null)

    local TRANS
    TRANS=$(echo "$JSON" | jq -r '.translatedText // empty' 2>/dev/null)

    if [[ -z "$TRANS" || "$TRANS" == "null" ]]; then
        echo ""
    else
        echo "$TRANS"
    fi
}

gtranslate() {
    local TEXT="$1"

    if [[ -z "$TEXT" ]]; then
        echo ""
        return
    fi

    local TRANS=""
    case "$TRANSLATION_PROVIDER" in
        google)
            TRANS=$(gtranslate_google "$TEXT")
            ;;
        libretranslate)
            TRANS=$(gtranslate_libretranslate "$TEXT")
            if [[ -z "$TRANS" ]]; then
                TRANS=$(gtranslate_google "$TEXT")
            fi
            ;;
        deepl)
            TRANS=$(gtranslate_deepl "$TEXT")
            if [[ -z "$TRANS" ]]; then
                TRANS=$(gtranslate_libretranslate "$TEXT")
            fi
            if [[ -z "$TRANS" ]]; then
                TRANS=$(gtranslate_google "$TEXT")
            fi
            ;;
        *)
            TRANS=$(gtranslate_deepl "$TEXT")
            if [[ -z "$TRANS" ]]; then
                TRANS=$(gtranslate_libretranslate "$TEXT")
            fi
            if [[ -z "$TRANS" ]]; then
                TRANS=$(gtranslate_google "$TEXT")
            fi
            ;;
    esac

    if [[ -z "$TRANS" ]]; then
        echo "$TEXT"
    else
        echo "$TRANS"
    fi

    if [[ -n "$TRANSLATION_DELAY" ]]; then
        sleep "$TRANSLATION_DELAY"
    fi
}

translate_indexed() {
    local INDEX="$1"
    local TEXT="$2"
    local OUTPUT_FILE="$3"

    local TRANS
    TRANS=$(gtranslate "$TEXT")
    printf '%s\t%s\n' "$INDEX" "$TRANS" > "$OUTPUT_FILE"
}

# ----------------------------------------
# Process PO file
# ----------------------------------------
echo "Translating $INPUTFILE → $OUTPUTFILE (provider: ${TRANSLATION_PROVIDER})"
rm -f "$OUTPUTFILE"
touch "$OUTPUTFILE"

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

ENTRY_LIST="$TMPDIR/entries.tsv"
CURRENT_MSGID=""
READING_MSGID=0
ENTRY_INDEX=0

while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" =~ ^msgid\ \"(.*)\"$ ]]; then
        CURRENT_MSGID="${BASH_REMATCH[1]}"
        READING_MSGID=1
        continue
    fi

    if [[ "$line" =~ ^msgstr\ \"(.*)\"$ ]]; then
        if [[ "$READING_MSGID" -eq 1 ]]; then
            printf '%s\t%s\n' "$ENTRY_INDEX" "$CURRENT_MSGID" >> "$ENTRY_LIST"
            ENTRY_INDEX=$((ENTRY_INDEX + 1))
            READING_MSGID=0
        fi
        continue
    fi
done < "$INPUTFILE"

if [[ -s "$ENTRY_LIST" ]]; then
    export -f gtranslate translate_indexed

    while IFS=$'\t' read -r index msgid; do
        while [[ $(jobs -pr | wc -l) -ge "$MAX_JOBS" ]]; do
            wait -n || true
        done

        translate_indexed "$index" "$msgid" "$TMPDIR/translation.$index" &
    done < "$ENTRY_LIST"

    wait
fi

# Build a translation map from the completed worker output files.
declare -A TRANSLATIONS=()
for translation_file in "$TMPDIR"/translation.*; do
    [[ -f "$translation_file" ]] || continue
    IFS=$'\t' read -r index translated < "$translation_file"
    TRANSLATIONS["$index"]="$translated"
done

CURRENT_MSGID=""
READING_MSGID=0
ENTRY_INDEX=0

while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" =~ ^msgid\ \"(.*)\"$ ]]; then
        CURRENT_MSGID="${BASH_REMATCH[1]}"
        READING_MSGID=1
        echo "$line" >> "$OUTPUTFILE"
        continue
    fi

    if [[ "$line" =~ ^msgstr\ \"(.*)\"$ ]]; then
        if [[ "$READING_MSGID" -eq 1 ]]; then
            translated_value="${TRANSLATIONS[$ENTRY_INDEX]:-$CURRENT_MSGID}"
            echo "Translating: $CURRENT_MSGID"
            echo "msgstr \"$translated_value\"" >> "$OUTPUTFILE"
            ENTRY_INDEX=$((ENTRY_INDEX + 1))
            READING_MSGID=0
        else
            echo "$line" >> "$OUTPUTFILE"
        fi
        continue
    fi

    echo "$line" >> "$OUTPUTFILE"
done < "$INPUTFILE"

echo "Done. Output saved: $OUTPUTFILE"
