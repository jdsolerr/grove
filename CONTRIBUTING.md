# Contribuir a Grove

¡Gracias por querer ayudar! Aceptamos *issues* y *pull requests*.

## Antes de empezar
- Abre un *issue* antes de una PR grande para acordar el enfoque.
- Una PR = un cambio. Que sea pequeña y revisable.
- Al enviar una PR aceptas que tu contribución se publique bajo la licencia MIT del proyecto.

## Entorno
Requisitos y comandos de desarrollo: sección **Desarrollo** del [README](README.md).

Antes de abrir la PR:
```bash
cargo fmt --all && cargo clippy --workspace && cargo test --workspace
cd app && npm run build
```

## Traducciones e ideas propias
- Para **añadir un idioma**, lo mejor es una PR: el i18n está en la hoja de ruta y se agradece ayuda.
- Si prefieres un **fork** (otro idioma, otras funciones), es bienvenido. Léete [`TRADEMARK.md`](TRADEMARK.md): tu fork debe llevar otro nombre y otro logo, y su propia OAuth App de GitHub.

## Seguridad
No abras *issues* públicos con vulnerabilidades: usa [`SECURITY.md`](SECURITY.md).

## Conducta
Sé respetuoso. Se puede cerrar cualquier *issue* o PR que sea abusivo, spam o fuera de tema.
