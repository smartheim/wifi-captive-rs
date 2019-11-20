#!/bin/bash

DEST=$(realpath "target/libdbus")

download() {
    local url="$1"
    local file="$(basename $url)"
    local strip="$3"
    : "${strip:=1}"
    trap "rm $file" EXIT
    ensure wget --no-check-certificate -q --show-progress "$url" -O "$file"
    mkdir -p "$2"
    if [ "$4" = "zip" ]; then
        ensure unzip -q "$file" -d "$2"
    else
        ensure tar xaf "$file" --strip-components=$strip -C "$2"
    fi
    ensure rm "$file"
    trap "" EXIT
}

prerequirements() {
    need_cmd wget
    need_cmd tput
    need_cmd tar

    mkdir -p $DEST

    if [ ! -d $DEST/x86_64 ]; then
        say "Download x86_64 musl compiler toolchain"
        download https://musl.cc/x86_64-linux-musl-native.tgz $DEST/x86_64 2
    fi

    if [ ! -d $DEST/armv7l ]; then
        say "Download armv7l musl cross compiler toolchain"
        download https://musl.cc/armv7l-linux-musleabihf-cross.tgz $DEST/armv7l 2
    fi

    if [ ! -d $DEST/aarch64 ]; then
        say "Download aarch64 musl cross compiler toolchain"
        download https://musl.cc/aarch64-linux-musl-cross.tgz $DEST/aarch64 2
    fi

    local libexecinfo="$DEST/libexecinfo"
    if [ ! -d $libexecinfo ]; then
        say "Download libexecinfo"
        download https://github.com/resslinux/libexecinfo/archive/master.zip $libexecinfo 1 zip
        ensure mv $libexecinfo/libexecinfo-master/* $libexecinfo
        ensure rm -rf $libexecinfo/libexecinfo-master
    fi

    local dbusdir="$DEST/libdbus"
    if [ ! -d $dbusdir ]; then
        say "Download libdbus"
        download https://gitlab.freedesktop.org/dbus/dbus/-/archive/master/dbus-master.tar.gz $DEST/libdbus_all
        ensure mv $DEST/libdbus_all/dbus $dbusdir
        ensure rm -rf $DEST/libdbus_all
        ensure mkdir -p $dbusdir/dbus

        # Move all header files to dbus directory
        ensure mv $dbusdir/*.h $dbusdir/dbus

        # Remove all non-c files
        ensure find $dbusdir -maxdepth 1 -type f ! -name "*.c" -exec rm {} \;

        pushd $dbusdir > /dev/null
        mkdir -p unix; mkdir -p win; mkdir -p wince
        ensure find . -maxdepth 1 -type f -name '*unix*' -or -name '*userdb*' -or -name '*epoll*' -exec mv {} unix/{} \;
        ensure find . -maxdepth 1 -type f -name '*wince*' -exec mv {} wince/{} \;
        ensure find . -maxdepth 1 -type f -name '*win*' -exec mv {} win/{} \;
        popd > /dev/null
    fi

    ensure cp scripts/config.h $dbusdir/dbus
    ensure cp scripts/dbus-arch-deps.h $dbusdir/dbus
}

compile() {
    # Inputs
    local ARCH="$1"
    local BIN_PREFIX="$2"
    local ADD_COMP="$3"
    local libexecinfo="$DEST/libexecinfo"
    local dbusdir="$DEST/libdbus"

    say "Build libdbus for $ARCH"

    local LINK="$DEST/$ARCH/lib"
    # variant is something like: aarch64-linux-musl
    local compiler_variant=$(ls $LINK/gcc)
    # The path depends on the compiler variant and version, eg: ./lib/gcc/aarch64-linux-musl/9.1.1/include
    local compiler_inc_path="$LINK/gcc/$compiler_variant/$(ls $LINK/gcc/$compiler_variant)/include"
    local INCLUDES="-I$DEST/$ARCH/include -I$compiler_inc_path"
    local CC=$(realpath "$DEST/$ARCH/bin/${BIN_PREFIX}gcc")
    local AR=$(realpath "$DEST/$ARCH/bin/${BIN_PREFIX}ar")
    local dbusbuilddir="$DEST/libdbus_build"

    if [ ! -f "$DEST/libdbus_static_$ARCH.a" ]; then
        mkdir -p $dbusbuilddir
        rm -rf $dbusbuilddir/* > /dev/null

        # Compile
        pushd $dbusbuilddir > /dev/null
        [ "$ARCH" = "x86_64" ] && $CC -c $(ls $libexecinfo/*.c) "-l$LINK" $INCLUDES "-I$libexecinfo"  -std=gnu11
        $CC -c $(ls $dbusdir/*.c) $(ls $dbusdir/unix/*.c) $(ls $dbusdir/unix/*.c) \
            "-l$LINK" $INCLUDES "-I$dbusdir" "-I$dbusdir/dbus" "-I$libexecinfo" \
            -DDBUS_COMPILATION -D_GNU_SOURCE -pthread -Wno-cpp -std=gnu11 $ADD_COMP
        popd > /dev/null

        # Link
        # Linking conventions: Start library with "lib"
        # c: create, r: insert with replacement, s: object-file index
        $AR crs $DEST/libdbus_static_$ARCH.a $(ls $dbusbuilddir/*.o)
    fi
    #objdump -f $dbusbuilddir/dbus_static.a

    # Create pkg-config file for the libdbus-sys crate
    local DEST_ARCH=$(realpath "$DEST/$ARCH")
printf "
prefix=$DEST_ARCH
exec_prefix=\${prefix}
libdir=\${prefix}/lib/$compiler_variant
includedir=\${prefix}/include

Name: dbus-1
Description: Static dbus library
Version: 1.13
Requires.private:
Libs: -L$DEST
Libs.private: -lgcc -ldbus_static_$ARCH
Cflags: -I\${includedir}
" > $DEST/$ARCH/dbus-1.pc
}

compile_crate() {
    local ARCH="$1"
    local TARGET="$2"
    local DEST_ARCH="$DEST/$ARCH"
    export PATH=$PATH:$DEST_ARCH/bin
    export PKG_CONFIG_PATH="$DEST_ARCH"
    export PKG_CONFIG_LIBDIR="$DEST_ARCH/lib"
    say "Build crate for $ARCH"
    local last_msg=$(cargo build --release --message-format=json --target $TARGET | tail -n1)
    [ $? != 0 ] && err "Failed to build"
    CRATE_NAME=$(echo $last_msg | jq -r -c '.package_id // empty' | cut -d' ' -f1)
    CRATE_VERSION=$(echo $last_msg | jq -r -c '.package_id // empty' | cut -d' ' -f2)
    local BINFILE=$(echo $last_msg | jq -r -c '.executable // empty')
    say "Before stripping $CRATE_NAME ($CRATE_VERSION): $(wc -c $BINFILE | cut -d' ' -f1) Bytes"
    if [ "$ARCH" = "x86_64" ]; then
        $DEST_ARCH/bin/strip $BINFILE
    else
        local compiler_variant=$(ls $DEST_ARCH/lib/gcc)
        $DEST_ARCH/bin/${compiler_variant}-strip $BINFILE
    fi
    say "After stripping: $(wc -c $BINFILE | cut -d' ' -f1) Bytes"
    mkdir -p $DEST/docker_root
    touch $DEST/docker_root/.empty
    local BINFILE_REL=$(realpath --relative-to="$DEST" "$BINFILE")
printf "
FROM scratch
COPY libdbus/docker_root /run/dbus
COPY $BINFILE_REL /bin
EXPOSE 53 67 80
VOLUME [\"/run/dbus\"]
ENTRYPOINT [\"/bin\"]
" > $DEST/../Dockerfile_$ARCH

}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        err "need '$1' (command not found) $2"
    fi
}

ensure() {
    "$@"
    if [ $? != 0 ]; then
        err "ERROR: command failed: $*";
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

prerequirements
compile "x86_64" "" ""
compile "aarch64" "aarch64-linux-musl-" "-DHAVE_BACKTRACE=0"
compile "armv7l" "armv7l-linux-musleabihf-" "-DHAVE_BACKTRACE=0"

export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_ALL_STATIC=1

rustup target add armv7-unknown-linux-musleabihf
rustup target add aarch64-unknown-linux-musl

compile_crate "x86_64" "x86_64-unknown-linux-musl"
compile_crate "aarch64" "aarch64-unknown-linux-musl"
compile_crate "armv7l" "armv7-unknown-linux-musleabihf"

docker="docker"
if command -v "podman" > /dev/null 2>&1; then
    docker="podman"
fi

if command -v $docker > /dev/null 2>&1; then
    tag="docker.pkg.github.com/openhab-nodes/wifi-captive-rs/$CRATE_NAME:$CRATE_VERSION"
    ensure $docker build -f $DEST/../Dockerfile_x86_64 -t $tag
    source github_token.inc
    ensure $docker push --creds=$GITHUB_USERNAME:$GITHUB_TOKEN $tag
fi

exit 0
