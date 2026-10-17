#!/bin/sh
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/target/PigmentLab.app"
iconset="$root/target/PigmentLab.iconset"

cargo build --release --manifest-path "$root/Cargo.toml" -p pigmentlab

rm -rf "$app" "$iconset"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$root/crates/app/ui/icons/icon.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) "$root/crates/app/ui/icons/icon.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/PigmentLab.icns"
rm -rf "$iconset"
cp "$root/target/release/pigmentlab" "$app/Contents/MacOS/PigmentLab"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>PigmentLab</string>
    <key>CFBundleDisplayName</key><string>PigmentLab</string>
    <key>CFBundleIdentifier</key><string>com.lukaszkujawa.pigmentlab</string>
    <key>CFBundleVersion</key><string>0.1.0</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>PigmentLab</string>
    <key>CFBundleIconFile</key><string>PigmentLab</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
</dict>
</plist>
PLIST
echo "built $app"
