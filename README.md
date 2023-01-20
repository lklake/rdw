# Description
forked from https://gitlab.gnome.org/malureau/rdw

Remove the broken dependency on cbindgen so that it can be used to provide rdw4 dependencies for the qemu-display project.

# RDW: Gtk4 remote desktop widgets

The project goal is to provide Gtk4 widgets for remote desktops: VNC, RDP,
Spice, QEMU -display dbus ...

There is a lot of interaction details to handle correctly and consistently
across the various protocols, hopefully the base widget allows to have some
common behaviour and to share code.

This is a Rust project, however it is meant to be usable from C, and GObject
Introspection (gir) bindings are provided (as well as Vala). For this to work,
the code is compiled as various shared libraries (rdw4/rdw4-vnc/rdw4-rdp..)

GObject-aware cbindgen & cargo-c are used to help, but there is a lot of room
for improvements to be able to ship GIR libraries from Rust code easily
(consider this in development with all the quirks). Furthermore, many of the
Rust bindings are either new or in active development (gtk-rs, gstreamer-rs, freerdp-rs etc)

## Dependencies

gtk4, gstreamer, gtk-vnc, spice-gtk, freerdp2..

## Building

By default, the crates link with ``rdw4 = features ["bindings"]``, so you must
compile and install rdw4 first!

``` sh
$ cd rdw4
$ make install
```

Then you should be able to run the demo:

``` sh
$ cd ../
$ cargo run -- rdp://192.168.122.251 (or spice://, vnc://)
```

To compile and install the other protocols as shared libraries:

``` sh
$ cd rdw-vnc (or -spice, -rdp)
$ make install
```

