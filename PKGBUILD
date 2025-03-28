pkgname=axuralis
pkgver=1.0.0
pkgrel=1
pkgdesc="Music player for AxOS"
arch=('x86_64')
depends=('gtk4' 'libadwaita')
makedepends=('meson' 'ninja' 'rust' 'cargo')

build() {
    cd src
    meson setup build \
        --prefix=/usr \
        --buildtype=release
    meson compile -C build
}

package() {
    cd src
    DESTDIR="$pkgdir" meson install -C build
}