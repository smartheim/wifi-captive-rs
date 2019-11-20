#!/bin/sh -e
# cargo install --git https://github.com/diwic/dbus-rs --force  dbus-codegen

# NM and Device
cat networkmanager.xml | dbus-codegen-rust -i org.freedesktop. -c nonblock -m None  -f NetworkManager --dbuscrate ::dbus -o networkmanager.rs
cat device.xml | dbus-codegen-rust -i org.freedesktop.NetworkManager. -c nonblock -m None  -f Device,Device.Wireless, --dbuscrate ::dbus -o device.rs

# Connections and Connection
cat connections.xml | dbus-codegen-rust -i org.freedesktop.NetworkManager. -c nonblock -m None  -f Settings, --dbuscrate ::dbus -o connections.rs
cat connection_nm.xml | dbus-codegen-rust -i org.freedesktop.NetworkManager.Settings. -c nonblock -m None  -f Connection, --dbuscrate ::dbus -o connection_nm.rs
cat connection_active.xml | dbus-codegen-rust -i org.freedesktop.NetworkManager. -c nonblock -m None  -f Connection.Active, --dbuscrate ::dbus -o connection_active.rs

# Access Points
cat access_point.xml | dbus-codegen-rust -i org.freedesktop.NetworkManager. -c nonblock -m None -f AccessPoint, --dbuscrate ::dbus -o access_point.rs
