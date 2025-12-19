# PostgrustSQL Makefile
# Build, install, and package targets

VERSION := 2.2.0
PREFIX ?= /usr/local
DESTDIR ?=

# Installation directories
BINDIR := $(PREFIX)/bin
LIBDIR := $(PREFIX)/lib/postgrustsql
DATADIR := $(PREFIX)/share/postgrustsql
SYSCONFDIR ?= /etc/postgrustsql
SYSTEMDDIR := /etc/systemd/system
VARDIR := /var/lib/postgrustsql

# Binaries
BINARIES := postgrustql pgr_cli pgr_dump pgr_restore
CARGO := cargo
INSTALL := install
STRIP := strip

.PHONY: all build release test clean install uninstall \
        install-server install-tools install-config install-systemd \
        package-deb package-arch help

# Default target
all: release

# Help target
help:
	@echo "PostgrustSQL Build System v$(VERSION)"
	@echo ""
	@echo "Available targets:"
	@echo "  make build           - Build debug binaries"
	@echo "  make release         - Build release binaries (default)"
	@echo "  make test            - Run all tests"
	@echo "  make clean           - Remove build artifacts"
	@echo ""
	@echo "  make install         - Install all components (server + tools)"
	@echo "  make install-server  - Install server only"
	@echo "  make install-tools   - Install client tools only"
	@echo "  make install-config  - Install config files"
	@echo "  make install-systemd - Install systemd service"
	@echo "  make uninstall       - Remove installed files"
	@echo ""
	@echo "  make package-deb     - Create Debian package"
	@echo "  make package-arch    - Create Arch Linux package"
	@echo ""
	@echo "Environment variables:"
	@echo "  PREFIX=$(PREFIX)     - Installation prefix"
	@echo "  DESTDIR=$(DESTDIR)   - Staging directory for packaging"
	@echo "  SYSCONFDIR=$(SYSCONFDIR) - System config directory"

# Build targets
build:
	@echo "Building debug binaries..."
	$(CARGO) build

release:
	@echo "Building release binaries..."
	$(CARGO) build --release
	@echo ""
	@echo "Built binaries:"
	@ls -lh target/release/postgrustql target/release/pgr_* 2>/dev/null | grep -v "\.d$$" || true

test:
	@echo "Running unit tests..."
	$(CARGO) test --lib
	@echo ""
	@echo "Running integration tests..."
	@./tests/integration/test_features.sh || true

clean:
	@echo "Cleaning build artifacts..."
	$(CARGO) clean
	rm -rf data/ data_*/ *.db *.wal
	@echo "Clean complete."

# Installation targets
install: install-server install-tools install-config
	@echo ""
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     PostgrustSQL $(VERSION) installed successfully          ║"
	@echo "╠══════════════════════════════════════════════════════════╣"
	@echo "║ Server:     $(BINDIR)/postgrustql                   ║"
	@echo "║ CLI:        $(BINDIR)/pgr_cli                       ║"
	@echo "║ Tools:      $(BINDIR)/pgr_dump, pgr_restore        ║"
	@echo "║ Config:     $(SYSCONFDIR)/postgrustsql.toml        ║"
	@echo "║ Data dir:   $(VARDIR)/data                         ║"
	@echo "╠══════════════════════════════════════════════════════════╣"
	@echo "║ Next steps:                                              ║"
	@echo "║  1. Edit config: sudo vim $(SYSCONFDIR)/postgrustsql.toml ║"
	@echo "║  2. Install systemd: sudo make install-systemd           ║"
	@echo "║  3. Start service: sudo systemctl start postgrustsql     ║"
	@echo "║  4. Enable on boot: sudo systemctl enable postgrustsql   ║"
	@echo "║  5. Check status: sudo systemctl status postgrustsql     ║"
	@echo "╚══════════════════════════════════════════════════════════╝"

install-server: release
	@echo "Installing server..."
	$(INSTALL) -Dm755 target/release/postgrustql $(DESTDIR)$(BINDIR)/postgrustql
	@if command -v $(STRIP) >/dev/null 2>&1; then \
		echo "Stripping binary..."; \
		$(STRIP) $(DESTDIR)$(BINDIR)/postgrustql; \
	fi

