#!/bin/sh
# leaks.sh FILE…: for each file, how many times the invite link appears in
# full (raw), URL-encoded (encoded), and masked. Prints counts only, never the link.
for f in "$@"; do printf "%s raw=%s encoded=%s masked=%s\n" "$(basename "$f")" "$(grep -oE '/accept/tk_[0-9a-f]{6}' "$f" | wc -l | tr -d ' ')" "$(grep -oE '%2Faccept%2Ftk_[0-9a-f]{6}' "$f" | wc -l | tr -d ' ')" "$(grep -o '•••invite_link•••' "$f" | wc -l | tr -d ' ')"; done
