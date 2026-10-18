#!/bin/sh
# clack-installer.sh — one-command installer for clack
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/DarrylMathias/clack/main/clack-installer.sh | sh
# Or from releases:
#   curl -fsSL https://github.com/DarrylMathias/clack/releases/latest/download/clack-installer.sh | sh

set -e

REPO="DarrylMathias/clack"
INSTALL_DIR="${HOME}/.local/bin"
TARGET_BIN="${INSTALL_DIR}/clack"

printf "\033[1;36m==>\033[0m Installing clack (mechanical keyboard sounds for your whole machine)...\n"

# 1. Detect OS
OS="$(uname -s)"
case "$OS" in
    Linux)
        OS_TYPE="linux"
        ;;
    Darwin)
        OS_TYPE="macos"
        ;;
    *)
        printf "\033[1;31merror:\033[0m Unsupported operating system: %s. clack supports Linux and macOS.\n" "$OS" >&2
        exit 1
        ;;
esac

# 2. Detect Architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        ARCH_TYPE="x86_64"
        ;;
    arm64|aarch64)
        ARCH_TYPE="aarch64"
        ;;
    *)
        printf "\033[1;31merror:\033[0m Unsupported architecture: %s\n" "$ARCH" >&2
        exit 1
        ;;
esac

# 3. Determine release asset name
if [ "$OS_TYPE" = "macos" ]; then
    ASSET_NAME="clack-macos"
else
    ASSET_NAME="clack-linux-${ARCH_TYPE}"
fi

DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ASSET_NAME}"

# 4. Prepare install directory
mkdir -p "$INSTALL_DIR"

TMP_FILE="$(mktemp "${TMPDIR:-/tmp}/clack.XXXXXX")"
trap 'rm -f "$TMP_FILE"' EXIT INT TERM

printf "Downloading \033[1m%s\033[0m from \033[1m%s\033[0m...\n" "$ASSET_NAME" "$DOWNLOAD_URL"

if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$DOWNLOAD_URL" -o "$TMP_FILE"
elif command -v wget >/dev/null 2>&1; then
    wget -qO "$TMP_FILE" "$DOWNLOAD_URL"
else
    printf "\033[1;31merror:\033[0m Neither curl nor wget found. Please install one to continue.\n" >&2
    exit 1
fi

chmod +x "$TMP_FILE"
mv "$TMP_FILE" "$TARGET_BIN"
printf "\033[1;32m✓\033[0m Installed clack to \033[1m%s\033[0m\n" "$TARGET_BIN"

# 5. Check PATH
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        printf "\n\033[1;33mNote:\033[0m %s is not currently in your PATH.\n" "$INSTALL_DIR"
        SHELL_NAME="$(basename "${SHELL:-sh}")"
        RC_FILE=""
        if [ "$SHELL_NAME" = "zsh" ] && [ -f "${HOME}/.zshrc" ]; then
            RC_FILE="${HOME}/.zshrc"
        elif [ "$SHELL_NAME" = "bash" ]; then
            if [ -f "${HOME}/.bashrc" ]; then
                RC_FILE="${HOME}/.bashrc"
            elif [ -f "${HOME}/.bash_profile" ]; then
                RC_FILE="${HOME}/.bash_profile"
            fi
        fi
        if [ -n "$RC_FILE" ]; then
            printf "Adding PATH to %s...\n" "$RC_FILE"
            printf '\n# Added by clack installer\nexport PATH="%s:$PATH"\n' "$INSTALL_DIR" >> "$RC_FILE"
            export PATH="${INSTALL_DIR}:${PATH}"
        fi
        ;;
esac

# 6. Linux udev rule setup if needed
if [ "$OS_TYPE" = "linux" ]; then
    if [ ! -f "/etc/udev/rules.d/70-clack.rules" ] && [ "$(id -u)" != "0" ]; then
        if ! groups | grep -q "\binput\b"; then
            printf "\n\033[1;36m==>\033[0m Setting up keyboard read access (udev rule)...\n"
            if command -v sudo >/dev/null 2>&1; then
                sudo sh -c 'echo "KERNEL==\"event*\", SUBSYSTEM==\"input\", TAG+=\"uaccess\"" > /etc/udev/rules.d/70-clack.rules && udevadm control --reload && udevadm trigger' || true
            fi
        fi
    fi
fi

# 7. Run onboarding setup
printf "\n\033[1;32m==>\033[0m Launching setup wizard...\n\n"
if [ -t 0 ]; then
    "$TARGET_BIN" setup
else
    # Running inside a piped curl | sh; reconnect tty if available
    if [ -e /dev/tty ]; then
        "$TARGET_BIN" setup < /dev/tty
    else
        "$TARGET_BIN" setup || true
    fi
fi
