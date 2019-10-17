#!/usr/bin/env bash

set -u

trap "exit 1" TERM
export TOP_PID=$$

if [ -f github_token.inc ]; then
  source github_token.inc
fi

: "${APPNAME:=wifi-captive}"
: "${GITHUB_TOKEN:=}"

main() {
    need_cmd curl
    need_cmd jq
    need_cmd mkdir
    need_cmd grep
    need_cmd cat
    need_cmd head
    need_cmd sed
    need_cmd basename
    need_cmd file

    local _build_nums
    local _tagname
    local _title
    local _body
    local _payload
    local _response
    local _upload_url


    if [ -z "$GITHUB_TOKEN" ]; then
        say "No github token set!"
        exit 0
    fi

    ensure mkdir -p target/releases

    # Build linux x86_64 variant
    ensure cargo build --release
    ensure cp target/release/wifi-captive target/releases/linux_$(uname -m)

    # Strip files
    local _size
    for _file in target/releases/*; do
        ensure strip $_file
        _size = $(du -h $_file | cut -f 1)
        say "Binary $_file ($_size)"
    done

    _build_nums=$(ensure git rev-parse HEAD)

    _tagname=$(cat CHANGELOG.md |grep -Po "v([0-9]{1,}\.)+[0-9]{1,}" -m 1)

    # Grab latest release notes from the Changelog
    _title=$(
        ensure cat CHANGELOG.md \
        | ensure grep -Pzo '##.*\n\n\K\X*?(?=\n##|$)' \
        | ensure tr '\0' '\n' \
        | ensure head -n1
    )

    _body=$(
        ensure cat CHANGELOG.md \
        | ensure grep -Pzo '##.*\n\n\K\X*?(?=\n##|$)' \
        | ensure tr '\0' '\n' \
        | ensure sed '1d'
    )

    _payload=$(
        jq --null-input \
            --arg tag "$_tagname" \
            --arg name "$_title" \
            --arg body "$_body" \
            '{ tag_name: $tag, name: $name, body: $body, draft: false }'
    )

    _response=$(
        curl -sSL -X POST "https://api.github.com/repos/openhab-nodes/ohx-os/releases" \
            -H "Accept: application/vnd.github.v3+json" \
            -H "Authorization: token $GITHUB_TOKEN" \
            -H "Content-Type: application/json" \
            -d "$_payload"
    )

    _upload_url=$(
        echo "$_response" \
        | ensure jq -r .upload_url \
        | ensure sed -e "s/{?name,label}//"
    )

    for _file in target/releases/*; do
        local _basename
        local _mimetype
        local _response
        local _state

        _basename=$(ensure basename "$_file")
        _mimetype=$(ensure file --mime-type -b "$_file") 

        say "Uploading $_basename..."
        
        _response=$(
            curl -sSL -X POST "$_upload_url?name=$_basename" \
                -H "Accept: application/vnd.github.manifold-preview" \
                -H "Authorization: token $GITHUB_TOKEN" \
                -H "Content-Type: $_mimetype" \
                --data-binary "@$_file"
        )

        _state=$(ensure jq -r '.state' <<< "$_response")

        if [ "$_state" != "uploaded" ]; then
            err "Artifact not uploaded: $_basename"
        fi
    done
}

say() {
    printf '\33[1m%s:\33[0m %s\n' "$APPNAME" "$1"
}

err() {
    printf '\33[1;31m%s:\33[0m %s\n' "$APPNAME" "$1" >&2
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
