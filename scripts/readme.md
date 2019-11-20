# Scripts and helper tools

* set_net_cap: Explained in the main readme
* build_dbus.sh: Cross compile for x86_64, armv7l, aarch64 as static musl binaries
* deploy: Deploy to Github Releases and Github Package Registry (Docker container)
* config.h + dbus-arch-deps.h: Used for building libdbus. 
  The build script of libdus autoconf/cmake build script is not used.
  Instead those pre-generated files and libdbus source files are compiled
  via the musl toolchain for the respective architecture.
  