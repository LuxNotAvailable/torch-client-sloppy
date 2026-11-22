#!/usr/bin/env bash
# Android game benchmark: builds and installs com.torch.bench (beside the
# real game, never over it), joins the local bench server through
# `adb reverse`, stands at spawn for $SECS seconds and writes the game's
# log to $OUT/bench.log. The server: target/bench-server (official jar,
# offline, creative, fixed seed, RCON on localhost; gamerules advance_time,
# advance_weather and spawn_* false), started separately.
#
#   tools/bench-apk/run.sh [KEY=VALUE ...]   # extra env.txt lines
#
# SECS (default 120), RD (render distance, default 32), NO_BUILD=1,
# PACK (shader pack name, default none; "builtin" for the built-in one),
# OPTS (extra options.json fields, e.g. '"shaderRenderScale": 70').
set -eu
here=$(cd "$(dirname "$0")" && pwd)
win() { if command -v cygpath > /dev/null; then cygpath -m "$1"; else echo "$1"; fi; }
export MSYS_NO_PATHCONV=1
P=com.torch.bench
D=/sdcard/Android/data/$P/files
OUT=${OUT:-.}
SECS=${SECS:-120}
RD=${RD:-32}
PACK=${PACK:-}
[ "$PACK" = builtin ] && PACK="Torch Vibrant (built-in)"
if [ "${NO_BUILD:-0}" != 1 ]; then
    sdk=${ANDROID_HOME:-$LOCALAPPDATA/Android/Sdk}
    export ANDROID_HOME=$sdk
    export ANDROID_NDK_ROOT=${ANDROID_NDK_ROOT:-$(ls -d "$sdk"/ndk/*/ | sort -V | tail -1)}
    export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(win "$here/../../target/bench-apk")}
    export CARGO_APK_RELEASE_KEYSTORE=${CARGO_APK_RELEASE_KEYSTORE:-$(win "$HOME/.android/debug.keystore")}
    export CARGO_APK_RELEASE_KEYSTORE_PASSWORD=${CARGO_APK_RELEASE_KEYSTORE_PASSWORD:-android}
    (cd "$here" && cargo apk build --release)
    adb install -r "$CARGO_TARGET_DIR/release/apk/bench-apk.apk" > /dev/null
fi
tmp=$(mktemp -d)
{
    echo "MC_CLIENT_CONFIG_DIR=/storage/emulated/0/Android/data/$P/files"
    echo "MC_NATIVE_GPU_TIMING=1"
    for kv in "$@"; do echo "$kv"; done
} > "$tmp/env.txt"
printf 'localhost:25565\nBench\n' > "$tmp/launch.txt"
# Uncapped: the mobile default caps at 30 fps.
printf '{"renderDistance": %s, "maxFps": 260%s}\n' "$RD" "${OPTS:+, $OPTS}" > "$tmp/options.json"
printf '%s' "$PACK" > "$tmp/shaderpack.txt"
# The game rewrites options.json; reset the render distance every run.
adb push "$(win "$tmp/env.txt")" $D/env.txt > /dev/null
adb push "$(win "$tmp/launch.txt")" $D/launch.txt > /dev/null
adb push "$(win "$tmp/options.json")" $D/options.json > /dev/null
adb push "$(win "$tmp/shaderpack.txt")" $D/shaderpack.txt > /dev/null
rm -rf "$tmp"
adb reverse tcp:25565 tcp:25565 > /dev/null
# Fixed conditions: the world's gamerules stop time, weather and mob
# spawns (set once); every run starts at noon in clear weather.
python -I "$(win "$here/rcon.py")" "time set noon" "weather clear" > /dev/null
adb shell input keyevent KEYCODE_WAKEUP
adb shell am force-stop $P
adb logcat -c
adb shell am start -n $P/android.app.NativeActivity > /dev/null
sleep "$SECS"
adb logcat -d -s torch-client:* RustStdoutStderr:* > "$(win "$OUT")/bench.log"
adb shell am force-stop $P
grep -c "native renderer" "$(win "$OUT")/bench.log" || true
