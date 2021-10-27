HEADER = inst/usr/include/rdw4/rdw.h
GIR = inst/usr/share/gir-1.0/Rdw4.gir
TYPELIB = inst/usr/lib64/girepository-1.0/Rdw4.typelib
VAPI = inst/usr/share/vala/vapi/rdw4.vapi

RUST_SOURCES = $(shell find src)

all: $(GIR) $(TYPELIB) $(VAPI)

export PKG_CONFIG_PATH=$(PWD)/inst/usr/lib64/pkgconfig
export GI_TYPELIB_PATH=$(PWD)/inst/usr/lib64/girepository-1.0
export LD_LIBRARY_PATH=$(PWD)/inst/usr/lib64

$(HEADER): $(RUST_SOURCES)
	cargo cinstall --release --destdir=inst --prefix=/usr --libdir=/usr/lib64

$(GIR): $(HEADER)
	mkdir -p $(@D)
	g-ir-scanner -v --warn-all \
		--namespace Rdw4 --nsversion=0.1 \
		--identifier-prefix Rdw \
		-Iinst/include --c-include "rdw.h" \
		--library=rdw4 --library-path=inst/usr/lib64 \
		--include=Gtk-4.0 -pkg gtk4 \
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

install: $(HEADER)
	sudo cp -r inst/* $(DESTDIR)/
