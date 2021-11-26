HEADER = inst/usr/include/rdw4/rdw-rdp.h
GIR = inst/usr/share/gir-1.0/RdwRdp-4.0.gir
TYPELIB = inst/usr/lib64/girepository-1.0/RdwRdp-4.0.typelib
VAPI = inst/usr/share/vala/vapi/rdw4-rdp.vapi

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
		--namespace RdwRdp --nsversion=4.0 \
		--identifier-prefix RdwRdp \
		-Iinst/include --c-include "rdw-rdp.h" \
		--library=rdw4-rdp --library-path=inst/usr/lib64 \
		--include=Rdw-4.0 --pkg rdw4 \
		--output $@ \
		$<

$(TYPELIB): $(GIR)
	mkdir -p $(@D)
	g-ir-compiler $< -o $@

$(VAPI): $(GIR)
	mkdir -p $(@D)
	vapigen \
		--pkg rdw4 \
		--library rdw4-rdp \
		$< -d $(@D)
	echo rdw4 > $(@D)/rdw4-rdp.deps

install: $(HEADER)
	sudo cp -r inst/* $(DESTDIR)/
