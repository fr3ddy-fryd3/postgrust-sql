# Maintainer: fr3ddy-fryd3 <fr3ddyfryd3@gmail.com>
pkgname=postgrustsql
pkgver=2.2.2
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
install=postgrustsql.install
# TEMPORARY: Using develop branch for testing, change to v$pkgver when ready
source=("$pkgname-develop.tar.gz::$url/archive/refs/heads/develop.tar.gz")
sha256sums=('SKIP')  # Update this after creating release tarball

prepare() {
    # Create .install script (must be in same dir as PKGBUILD)
    cat > "${startdir}/postgrustsql.install" << 'INSTALLEOF'
post_install() {
    if ! getent passwd postgres > /dev/null 2>&1; then
        useradd -r -M -s /bin/false postgres
    fi
    mkdir -p /var/lib/postgrustsql/data
    chown -R postgres:postgres /var/lib/postgrustsql
    chmod 750 /var/lib/postgrustsql
    systemctl daemon-reload 2>/dev/null || true
    echo "PostgrustSQL installed. Start: sudo systemctl start postgrustsql"
}

post_upgrade() {
    mkdir -p /var/lib/postgrustsql/data 2>/dev/null || true
    chown -R postgres:postgres /var/lib/postgrustsql 2>/dev/null || true
    systemctl daemon-reload 2>/dev/null || true
}

pre_remove() {
    systemctl stop postgrustsql 2>/dev/null || true
    systemctl disable postgrustsql 2>/dev/null || true
}

post_remove() {
    systemctl daemon-reload 2>/dev/null || true
    echo "Data in /var/lib/postgrustsql preserved"
}
INSTALLEOF
}

build() {
    cd "$srcdir/postgrust-sql-develop"

    # Build with release profile
    cargo build --release
}

check() {
    cd "$srcdir/postgrust-sql-develop"

    # Run unit tests (skip integration tests that need server)
    cargo test --release --lib
}

package() {
    cd "$srcdir/postgrust-sql-develop"

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

