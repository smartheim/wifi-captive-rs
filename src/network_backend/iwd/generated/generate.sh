#!/bin/sh -e
# cargo install --git https://github.com/diwic/dbus-rs --force  dbus-codegen

# NM and Device
cat iwd.xml | dbus-codegen-rust -c nonblock -m None --dbuscrate ::dbus -o iwd.rs
cat device.xml | dbus-codegen-rust  -c nonblock -m None --dbuscrate ::dbus -o device.rs
cat adapter.xml | dbus-codegen-rust -c nonblock -m None --dbuscrate ::dbus -o adapter.rs
cat network.xml | dbus-codegen-rust -c nonblock -m None --dbuscrate ::dbus -o network.rs
cat known_network.xml | dbus-codegen-rust -c nonblock -m None --dbuscrate ::dbus -o known_network.rs
