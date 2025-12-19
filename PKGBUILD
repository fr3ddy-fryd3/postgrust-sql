# Maintainer: fr3ddy-fryd3 <fr3ddyfryd3@gmail.com>
pkgname=postgrustsql
pkgver=2.2.0
pkgrel=1
pkgdesc="PostgreSQL-compatible database server written in Rust with MVCC, transactions, and full SQL support"
arch=('x86_64')
url="https://github.com/fr3ddy-fryd3/postgrust-sql"
license=('MIT')
depends=()
makedepends=('rust' 'cargo')
optdepends=(
    'postgresql: for pg_dump/pg_restore compatibility'
)
backup=('etc/postgrustsql/postgrustsql.toml')
source=("$pkgname-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')  # Update this after creating release tarball

build() {
    cd "$srcdir/postgrust-sql-$pkgver"

    # Build with release profile
    cargo build --release
}

check() {
    cd "$srcdir/postgrust-sql-$pkgver"

    # Run unit tests (skip integration tests that need server)
    cargo test --release --lib
}

package() {
    cd "$srcdir/postgrust-sql-$pkgver"

    # Install binaries
    install -Dm755 target/release/postgrustql "$pkgdir/usr/bin/postgrustql"
    install -Dm755 target/release/pgr_cli "$pkgdir/usr/bin/pgr_cli"
    install -Dm755 target/release/pgr_dump "$pkgdir/usr/bin/pgr_dump"
    install -Dm755 target/release/pgr_restore "$pkgdir/usr/bin/pgr_restore"

    # Install configuration
    install -Dm644 config/postgrustsql.toml "$pkgdir/etc/postgrustsql/postgrustsql.toml"

    # Install systemd service
    install -Dm644 systemd/postgrustsql.service "$pkgdir/usr/lib/systemd/system/postgrustsql.service"

    # Create data directory
    install -dm755 "$pkgdir/var/lib/postgrustsql"

    # Install documentation
    install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
    install -Dm644 ROADMAP.md "$pkgdir/usr/share/doc/$pkgname/ROADMAP.md"

    # Install license
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}

post_install() {
    echo ""
    echo "╔══════════════════════════════════════════════════════════╗"
    echo "║     PostgrustSQL $pkgver installed successfully             ║"
    echo "╠══════════════════════════════════════════════════════════╣"
    echo "║ Configuration:                                           ║"
    echo "║   Edit: /etc/postgrustsql/postgrustsql.toml              ║"
    echo "║                                                          ║"
    echo "║ Create postgres user (if needed):                        ║"
    echo "║   sudo useradd -r -M -s /bin/false postgres             ║"
    echo "║   sudo chown -R postgres:postgres /var/lib/postgrustsql  ║"
    echo "║                                                          ║"
    echo "║ Start service:                                           ║"
    echo "║   sudo systemctl start postgrustsql                      ║"
    echo "║   sudo systemctl enable postgrustsql                     ║"
    echo "║                                                          ║"
    echo "║ Connect with CLI:                                        ║"
    echo "║   pgr_cli                                                ║"
    echo "║                                                          ║"
    echo "║ Documentation: /usr/share/doc/postgrustsql/              ║"
    echo "╚══════════════════════════════════════════════════════════╝"
    echo ""
}

post_upgrade() {
    post_install
}

pre_remove() {
    if systemctl is-active --quiet postgrustsql; then
        systemctl stop postgrustsql
        systemctl disable postgrustsql
    fi
}

post_remove() {
    echo ""
    echo "PostgrustSQL has been removed."
    echo "Data directory /var/lib/postgrustsql was preserved."
    echo "Remove manually if needed: sudo rm -rf /var/lib/postgrustsql"
    echo ""
}
