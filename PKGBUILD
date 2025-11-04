# Maintainer: Your Name <your.email@example.com>
pkgname=bun-docs-mcp-proxy
pkgver=0.2.1
pkgrel=1
pkgdesc="MCP proxy for Bun documentation search"
arch=('x86_64' 'aarch64')
url="https://github.com/kjanat/bun-docs-mcp-proxy"
license=('MIT')
depends=()
makedepends=('rust' 'cargo')
source=("$pkgname-$pkgver.tar.gz::https://github.com/kjanat/$pkgname/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')  # Update with actual checksum after first build

build() {
    cd "$pkgname-$pkgver"
    cargo build --release --locked
}

check() {
    cd "$pkgname-$pkgver"
    cargo test --release --locked
}

package() {
    cd "$pkgname-$pkgver"
    install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
    install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
}
