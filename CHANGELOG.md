# Change Log

This project adheres to [Semantic Versioning](http://semver.org/).

## v0.3.0 - 2019-12-11

Update to latest async ecosystem

* Update to tokio 0.2
* Update to futures 0.3
* Update to hyper 0.13
* Fix --ui-directory command line argument

## v0.2.0 - 2019-11-20

Multiple Backends and stable Rust

* Support iwd dbus API next to Network Manager:
  - Generic API in `network_interface/*`
  - Backend API in `nm/` and `iwd/`
* Use stable Rust 1.39

## v0.1.0 - 2019-10-10

First release

* Network manager dbus API
* dhcp server
* dns server
* state machine for captive portal
* **warn**: nightly rust, because async/await requires rust 1.38
* **warn**: requires local copy of dbus crate, because of missing
  asnyc / await functionality
