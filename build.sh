#!/usr/bin/env bash
# 编译 release 二进制并打包为 macOS 应用 dist/Imager.app（bundle id: com.liuliu.imager）。
# 图标：assets/Imager.icns；缺失时由 assets/icon.svg 生成（需要 rsvg-convert 与 iconutil）。
#
# 用法：./build.sh [--arch aarch64|x86_64]   默认为当前系统架构
set -euo pipefail

APP_NAME="Imager"
BUNDLE_ID="com.liuliu.imager"
BIN_NAME="imageclient"

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

if [ "$(uname -s)" != "Darwin" ]; then
    echo "错误：.app 打包只能在 macOS 上执行" >&2
    exit 1
fi

usage() {
    echo "用法：$0 [--arch aarch64|x86_64]（默认当前系统架构）" >&2
    exit 1
}

ARCH="$(uname -m)"
while [ $# -gt 0 ]; do
    case "$1" in
        --arch)
            [ $# -ge 2 ] || usage
            ARCH="$2"
            shift 2
            ;;
        --arch=*)
            ARCH="${1#--arch=}"
            shift
            ;;
        -h | --help) usage ;;
        *)
            echo "未知参数：$1" >&2
            usage
            ;;
    esac
done
case "$ARCH" in
    aarch64 | arm64) ARCH="aarch64" ;;
    x86_64 | amd64 | x64) ARCH="x86_64" ;;
    *)
        echo "错误：不支持的架构 ${ARCH}，可选 aarch64 / x86_64" >&2
        exit 1
        ;;
esac
TARGET="${ARCH}-apple-darwin"

if command -v rustup >/dev/null && ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "错误：未安装 Rust 目标 ${TARGET}，请先执行：rustup target add ${TARGET}" >&2
    exit 1
fi

VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
ICNS="assets/${APP_NAME}.icns"

make_icns() {
    command -v rsvg-convert >/dev/null || { echo "错误：缺少 rsvg-convert（brew install librsvg）" >&2; exit 1; }
    local set_dir
    set_dir="$(mktemp -d)/${APP_NAME}.iconset"
    mkdir -p "$set_dir"
    for size in 16 32 128 256 512; do
        rsvg-convert -w "$size" -h "$size" assets/icon.svg -o "$set_dir/icon_${size}x${size}.png"
        rsvg-convert -w $((size * 2)) -h $((size * 2)) assets/icon.svg -o "$set_dir/icon_${size}x${size}@2x.png"
    done
    iconutil -c icns "$set_dir" -o "$ICNS"
    rm -rf "$(dirname "$set_dir")"
    echo "已生成图标 $ICNS"
}

if [ ! -f "$ICNS" ] || [ assets/icon.svg -nt "$ICNS" ]; then
    make_icns
fi

cargo build --release

APP="dist/${APP_NAME}.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/release/${BIN_NAME}" "$APP/Contents/MacOS/${APP_NAME}"
cp "$ICNS" "$APP/Contents/Resources/${APP_NAME}.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key><string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
    <key>CFBundleExecutable</key><string>${APP_NAME}</string>
    <key>CFBundleIconFile</key><string>${APP_NAME}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
EOF

# 本地 ad-hoc 签名，避免 Apple Silicon 上因无签名拒绝运行
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || echo "提示：ad-hoc 签名失败，可忽略或手动签名" >&2

echo "已打包：${ROOT}/${APP}（${BUNDLE_ID} v${VERSION}）"
