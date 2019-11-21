## iwd: Access points discovered / removed

`dbus-monitor --system interface=org.freedesktop.DBus.ObjectManager`

```
signal time=1573482149.417042 sender=:1.12 -> destination=(null destination) serial=11625 path=/; interface=org.freedesktop.DBus.ObjectManager; member=InterfacesAdded
   object path "/0/3/416b6775656e2032205453203631_psk"
   array [
      dict entry(
         string "net.connman.iwd.Network"
         array [
            dict entry(
               string "Name"
               variant                   string "Akguen 2 TS 61"
            )
            dict entry(
               string "Connected"
               variant                   boolean false
            )
            dict entry(
               string "Device"
               variant                   object path "/0/3"
            )
            dict entry(
               string "Type"
               variant                   string "psk"
            )
         ]
      )
      dict entry(
         string "org.freedesktop.DBus.Properties"
         array [
         ]
      )
   ]
```

```
signal time=1573482209.220742 sender=:1.12 -> destination=(null destination) serial=11646 path=/; interface=org.freedesktop.DBus.ObjectManager; member=InterfacesRemoved
   object path "/0/3/5a756772696666207665727765696765727421_psk"
   array [
      string "net.connman.iwd.Network"
      string "org.freedesktop.DBus.Properties"
   ]
```