#!/usr/bin/env bash
# Run after `tauri android init`. Adds what Xent needs on Android:
#  - network permissions
#  - a Wi-Fi MulticastLock (without it Android silently drops the UDP multicast
#    packets that Xent uses to find other devices)
set -euo pipefail
ROOT="src-tauri/gen/android/app/src/main"
MANIFEST="$ROOT/AndroidManifest.xml"
ACTIVITY="$ROOT/java/com/xent/app/MainActivity.kt"

[ -f "$MANIFEST" ] || { echo "Run 'npx @tauri-apps/cli android init' first"; exit 1; }

add_perm () {
  grep -q "android.permission.$1" "$MANIFEST" || \
    sed -i "s#<application#<uses-permission android:name=\"android.permission.$1\" />\n    <application#" "$MANIFEST"
}
add_perm INTERNET
add_perm ACCESS_NETWORK_STATE
add_perm ACCESS_WIFI_STATE
add_perm CHANGE_WIFI_MULTICAST_STATE
add_perm FOREGROUND_SERVICE
add_perm POST_NOTIFICATIONS

mkdir -p "$(dirname "$ACTIVITY")"
cat > "$ACTIVITY" <<'KOTLIN'
package com.xent.app

import android.content.Context
import android.net.wifi.WifiManager
import android.os.Bundle

class MainActivity : TauriActivity() {
    private var multicastLock: WifiManager.MulticastLock? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        multicastLock = wifi.createMulticastLock("xent-discovery").apply {
            setReferenceCounted(false)
            acquire()
        }
    }

    override fun onDestroy() {
        multicastLock?.let { if (it.isHeld) it.release() }
        super.onDestroy()
    }
}
KOTLIN
echo "Android project patched."
