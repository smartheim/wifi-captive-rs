#!/bin/sh -e
# cargo install --git https://github.com/diwic/dbus-rs --force  dbus-codegen

# NM and Device
cat networkmanager.xml | dbus-codegen-rust -i org.freedesktop.DBus. -c nonblock -m None  > networkmanager.rs
cat device.xml | dbus-codegen-rust -i org.freedesktop.DBus. -c nonblock -m None > device.rs

# Connections and Connection
cat connections.xml | dbus-codegen-rust -i org.freedesktop.DBus. -c nonblock -m None > connections.rs
cat connection_nm.xml | dbus-codegen-rust -i org.freedesktop.DBus. -c nonblock -m None > connection_nm.rs
cat connection_active.xml | dbus-codegen-rust -i org.freedesktop.DBus. -c nonblock -m None > connection_active.rs

# Access Points
cat access_point.xml | dbus-codegen-rust -i org.freedesktop.DBus. -c nonblock -s -m None > access_point.rs
