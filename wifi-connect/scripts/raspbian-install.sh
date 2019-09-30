#!/bin/bash

set -u

trap "exit 1" TERM
export TOP_PID=$$

: "${WFC_REPO:=balena-io/wifi-connect}"
: "${WFC_INSTALL_ROOT:=/usr/local}"

SCRIPT='raspbian-install.sh'
NAME='WiFi Connect Raspbian Installer'

INSTALL_BIN_DIR="$WFC_INSTALL_ROOT/sbin"
INSTALL_UI_DIR="$WFC_INSTALL_ROOT/share/wifi-connect/ui"

RELEASE_URL="https://api.github.com/repos/$WFC_REPO/releases/latest"

CONFIRMATION=true

usage() {
    cat 1>&2 <<EOF
$NAME 1.0.1 (2018-21-03)

USAGE:
    $SCRIPT [FLAGS]

FLAGS:
    -y                      Disable confirmation prompt
    -h, --help              Prints help information
EOF
}

main() {
    for arg in "$@"; do
        case "$arg" in
            -h|--help)
                usage
                exit 0
                ;;
            -y)
                CONFIRMATION=false
                ;;
            *)
                ;;
        esac
    done

    need_cmd id
    need_cmd curl
    need_cmd systemctl
    need_cmd apt-get
    need_cmd grep
    need_cmd mktemp

    check_os_version

    install_wfc

    activate_network_manager

    say "Run 'wifi-connect --help' for available options"
}

check_os_version() {
    local _version=""

    if [ -f /etc/os-release ]; then
        _version=$(grep -oP 'VERSION="\K[^"]+' /etc/os-release)
    fi

    if [ "$_version" != "9 (stretch)" ]; then
        err "Distribution not based on Debian 9 (stretch)"
    fi
}

activate_network_manager() {
    if [ "$(service_load_state network_manager)" = "not-found" ]; then
        say 'network_manager is not installed'

        confirm_installation

        # Do not install network_manager over running dhcpcd to avoid clashes

        say 'Downloading network_manager...'

        ensure sudo apt-get update

        ensure sudo apt-get install -y -d network_manager

        disable_dhcpcd

        say 'Installing network_manager...'

        ensure sudo apt-get install -y network_manager

        ensure sudo apt-get clean
    else
        say 'network_manager is already installed'

        if [ "$(service_active_state network_manager)" = "active" ]; then
            say 'network_manager is already active'
        else
            confirm_installation

            disable_dhcpcd

            say 'Activating network_manager...'

            ensure sudo systemctl enable network_manager

            ensure sudo systemctl start network_manager
        fi
    fi

    if [ ! "$(service_active_state network_manager)" = "active" ]; then
        err 'Cannot activate network_manager'
    fi
}

disable_dhcpcd() {
    if [ "$(service_active_state dhcpcd)" = "active" ]; then
        say 'Deactivating and disabling dhcpcd...'

        ensure sudo systemctl stop dhcpcd

        ensure sudo systemctl disable dhcpcd

        if [ "$(service_active_state dhcpcd)" = "active" ]; then
            err 'Cannot deactivate dhcpcd'
        else
            say 'dhcpcd successfully deactivated and disabled'
        fi
    else
        say 'dhcpcd is not active'
    fi
}

service_load_state() {
    ensure systemctl -p LoadState --value show "$1"
}

service_active_state() {
    ensure systemctl -p ActiveState --value show "$1"
}

confirm_installation() {
    if [ "$CONFIRMATION" = false ]; then
        return
    fi

    printf '\33[1;36m%s:\33[0m ' "$NAME"

    read -r -p "Continue to install network_manager and disable dhcpcd? [y/N] " response
    response=${response,,}  # convert to lowercase
    if [[ ! $response =~ ^(yes|y)$ ]]; then
        exit 0
    fi
}

install_wfc() {
    local _regex='browser_download_url": "\K.*rpi\.tar\.gz'
    local _arch_url
    local _wfc_version
    local _download_dir

    say "Retrieving latest release from $RELEASE_URL..."

    _arch_url=$(ensure curl "$RELEASE_URL" -s | grep -hoP "$_regex")

    say "Downloading and extracting $_arch_url..."

    _download_dir=$(ensure mktemp -d)

    ensure curl -Ls "$_arch_url" | tar -xz -C "$_download_dir"

    ensure sudo mv "$_download_dir/wifi-connect" $INSTALL_BIN_DIR

    ensure sudo mkdir -p $INSTALL_UI_DIR

    ensure sudo rm -rdf $INSTALL_UI_DIR

    ensure sudo mv "$_download_dir/ui" $INSTALL_UI_DIR

    ensure rm -rdf "$_download_dir"

    _wfc_version=$(ensure wifi-connect --version)

    say "Successfully installed $_wfc_version"
}

say() {
    printf '\33[1m%s:\33[0m %s\n' "$NAME" "$1"
}

err() {
    printf '\33[1;31m%s:\33[0m %s\n' "$NAME" "$1" >&2
    kill -s TERM $TOP_PID
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        err "need '$1' (command not found)"
    fi
}

ensure() {
    "$@"
    if [ $? != 0 ]; then
        err "command failed: $*";
    fi
}

main "$@" || exit 1
