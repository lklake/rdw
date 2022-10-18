MACHINE := $(shell uname -i)
ifeq ($(MACHINE), x86_64)
LIB = lib64
else
LIB = lib
endif

ifdef MINGW_PREFIX
export MSYS2_ARG_CONV_EXCL=*
export PREFIX=$(MINGW_PREFIX)
else
export PREFIX=/usr
endif
export PKG_CONFIG_PATH=$(PWD)/inst/$(PREFIX)/$(LIB)/pkgconfig
export GI_TYPELIB_PATH=$(PWD)/inst/$(PREFIX)/$(LIB)/girepository-1.0
export LD_LIBRARY_PATH=$(PWD)/inst/$(PREFIX)/$(LIB)

HEADER = inst/$(PREFIX)/include/rdw4/rdw.h
GIR = inst/$(PREFIX)/share/gir-1.0/Rdw-4.0.gir
TYPELIB = inst/$(PREFIX)/$(LIB)/girepository-1.0/Rdw-4.0.typelib
VAPI = inst/$(PREFIX)/share/vala/vapi/rdw4.vapi

RUST_SOURCES = $(shell find src)

all: $(GIR) $(TYPELIB) $(VAPI)

$(HEADER): $(RUST_SOURCES)
	cargo cinstall --release --destdir=inst --prefix=$(PREFIX) --libdir=$(PREFIX)/$(LIB)

$(GIR): $(HEADER)
	mkdir -p $(@D)
	g-ir-scanner -v --warn-all \
		--namespace Rdw --nsversion=4.0 \
		--identifier-prefix Rdw \
		-Iinst/include --c-include "rdw.h" \
		--library=rdw4 --library-path=inst/$(PREFIX)/$(LIB) \
		--include=Gtk-4.0 --pkg gtk4 \
		--output $@ \
		$<

$(TYPELIB): $(GIR)
	mkdir -p $(@D)
	g-ir-compiler $< -o $@

$(VAPI): $(GIR)
	mkdir -p $(@D)
	vapigen \
		--pkg gtk4 \
		--library rdw4 \
		$< -d $(@D)
	echo gtk4 > $(@D)/rdw4.deps

install: $(HEADER)
	sudo cp -r inst/* $(DESTDIR)/
