#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
if [ "$(uname -sm)" != 'Darwin arm64' ]; then
    echo '仅在 Apple Silicon macOS 上验证过此构建。' >&2
    exit 1
fi

cargo build --release --locked --bin tank-arena
target_dir=$(cargo metadata --locked --no-deps --format-version=1 | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')

dist=${DIST_DIR:-"$PWD/target/dist"}
mkdir -p "$dist"
stage=$(mktemp -d "$dist/.stage.XXXXXX")
trap 'rm -rf "$stage"' EXIT
bundle="$stage/Tank Arena.app/Contents"
mkdir -p "$bundle/MacOS" "$bundle/Resources" "$stage/Game.iconset"
cp "$target_dir/release/tank-arena" "$bundle/MacOS/"

# 签名前排除截图附带的 Finder 缩略图和资源分支。
# 只打包随附的初始模型，不能混入开发者在 bots/ 里添加的私人坦克。
mkdir -p "$bundle/Resources/bots"
cp -X assets/bots/example.luau "$bundle/Resources/bots/"
python3 scripts/notices.py "$bundle/Resources/ThirdParty"

sips -s format png assets/ui/icon.svg --out "$stage/icon.png" >/dev/null
for size in 16 32 128 256 512; do
  for scale in 1 2; do
    suffix=''; [ "$scale" = 1 ] || suffix='@2x'
    sips -z $((size*scale)) $((size*scale)) "$stage/icon.png" --out "$stage/Game.iconset/icon_${size}x${size}${suffix}.png" >/dev/null
  done
done
iconutil -c icns "$stage/Game.iconset" -o "$bundle/Resources/Game.icns"

cat > "$bundle/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>local.tankarena.game</string>
<key>CFBundleName</key><string>Tank Arena</string>
<key>CFBundleExecutable</key><string>tank-arena</string>
<key>CFBundleIconFile</key><string>Game.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>1.0.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

codesign --force --sign - "${bundle%/Contents}"
codesign --verify --strict "${bundle%/Contents}"
# 先在暂存目录中生成完整压缩包；发布时不会读到上一次打包留下的 ZIP 内容。
archive=Tank-Arena-macOS-arm64.zip
(cd "$stage" && ditto -c -k --norsrc --noextattr --keepParent 'Tank Arena.app' "$archive" && shasum -a 256 "$archive" > SHA256SUMS)

rm -rf "$dist/Tank Arena.app"
mv "${bundle%/Contents}" "$stage/$archive" "$stage/SHA256SUMS" "$dist/"
printf '%s\n' "$dist/Tank Arena.app"
