#!/bin/bash -e

# License: MIT
# David Graeff <david.graeff@web.de> - 2019

# Creates a Github release for the last entry of the CHANGELOG file.
#
# Uploads all (already generated) release binaries of this Rust crate
# and all local docker images that are tagged with docker.pkg.github.com/$GITHUB_ID/$CRATE_NAME.

readonly GITHUB_ID=$(git remote -v|cut -f2|cut -d':' -f2|cut -d' ' -f1|cut -d'.' -f1|head -n1)
readonly RELEASE_API_URL="https://api.github.com/repos/$GITHUB_ID/releases"
readonly UPLOAD_API_URL="https://uploads.github.com/repos/$GITHUB_ID/releases"
readonly METADATA=$(cargo metadata --format-version 1 | jq -r '.workspace_members[]' | tail -n1)
readonly PACKAGE_NAME=$(echo $METADATA | cut -d' ' -f1)
readonly PACKAGE_VERSION=$(echo $METADATA | cut -d' ' -f2)

if [ -f github_token.inc ]; then
  source ./github_token.inc
fi

: "${GITHUB_TOKEN:=}"

# Attach binary to Github release. Remove existing one if necessary.
# The filename pattern is: crateName-arch-tagname-sha256short.xz, eg: "wifi_captive-x86_64-v1.0.0-aceac12.xz"
deploy() {
	local _file="$1"
	local ARCH="$2"
	local LATEST_RELEASE="$3"

	[ ! -f $_file ] && err "Skip non existing $_file"

	local PACKAGE_NAME_ESC=$(echo $PACKAGE_NAME | sed -e 's/-/_/g')
	local rel_id=$(echo $LATEST_RELEASE | jq -r '.id')
	local assets=$(echo $LATEST_RELEASE | jq -r '.assets[] | with_entries(select(.key == "id" or .key == "name")) | flatten | .[]')
	local sha=$(cat $_file| sha256sum -bz|cut -c -6)
	local current_tag=$(tagname)

	[ -z "$rel_id" ] && err "Failed to determine release id: $rel_id"

	local is_done="0"

	# Do we need to deploy?
	IFS=' '
	while read -r asset_id; do
        [ -z "$asset_id" ] && break
		read -r asset_name
		local asset_arch=$(echo $asset_name|cut -d '-' -f2)
		local asset_tagname=$(echo $asset_name|cut -d '-' -f3)
		local asset_sha=$(echo $asset_name|cut -d '-' -f4|cut -d "." -f1)
		local delete_url="$RELEASE_API_URL/assets/$asset_id"

		if [ "$asset_tagname" = "$current_tag" ] && [ "$asset_arch" = "$ARCH" ]; then
			if [ "$asset_sha" != "$sha" ]; then
				say "Checksums not equal. Reupload for $ARCH ($asset_sha vs $sha). Old ID: $asset_id"
				curl -sSL -X DELETE "$delete_url" \
					-H "Accept: application/vnd.github.v3+json" \
					-H "Authorization: token $GITHUB_TOKEN" \
					-H "Content-Type: application/json"
			else
				say "Identical checksums. No need to redeploy for $ARCH"
				is_done="1"
			fi
			break
		fi
	done <<< $assets

	[ "$is_done" = "1" ] && return

	local mimetype=$(file --mime-type -b "$_file")
	local basename="$PACKAGE_NAME_ESC-$ARCH-$current_tag-$sha"
	local label="$PACKAGE_NAME ($PACKAGE_VERSION)"
	#label=$(echo $label | curl -Gso /dev/null -w %{url_effective} --data-urlencode @- "" | cut -c 3-)
	local upload_url="$UPLOAD_API_URL/$rel_id/assets?name=$basename"
	say "Uploading $basename..."

	local _response=$(
		curl -SL -X POST \
			-H "Accept: application/vnd.github.manifold-preview" \
			-H "Authorization: token $GITHUB_TOKEN" \
			-H "Content-Type: $mimetype" \
			--data-binary "@$_file" "$upload_url"
	)

	local _state=$(jq -r '.state' <<< "$_response")

	if [ "$_state" != "uploaded" ]; then
		err "Artifact not uploaded: $basename: $_response"
	else
		say "Uploaded!"
	fi
}

