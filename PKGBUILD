pkgname=axuralis
pkgver=1.2.3
pkgrel=1
pkgdesc="Music player for AxOS"
arch=('x86_64')
depends=('gtk4' 'libadwaita')
makedepends=('meson' 'rust' 'cargo')

build() {
    cd $srcdir
    meson setup build \
        --prefix=/usr \
        --buildtype=release
    meson compile -C build
}

package() {
    cd $srcdir
    DESTDIR="$pkgdir" meson install -C build
}