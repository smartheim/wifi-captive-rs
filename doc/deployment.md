# Building and deploying

**Compile** with `cargo build`. You need at least Rust 1.39.
The default is to use the *NetworkManager* backend.
- If you want to to use [*iwd*](https://wiki.archlinux.org/index.php/Iwd), use `cargo build --features iwd --no-default-features`.
- If you want to to use [*connman*](https://01.org/connman/documentation), use `cargo build --features connman --no-default-features`. 
- Enable the "includeui" feature to embed the ui files into the binary.
  No disk access necessary anymore. This is the default for release builds.
  
## Cross compiling and software container distribution

If you build with `cargo build` the resulting binary will be
build with the system default linker. This usually means that
the binary is dynamically linked to the systems libc and dbus library.

For truly static binaries for all architectures you may use
the `scripts/build.sh` script instead. It downloads the musl gcc
compiler for x86_64, armv7l and aarch64 and builds the crate.

If a docker CLI compatible binary can be found, this will also
build container images.
All containers are self-contained "from scratch" with only the binary and
a `/run/dbus` directory.

## Usage of the containers

The following examples use the "docker" CLI.
Command line compatible tools like "podman" work exactly the same.

You must share the DBus system daemon socket path and expose ports
53 (dns), 67 (dhcp server) and 80 (http captive portal web page) like so:
 
```sh
docker ... -v /run/dbus/system_bus_socket:/run/dbus/system_bus_socket -p 53:53 -p 67:67 -p 80:80
```

To not collide with other running web-services, dns or dhcp services,
you might want to restrict the port forwarding to your wifi adapter interface.
Assuming that you have assigned the static IP *192.168.4.1* to your adapter:

```sh
docker ... -p 192.168.4.1:53:53 -p 192.168.4.1:67:67 -p 192.168.4.1:80:80
```

All ports below 1024 are system ports. This is also true for software container processes.
You either need the SYS_NET Linux capability assigned to the container,
or use the command line arguments to change the ports .

An alternative is:

```sh
docker ... --net="host" --privileged
```

This allows the service to bind the dns, dhcp, web ports (53, 67, 80)
directly on the host network. The service already makes sure that it only
binds to wifi adapter interfaces.