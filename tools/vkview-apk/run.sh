#!/usr/bin/env bash
# Builds and installs the vkview APK, runs it on the connected Android
# device with the given vkview arguments, prints its output from logcat and
# pulls any PNGs it wrote into $OUT (default: the current folder).
#
#   tools/vkview-apk/run.sh --terrain . --frames 600 --sheet s.png
#
# The app runs in its external files folder
# (/sdcard/Android/data/com.torch.vkview/files): push a terrain dump's
# sections.bin and atlas.png straight into it (not a subfolder adb makes,
# which the app can't read) and pass `--terrain .`. NO_BUILD=1 skips the
# build.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
# Windows-native tools (cargo-apk, apksigner, adb) want Windows paths.
win() { if command -v cygpath > /dev/null; then cygpath -m "$1"; else echo "$1"; fi; }
export MSYS_NO_PATHCONV=1
D=/sdcard/Android/data/com.torch.vkview/files
OUT=${OUT:-.}
if [ "${NO_BUILD:-0}" != 1 ]; then
    sdk=${ANDROID_HOME:-$LOCALAPPDATA/Android/Sdk}
    export ANDROID_HOME=$sdk
    export ANDROID_NDK_ROOT=${ANDROID_NDK_ROOT:-$(ls -d "$sdk"/ndk/*/ | sort -V | tail -1)}
    export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(win "$here/../../target/vkview-apk")}
    export CARGO_APK_RELEASE_KEYSTORE=${CARGO_APK_RELEASE_KEYSTORE:-$(win "$HOME/.android/debug.keystore")}
    export CARGO_APK_RELEASE_KEYSTORE_PASSWORD=${CARGO_APK_RELEASE_KEYSTORE_PASSWORD:-android}
    (cd "$here" && cargo apk build --release)
    adb install -r "$CARGO_TARGET_DIR/release/apk/vkview-apk.apk" > /dev/null
fi
args=$(mktemp)
printf '%s\n' "$*" > "$args"
# Windows' adb needs a Windows path for files from Git Bash.
local_args=$args
command -v cygpath > /dev/null && local_args=$(cygpath -w "$args")
adb push "$local_args" $D/args.txt > /dev/null
rm -f "$args"
adb shell "find $D -maxdepth 1 -name \"*.png\" ! -name atlas.png -delete"
adb shell input keyevent KEYCODE_WAKEUP
adb shell am force-stop com.torch.vkview
adb logcat -c
adb shell am start -n com.torch.vkview/android.app.NativeActivity > /dev/null
for _ in $(seq 900); do
    sleep 1
    if adb logcat -d -s torch-client:* | grep -q "vkview: exit"; then break; fi
done
adb logcat -d -s torch-client:* | grep -o "vkview.*"
for f in $(adb shell "ls $D/*.png 2>/dev/null" | grep -v atlas.png); do
    adb pull "$f" "$(command -v cygpath > /dev/null && cygpath -w "$OUT" || echo "$OUT")" > /dev/null
done