install-tools: release
	@echo "Installing client tools..."
	$(INSTALL) -Dm755 target/release/pgr_cli $(DESTDIR)$(BINDIR)/pgr_cli
	$(INSTALL) -Dm755 target/release/pgr_dump $(DESTDIR)$(BINDIR)/pgr_dump
	$(INSTALL) -Dm755 target/release/pgr_restore $(DESTDIR)$(BINDIR)/pgr_restore
	@if command -v $(STRIP) >/dev/null 2>&1; then \
		echo "Stripping binaries..."; \
		$(STRIP) $(DESTDIR)$(BINDIR)/pgr_*; \
	fi

install-config:
	@echo "Installing configuration..."
	$(INSTALL) -Dm644 config/postgrustsql.toml $(DESTDIR)$(SYSCONFDIR)/postgrustsql.toml
	@echo "Creating data directory..."
	$(INSTALL) -dm755 $(DESTDIR)$(VARDIR)/data

install-systemd:
	@echo "Installing systemd service..."
	@if [ ! -f systemd/postgrustsql.service ]; then \
		echo "Error: systemd/postgrustsql.service not found"; \
		echo "Run this target after systemd service file is created"; \
		exit 1; \
	fi
	$(INSTALL) -Dm644 systemd/postgrustsql.service $(DESTDIR)$(SYSTEMDDIR)/postgrustsql.service
	@if [ -z "$(DESTDIR)" ]; then \
		echo "Reloading systemd daemon..."; \
		systemctl daemon-reload; \
	fi
	@echo "Systemd service installed."
	@echo "Enable with: sudo systemctl enable postgrustsql"
	@echo "Start with:  sudo systemctl start postgrustsql"

uninstall:
	@echo "Uninstalling PostgrustSQL..."
	rm -f $(BINDIR)/postgrustql
	rm -f $(BINDIR)/pgr_cli
	rm -f $(BINDIR)/pgr_dump
	rm -f $(BINDIR)/pgr_restore
	rm -f $(SYSCONFDIR)/postgrustsql.toml
	rm -f $(SYSTEMDDIR)/postgrustsql.service
	@if [ -z "$(DESTDIR)" ] && systemctl is-active --quiet postgrustsql; then \
		echo "Stopping postgrustsql service..."; \
		systemctl stop postgrustsql; \
		systemctl disable postgrustsql; \
		systemctl daemon-reload; \
	fi
	@echo "Uninstall complete."
	@echo "Note: Data directory $(VARDIR) was NOT removed (contains your databases)"
	@echo "      Remove manually if needed: sudo rm -rf $(VARDIR)"

# Packaging targets
package-deb:
	@echo "Creating Debian package..."
	@if [ ! -d debian ]; then \
		echo "Error: debian/ directory not found"; \
		echo "Create debian packaging files first"; \
		exit 1; \
	fi
	dpkg-buildpackage -us -uc -b
	@echo "Debian package created in parent directory"

package-arch:
	@echo "Creating Arch Linux package..."
	@if [ ! -f PKGBUILD ]; then \
		echo "Error: PKGBUILD not found"; \
		echo "Create PKGBUILD file first"; \
		exit 1; \
	fi
	makepkg -sf
	@echo "Arch package created: postgrustsql-$(VERSION)-1-x86_64.pkg.tar.zst"

# Development targets
check:
	$(CARGO) check

clippy:
	$(CARGO) clippy --release

fmt:
	$(CARGO) fmt

run: release
	@echo "Starting PostgrustSQL server..."
	POSTGRUSTQL_DATA_DIR=./data POSTGRUSTQL_INITDB=true ./target/release/postgrustql

run-cli: release
	@echo "Starting PostgrustSQL CLI client..."
	./target/release/pgr_cli

# Show installation paths
show-paths:
	@echo "Installation paths:"
	@echo "  PREFIX=$(PREFIX)"
	@echo "  DESTDIR=$(DESTDIR)"
	@echo "  BINDIR=$(BINDIR)"
	@echo "  SYSCONFDIR=$(SYSCONFDIR)"
	@echo "  SYSTEMDDIR=$(SYSTEMDDIR)"
	@echo "  VARDIR=$(VARDIR)"
