# Distro-neutral build/install for ajuste. Packagers: `make build && make DESTDIR=... install`.
PREFIX  ?= /usr/local
BINDIR  ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
# udev ignores /usr/local; use /etc for source installs, /usr/lib for distro packages.
UDEVDIR ?= $(if $(filter /usr,$(PREFIX)),/usr/lib/udev/rules.d,/etc/udev/rules.d)
CARGO   ?= cargo
CARGO_FLAGS ?= --locked
TARGET_DIR ?= $(or $(CARGO_TARGET_DIR),target)

.PHONY: all build install uninstall

all: build

build:
	$(CARGO) build --release $(CARGO_FLAGS) -p ajuste -p ajustectl

install:
	install -Dm755 $(TARGET_DIR)/release/ajuste            $(DESTDIR)$(BINDIR)/ajuste
	install -Dm755 $(TARGET_DIR)/release/ajustectl         $(DESTDIR)$(BINDIR)/ajustectl
	install -Dm644 packaging/70-fractal-scape.rules        $(DESTDIR)$(UDEVDIR)/70-fractal-scape.rules
	install -Dm644 packaging/ajuste.desktop                $(DESTDIR)$(DATADIR)/applications/ajuste.desktop
	install -Dm644 packaging/ajuste-autostart.desktop      $(DESTDIR)$(DATADIR)/ajuste/ajuste-autostart.desktop
	install -Dm644 README.md                               $(DESTDIR)$(DATADIR)/doc/ajuste/README.md
	install -Dm644 FINDINGS.md                             $(DESTDIR)$(DATADIR)/doc/ajuste/FINDINGS.md
	install -Dm644 LICENSE-MIT                             $(DESTDIR)$(DATADIR)/licenses/ajuste/LICENSE-MIT
	install -Dm644 LICENSE-APACHE                          $(DESTDIR)$(DATADIR)/licenses/ajuste/LICENSE-APACHE

uninstall:
	rm -f  $(DESTDIR)$(BINDIR)/ajuste $(DESTDIR)$(BINDIR)/ajustectl
	rm -f  $(DESTDIR)$(UDEVDIR)/70-fractal-scape.rules
	rm -f  $(DESTDIR)$(DATADIR)/applications/ajuste.desktop
	rm -rf $(DESTDIR)$(DATADIR)/ajuste $(DESTDIR)$(DATADIR)/doc/ajuste $(DESTDIR)$(DATADIR)/licenses/ajuste