tagname() {
	cat CHANGELOG.md |grep -Po "v([0-9]{1,}\.)+[0-9]{1,}" -m 1
}

build_num() {
	git rev-parse HEAD
}

next_rel_title() {
	cat CHANGELOG.md | grep -Pzo '##.*\n\n\K\X*?(?=\n##)' | tr '\0' '\n' | head -n1
}

next_rel_body() {
	cat CHANGELOG.md | grep -Pzo '##.*\n\n\K\X*?(?=\n##)' | tr '\0' '\n' | sed '1d'
}

# Create Github release if not yet existing
make_release() {
	latest_release=$(curl -sSL "${RELEASE_API_URL}/latest" \
		-H "Accept: application/vnd.github.v3+json" \
		-H "Authorization: token $GITHUB_TOKEN" \
		-H "Content-Type: application/json")

	if [ "$(echo $latest_release | jq -r '.message')" = "Not Found" ]; then
		say "Latest release not found"
		need_new="1"
	else
		latest_ver=$(echo $latest_release | jq -r '.name')
		say "Latest release found: $latest_ver"
		[ "$latest_ver" != "$(next_rel_title)" ] && need_new="1"
	fi

	if [ ! -z "$need_new" ]; then
		say "Create new release: $(next_rel_title)"
		local _payload=$(
			jq --null-input \
				--arg tag "$(tagname)" \
				--arg name "$(next_rel_title)" \
				--arg body "$(next_rel_body)" \
				'{ tag_name: $tag, name: $name, body: $body, draft: false }'
		)

		curl -sSL -X POST "$RELEASE_API_URL" \
			-H "Accept: application/vnd.github.v3+json" \
			-H "Authorization: token $GITHUB_TOKEN" \
			-H "Content-Type: application/json" \
			-d "$_payload" > /dev/null
	fi
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        err "need '$1' (command not found) $2"
    fi
}

say() {
	local color=$( tput setaf 2 )
	local normal=$( tput sgr0 )
	echo "${color}$1${normal}"
}

err() {
	local color=$( tput setaf 1 )
	local normal=$( tput sgr0 )
	echo "${color}$1${normal}" >&2
	exit 1
}

need_cmd curl
need_cmd jq
need_cmd mkdir
need_cmd grep
need_cmd cat
need_cmd head
need_cmd sed
need_cmd basename
need_cmd file

if [ -z "$GITHUB_TOKEN" ]; then
	say 'No github token set! Go to your Github account -> Developer Settings -> Tokens and create a new token.'
	say 'Store the new token in a file github_token.inc file as GITHUB_TOKEN=your_token \n GITHUB_ACCOUNT=your_account'
	exit 0
fi

make_release || err "Failed to create a Github release"
targets=$(find target -mindepth 1 -maxdepth 1 -type d -not -name "debug" -and -not -name "release" -and -not -name "libdbus" -and -not -name "package")

unset IFS
for target in $targets; do
    arch=$(echo $target|cut -d'/' -f2|cut -d'-' -f1)
    binary="$target/release/$PACKAGE_NAME"
    deploy "$binary" "$arch" "$latest_release"
done

if [ -z "$GITHUB_USERNAME" ]; then
	say 'No github username set! Please add GITHUB_USERNAME=your_github_username to your github_token.inc file'
	exit 0
fi

docker="docker"
if command -v "podman" > /dev/null 2>&1; then
    docker="podman"
fi

if command -v $docker > /dev/null 2>&1; then
    unset IFS
    basetag="docker.pkg.github.com/$GITHUB_ID/${PACKAGE_NAME}:${PACKAGE_VERSION}"
    additional_tags=""
    for target in $targets; do
        arch=$(echo $target|cut -d'/' -f2|cut -d'-' -f1)
        tag="${basetag}_$arch"
        $docker build -f "target/Dockerfile_${arch}" -t $tag
        additional_tags="$additional_tags $tag"
    done
    if [ "$docker" = "podman" ] && command -v buildah > /dev/null 2>&1; then
      # shellcheck disable=SC2086
      sha=$(buildah manifest create "${basetag}" $additional_tags)
      buildah manifest push --all --creds=$GITHUB_USERNAME:$GITHUB_TOKEN "${basetag}" "docker://${basetag}"
      buildah rmi $sha
    fi
fi
