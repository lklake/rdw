ifeq ($(MACHINE), x86_64)
LIB = lib64
else
LIB = lib
endif

HEADER = inst/usr/include/rdw4/rdw-vnc.h
GIR = inst/usr/share/gir-1.0/RdwVnc-4.0.gir
TYPELIB = inst/usr/$(LIB)/girepository-1.0/RdwVnc-4.0.typelib
VAPI = inst/usr/share/vala/vapi/rdw4-vnc.vapi

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
		--namespace RdwVnc --nsversion=4.0 \
		--identifier-prefix RdwVnc \
		-Iinst/include --c-include "rdw-vnc.h" \
		--library=rdw4-vnc --library-path=inst/usr/$(LIB) \
		--include=GVnc-1.0 --pkg gvnc-1.0 \
		--include=Rdw-4.0 --pkg rdw4 \
		--output $@ \
		$<

$(TYPELIB): $(GIR)
	mkdir -p $(@D)
	g-ir-compiler $< -o $@

$(VAPI): $(GIR)
	mkdir -p $(@D)
	vapigen \
		--pkg gvnc-1.0 \
		--pkg rdw4 \
		--library rdw4-vnc \
		$< -d $(@D)
	echo rdw4 > $(@D)/rdw4-vnc.deps
	echo gvnc-1.0 >> $(@D)/rdw4-vnc.deps

install: $(HEADER)
	sudo cp -r inst/* $(DESTDIR)/
