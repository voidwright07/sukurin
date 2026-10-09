#!/usr/bin/env bash
# Usage: scripts/package.sh <platform-id> [version]
# Turns the release binaries under target/<triple>/release into dist/<asset>.
set -euo pipefail

usage() {
	echo "usage: $0 <windows-x86_64|macos-universal|linux-x86_64> [version]" >&2
	exit 2
}

[ $# -ge 1 ] || usage
platform=$1
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

version=${2:-}
version=${version#v}
if [ -z "$version" ]; then
	version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
fi

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p dist

case $platform in
windows-x86_64)
	asset=sukurin-windows-x86_64.zip
	cp target/x86_64-pc-windows-msvc/release/sukurin.exe "$stage/"
	rm -f "dist/$asset"
	# 7-Zip ships on the windows-latest image; Git Bash has no zip.
	(cd "$stage" && 7z a -tzip -bso0 -bsp0 "$root/dist/$asset" sukurin.exe)
	;;
macos-universal)
	asset=sukurin-macos-universal.tar.gz
	app=$stage/Sukurin.app
	mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
	lipo -create -output "$app/Contents/MacOS/sukurin" \
		target/aarch64-apple-darwin/release/sukurin \
		target/x86_64-apple-darwin/release/sukurin
	cp assets/icon/sukurin.icns "$app/Contents/Resources/sukurin.icns"
	# Bundle versions must be numeric, so a tag like 1.2.0-rc.1 becomes 1.2.0.
	sed "s/@VERSION@/${version%%[-+]*}/g" packaging/macos/Info.plist >"$app/Contents/Info.plist"
	codesign --force --deep -s - "$app"
	rm -f "dist/$asset"
	# COPYFILE_DISABLE keeps bsdtar from adding AppleDouble ._ files.
	COPYFILE_DISABLE=1 tar -C "$stage" -czf "dist/$asset" Sukurin.app
	;;
linux-x86_64)
	asset=sukurin-linux-x86_64.tar.gz
	install -m 755 target/x86_64-unknown-linux-gnu/release/sukurin "$stage/sukurin"
	# The release profile keeps line tables, which on Linux live inside the binary (371 MB -> 70 MB).
	strip --strip-debug "$stage/sukurin"
	cp packaging/linux/sukurin.desktop "$stage/sukurin.desktop"
	cp assets/icon/sukurin.png "$stage/sukurin.png"
	rm -f "dist/$asset"
	tar -C "$stage" -czf "dist/$asset" sukurin sukurin.desktop sukurin.png
	;;
*)
	usage
	;;
esac

echo "dist/$asset ($version)"
