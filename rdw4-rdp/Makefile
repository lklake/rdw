MACHINE := $(shell uname -i)
ifeq ($(MACHINE), x86_64)
LIB = lib64
else
LIB = lib
endif

HEADER = inst/usr/include/rdw4/rdw-rdp.h
GIR = inst/usr/share/gir-1.0/RdwRdp-4.0.gir
TYPELIB = inst/usr/$(LIB)/girepository-1.0/RdwRdp-4.0.typelib
VAPI = inst/usr/share/vala/vapi/rdw4-rdp.vapi

RUST_SOURCES = $(shell find src)

all: $(GIR) $(TYPELIB) $(VAPI)

export PKG_CONFIG_PATH=$(PWD)/inst/usr/$(LIB)/pkgconfig
export GI_TYPELIB_PATH=$(PWD)/inst/usr/$(LIB)/girepository-1.0
export LD_LIBRARY_PATH=$(PWD)/inst/usr/$(LIB)

$(HEADER): $(RUST_SOURCES)
	cargo cinstall --release --destdir=inst --prefix=/usr --libdir=/usr/$(LIB)

$(GIR): $(HEADER)
	mkdir -p $(@D)
	g-ir-scanner -v --warn-all \
		--namespace RdwRdp --nsversion=4.0 \
		--identifier-prefix RdwRdp \
		-Iinst/include --c-include "rdw-rdp.h" \
		--library=rdw4-rdp --library-path=inst/usr/$(LIB) \
		--include-uninstalled=freerdp2-1.0.gir \
		--include=Rdw-4.0 --pkg rdw4 \
		--pkg freerdp2 \
		--output $@ \
		$<

$(TYPELIB): $(GIR)
	mkdir -p $(@D)
	g-ir-compiler \
		--includedir . \
		$< -o $@

$(VAPI): $(GIR)
	mkdir -p $(@D)
	vapigen \
		--girdir . \
		--metadatadir . \
		--pkg rdw4 \
		--pkg freerdp2 \
		--library rdw4-rdp \
		$< -d $(@D)
	echo rdw4 > $(@D)/rdw4-rdp.deps
	echo freerdp2 >> $(@D)/rdw4-rdp.deps

install: $(HEADER)
	sudo cp -r inst/* $(DESTDIR)/
