HEADER = include/rdw/rdw.h

RUST_SOURCES = $(wildcard src/*.rs)

all: Rdw-0.1.gir Rdw-0.1.typelib Rdw-0.1.vapi

TARGET_PATH = $(PWD)/../target/debug
export PKG_CONFIG_PATH=$(PWD)
export GI_TYPELIB_PATH=$(PWD)
export LD_LIBRARY_PATH=$(TARGET_PATH)

$(TARGET_PATH)/librdw.so: $(RUST_SOURCES)
	cargo build --all-features

../cbindgen/target/debug/gbindgen:
	cargo build --all-features --manifest-path=../cbindgen/Cargo.toml

$(HEADER): ../cbindgen/target/debug/gbindgen $(RUST_SOURCES)
	../cbindgen/target/debug/gbindgen -o $(HEADER)

Rdw-0.1.gir: $(TARGET_PATH)/librdw.so $(HEADER)
	g-ir-scanner -v --warn-all \
		--namespace Rdw --nsversion=0.1 \
		-Iinclude --c-include "rdw/rdw.h" \
		--library=rdw --library-path=$(TARGET_PATH)/ \
		--include=GObject-2.0 -pkg gobject-2.0 \
		--include=Gtk-4.0 -pkg gtk4 \
		--output $@ \
		$(HEADER)

Rdw-0.1.typelib: Rdw-0.1.gir
	g-ir-compiler \
		--includedir=include \
		$< -o $@

Rdw-0.1.vapi: Rdw-0.1.gir
	vapigen \
		--pkg gtk4 \
		--library Rdw-0.1 \
		$<

DESTDIR ?= /
PREFIX ?= /usr
LIBDIR ?= /usr/lib64

install:
	cargo cbuild --release --prefix=$(PREFIX) --libdir=$(LIBDIR)
	cargo cinstall --release --destdir=$(DESTDIR) --prefix=$(PREFIX) --libdir=$(LIBDIR)

clean:
	rm -rf include
	rm -f Rdw-0.1.typelib
	rm -f Rdw-0.1.gir
	rm -f Rdw-0.1.vapi
	cargo clean
