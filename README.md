<div align="center">

# Sonora · fork de Omar

### Cliente de música nativo, escrito en Rust con GPUI

Spotify, YouTube Music, archivos locales y Samply en una sola aplicación **nativa**.

</div>

Este es el fork personal de Omar ([KratozZ12](https://github.com/KratozZ12)) de
[Sonora](https://github.com/nolight132/sonora), el cliente creado por
[nolight132](https://github.com/nolight132).

## Qué trae

- **Cuatro fuentes en una biblioteca:** Spotify, YouTube Music, archivos locales y proyectos
  de [Samply](https://samply.app).
- **Letras sincronizadas hechas desde cero:**
  - El texto se parte solo cuando no cabe, y cada letra se ilumina en el momento en que se canta.
  - Las palabras sostenidas hacen una ola y brillan; las demás se elevan un poco y vuelven a su sitio.
  - Los coros van en pequeño debajo de su verso, aunque empiecen a mitad de él.
- **Fondo en movimiento** con los colores de la portada en las páginas de artista y de álbum.
- **Cabecera de álbum grande**, teñida con el segundo color de la portada.
- **«A los fans también les gusta»** en la página de artista (solo Spotify).
- **Injertos:** un álbum local se puede añadir a la discografía de un artista de streaming, y una
  canción local a un álbum. Lo añadido se ordena arrastrándolo.
- **Búsqueda desde la barra de título.**
- **Traducción completa al español.**

## Samply

El token se lee de la variable `$SAMPLY_TOKEN` o del archivo `~/.config/sonora/samply-token`.
Sonora nunca lo escribe por su cuenta.

## Compilar

Hace falta Rust, `mold` y las bibliotecas de sistema. En Fedora:

```sh
sudo dnf install @development-tools pkgconf-pkg-config mold alsa-lib-devel fontconfig-devel \
  freetype-devel sqlite-devel libX11-devel libxcb-devel libXcursor-devel libXi-devel \
  libxkbcommon-devel libxkbcommon-x11-devel wayland-devel vulkan-loader-devel dbus-devel \
  mesa-vulkan-drivers
```

Después:

```sh
cargo build --release --package sonora
./target/release/sonora
```

La primera compilación tarda varios minutos, porque compila GPUI desde cero.

## Créditos

- [nolight132](https://github.com/nolight132): creador de Sonora.
- Omar ([KratozZ12](https://github.com/KratozZ12)): este fork.
- Claude, de Anthropic: programó los cambios de este fork junto con Omar.

## Licencia

GPL-3.0 o posterior, la misma que el proyecto original. El texto completo está en
[COPYING](./COPYING) y los avisos de terceros en [THIRD-PARTY.md](./THIRD-PARTY.md).
