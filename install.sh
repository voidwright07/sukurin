#!/bin/sh
# Installs or upgrades Sukurin on macOS and Linux x86_64.
#   curl -fsSL https://raw.githubusercontent.com/voidwright07/shattr/main/install.sh | sh
# SUKURIN_VERSION=v0.2.0 pins a release; SUKURIN_DOWNLOAD_BASE points at a mirror of the releases URL.
set -eu

say() { printf 'sukurin: %s\n' "$*"; }
die() {
	printf 'sukurin: error: %s\n' "$*" >&2
	exit 1
}

base=${SUKURIN_DOWNLOAD_BASE:-https://github.com/voidwright07/shattr/releases}
if [ -n "${SUKURIN_VERSION:-}" ]; then
	download=$base/download/$SUKURIN_VERSION
else
	download=$base/latest/download
fi

os=$(uname -s)
arch=$(uname -m)
case $os in
Darwin) asset=sukurin-macos-universal.tar.gz ;;
Linux)
	case $arch in
	x86_64 | amd64) asset=sukurin-linux-x86_64.tar.gz ;;
	*) die "Linux on $arch is not supported yet; releases are built for x86_64 only." ;;
	esac
	;;
MINGW* | MSYS* | CYGWIN*)
	die "this installer is for macOS and Linux. On Windows run: powershell -c \"irm https://raw.githubusercontent.com/voidwright07/shattr/main/install.ps1 | iex\""
	;;
*) die "$os is not supported; releases are built for macOS, Linux x86_64 and Windows." ;;
esac

fetch() {
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL -o "$2" "$1"
	elif command -v wget >/dev/null 2>&1; then
		wget -q -O "$2" "$1"
	else
		die "curl or wget is required"
	fi
}

stop_running() {
	pkill -x sukurin 2>/dev/null || return 0
	say "stopping the running Sukurin"
	i=0
	while pgrep -x sukurin >/dev/null 2>&1 && [ $i -lt 10 ]; do
		sleep 1
		i=$((i + 1))
	done
	pkill -9 -x sukurin 2>/dev/null || true
}

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t sukurin)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT TERM

say "downloading $download/$asset"
fetch "$download/$asset" "$tmp/$asset" || die "download failed: $download/$asset"
tar -xzf "$tmp/$asset" -C "$tmp" || die "could not unpack $asset"

install_macos() {
	if [ -d /Applications/Sukurin.app ]; then
		dest=/Applications/Sukurin.app
		[ -w /Applications ] || die "/Applications is not writable; remove /Applications/Sukurin.app or rerun as a user who can write there"
	else
		mkdir -p "$HOME/Applications"
		dest=$HOME/Applications/Sukurin.app
	fi
	stop_running
	say "installing to $dest"
	rm -rf "$dest"
	cp -R "$tmp/Sukurin.app" "$dest"
	xattr -dr com.apple.quarantine "$dest" 2>/dev/null || true
	open "$dest"
	say "Sukurin is running in the menu bar."
}

# Maps a shared library to the packages that provide it: apt dnf pacman.
packages_for() {
	case $1 in
	libgtk-3.so* | libgdk-3.so*) echo "libgtk-3-0 gtk3 gtk3" ;;
	libatk-1.0.so*) echo "libatk1.0-0 atk at-spi2-core" ;;
	libcairo-gobject.so*) echo "libcairo-gobject2 cairo-gobject cairo" ;;
	libcairo.so*) echo "libcairo2 cairo cairo" ;;
	libpangocairo-1.0.so*) echo "libpangocairo-1.0-0 pango pango" ;;
	libpango-1.0.so*) echo "libpango-1.0-0 pango pango" ;;
	libgdk_pixbuf-2.0.so*) echo "libgdk-pixbuf-2.0-0 gdk-pixbuf2 gdk-pixbuf2" ;;
	libglib-2.0.so* | libgobject-2.0.so* | libgio-2.0.so*) echo "libglib2.0-0 glib2 glib2" ;;
	libxcb.so*) echo "libxcb1 libxcb libxcb" ;;
	libxkbcommon-x11.so*) echo "libxkbcommon-x11-0 libxkbcommon-x11 libxkbcommon-x11" ;;
	libxkbcommon.so*) echo "libxkbcommon0 libxkbcommon libxkbcommon" ;;
	libfontconfig.so*) echo "libfontconfig1 fontconfig fontconfig" ;;
	libEGL.so*) echo "libegl1 libglvnd-egl libglvnd" ;;
	libgbm.so*) echo "libgbm1 mesa-libgbm mesa" ;;
	libpipewire-0.3.so*) echo "libpipewire-0.3-0 pipewire-libs libpipewire" ;;
	libayatana-appindicator3.so*) echo "libayatana-appindicator3-1 libayatana-appindicator-gtk3 libayatana-appindicator" ;;
	libvulkan.so*) echo "libvulkan1 vulkan-loader vulkan-icd-loader" ;;
	esac
}

