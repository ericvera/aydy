#!/bin/sh
# AYDY hook script v2
dir="$HOME/Library/Application Support/app.aydy/status"
mkdir -p "$dir" 2>/dev/null || exit 0
tmp="$dir/.hook.$$.tmp"
cat > "$tmp" 2>/dev/null || { rm -f "$tmp" 2>/dev/null; exit 0; }
field() { sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\([A-Za-z0-9_-]\{1,128\}\)\".*/\1/p" "$tmp" 2>/dev/null | head -n 1; }
id=$(field session_id)
if [ -z "$id" ]; then rm -f "$tmp" 2>/dev/null; exit 0; fi
event=$(field hook_event_name)
if [ "$event" = SessionEnd ]; then rm -f "$tmp" "$dir/$id.json" 2>/dev/null; exit 0; fi
if [ "$event" = Notification ]; then
  case "$(field notification_type)" in
    agent_completed|auth_success) rm -f "$tmp" 2>/dev/null; exit 0 ;;
  esac
fi
mv -f "$tmp" "$dir/$id.json" 2>/dev/null
rm -f "$tmp" 2>/dev/null
exit 0
