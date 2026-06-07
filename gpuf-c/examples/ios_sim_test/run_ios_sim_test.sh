#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR"
PROJECT_NAME="GPUFIosSimTest"
PROJECT_PATH="$PROJECT_DIR/$PROJECT_NAME.xcodeproj"
SCHEME_NAME="$PROJECT_NAME"
CONFIGURATION="Debug"

if ! command -v xcodebuild >/dev/null 2>&1; then
  echo "❌ xcodebuild not found"
  exit 1
fi

# Pick a booted simulator if available; otherwise boot a reasonable default.
BOOTED_JSON="$(xcrun simctl list -j devices booted 2>/dev/null || true)"
BOOTED_UDID="$(BOOTED_JSON="$BOOTED_JSON" /usr/bin/python3 - <<'PY'
import json,sys
import os
data=os.environ.get('BOOTED_JSON', '').strip()
if not data:
  print('')
  sys.exit(0)
try:
  j=json.loads(data)
except Exception:
  print('')
  sys.exit(0)
for rt,devs in j.get('devices',{}).items():
  for d in devs:
    if d.get('state')=='Booted':
      print(d.get('udid',''))
      sys.exit(0)
print('')
PY
)"

if [ -z "$BOOTED_UDID" ]; then
  # Fallback for Xcode versions where JSON output is unavailable.
  BOOTED_UDID="$(xcrun simctl list devices booted 2>/dev/null | sed -nE 's/.*\(([0-9A-Fa-f-]{36})\) \(Booted\).*/\1/p' | head -n 1)"
fi

if [ -z "$BOOTED_UDID" ]; then
  # Try iPhone 15 first, fallback to any available iPhone.
  DEVICE_NAME="iPhone 15"
  AVAILABLE_JSON="$(xcrun simctl list -j devices available 2>/dev/null || true)"
  UDID="$(AVAILABLE_JSON="$AVAILABLE_JSON" /usr/bin/python3 - <<'PY'
import json,sys
import os
data=os.environ.get('AVAILABLE_JSON', '').strip()
if not data:
  print('')
  sys.exit(0)
try:
  j=json.loads(data)
except Exception:
  print('')
  sys.exit(0)
candidates=[]
for rt,devs in j.get('devices',{}).items():
  for d in devs:
    if not d.get('isAvailable',False):
      continue
    name=d.get('name','')
    udid=d.get('udid','')
    if name=='iPhone 15':
      print(udid)
      sys.exit(0)
    if name.startswith('iPhone'):
      candidates.append(udid)
if candidates:
  print(candidates[0])
else:
  print('')
PY
)"

  if [ -z "$UDID" ]; then
    # Fallback plain-text parsing.
    UDID="$(xcrun simctl list devices available 2>/dev/null | sed -nE 's/.*iPhone[^\(]*\(([0-9A-Fa-f-]{36})\).*/\1/p' | head -n 1)"
  fi

  if [ -z "$UDID" ]; then
    echo "❌ No available iOS simulators found"
    exit 1
  fi

  echo "📱 Booting simulator: $UDID"
  xcrun simctl boot "$UDID" >/dev/null 2>&1 || true
  xcrun simctl bootstatus "$UDID" -b
  BOOTED_UDID="$UDID"
fi

DESTINATION="platform=iOS Simulator,id=$BOOTED_UDID"

echo "🔨 Building + running on simulator ($BOOTED_UDID)..."

# Build
xcodebuild \
  -project "$PROJECT_PATH" \
  -scheme "$SCHEME_NAME" \
  -configuration "$CONFIGURATION" \
  -destination "$DESTINATION" \
  -derivedDataPath "$PROJECT_DIR/DerivedData" \
  build

APP_PATH="$PROJECT_DIR/DerivedData/Build/Products/$CONFIGURATION-iphonesimulator/$PROJECT_NAME.app"
if [ ! -d "$APP_PATH" ]; then
  echo "❌ App not found at: $APP_PATH"
  exit 1
fi

BUNDLE_ID="com.gpuf.iossimtest"

echo "📦 Installing app to simulator..."
xcrun simctl install "$BOOTED_UDID" "$APP_PATH"

echo "🚀 Launching app... (check Console output in Xcode or: Console.app -> Simulator)"
xcrun simctl launch "$BOOTED_UDID" "$BUNDLE_ID"