ldconfig_cache() {
	for l in ldconfig /sbin/ldconfig /usr/sbin/ldconfig; do
		if command -v "$l" >/dev/null 2>&1; then
			"$l" -p 2>/dev/null
			return
		fi
	done
}

append_unique() {
	case " $1 " in
	*" $2 "*) echo "$1" ;;
	*) echo "${1:+$1 }$2" ;;
	esac
}

check_libraries() {
	missing=$(ldd "$1" 2>/dev/null | awk '/not found/ { s = s (s ? " " : "") $1 } END { print s }')
	# The tray icon and the GPU renderer are loaded with dlopen, so ldd cannot see them.
	cache=$(ldconfig_cache)
	if [ -n "$cache" ]; then
		printf '%s\n' "$cache" | grep -Eq 'lib(ayatana-)?appindicator3\.so' ||
			missing=$(append_unique "$missing" libayatana-appindicator3.so.1)
		printf '%s\n' "$cache" | grep -q 'libvulkan\.so\.1' ||
			missing=$(append_unique "$missing" libvulkan.so.1)
	fi
	[ -n "$missing" ] || return 0

	apt=''
	dnf=''
	pacman=''
	for lib in $missing; do
		# shellcheck disable=SC2046
		set -- $(packages_for "$lib")
		[ $# -eq 3 ] || continue
		apt=$(append_unique "$apt" "$1")
		dnf=$(append_unique "$dnf" "$2")
		pacman=$(append_unique "$pacman" "$3")
	done
	say "missing system libraries: $missing"
	if [ -n "$apt" ]; then
		say "install them with one of:"
		printf '  Debian/Ubuntu: sudo apt install %s\n' "$apt"
		printf '  Fedora:        sudo dnf install %s\n' "$dnf"
		printf '  Arch:          sudo pacman -S %s\n' "$pacman"
	fi
}

install_linux() {
	bindir=$HOME/.local/bin
	icondir=$HOME/.local/share/icons/hicolor/512x512/apps
	appdir=$HOME/.local/share/applications
	stop_running
	say "installing to $bindir/sukurin"
	mkdir -p "$bindir" "$icondir" "$appdir"
	install -m 755 "$tmp/sukurin" "$bindir/sukurin"
	install -m 644 "$tmp/sukurin.png" "$icondir/sukurin.png"
	sed "s|^Exec=.*|Exec=\"$bindir/sukurin\"|" "$tmp/sukurin.desktop" >"$appdir/sukurin.desktop"
	update-desktop-database "$appdir" >/dev/null 2>&1 || true
	gtk-update-icon-cache -q -t "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true

	check_libraries "$bindir/sukurin"
	case ":$PATH:" in
	*":$bindir:"*) ;;
	*) say "warning: $bindir is not on your PATH; add it to run sukurin from a terminal" ;;
	esac

	if [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
		nohup "$bindir/sukurin" >/dev/null 2>&1 &
		say "Sukurin is running in the system tray."
	else
		say "installed. Start Sukurin from your app menu; it lives in the system tray."
	fi
}

case $os in
Darwin) install_macos ;;
Linux) install_linux ;;
esac
