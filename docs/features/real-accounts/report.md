# Reporte — Real accounts service

## Resumen

Se sustituyó el fixture del emulador por un servicio nativo de cuentas para `anthracite:accounts`. La entrega cubre la checklist v0.4 en dos fases: inventario y mutaciones básicas sobre 1.1.0, y paridad completa sobre 1.2.0 con `getent`/`source`, contraseñas, grupos, GECOS y borrado opcional del home.

## Fases integradas

| Fase | PR | Estado | Merge |
|------|----|--------|-------|
| Real accounts service (1.1.0) | #150 | merged | `29effd7` |
| Accounts 1.2.0 parity | #151 | merged | `4578671` |

## Funcionalidades entregadas

- Servicio nativo `accounts-service` con clase `host`, probe de `/etc` y `getent`, y runner de comandos inyectable.
- Inventario local y NSS: `source` local/nss, lock opcional, GECOS, grupo primario y grupos suplementarios.
- `create-user`, `delete-user` (con `remove-home`), `set-lock`, `set-password`, `create-group` y `delete-group`.
- Mutaciones por argv (`useradd`, `userdel`, `usermod`, `groupadd`, `groupdel`), sin shell; contraseña solo por stdin de `chpasswd`.
- Política por argv: `--control`, `--passwords`, `--allow-system`, `--allow-privileged-groups`, `--home-root`, `--shell`, `--default-shell`.
- Guards de root, cuenta del servicio, último miembro de cada grupo administrativo, ids de sistema, grupos privilegiados, home, shell y grupo primario.
- Bundle real, manifiesto y build script; emulador de demo actualizado al mismo contrato.
- Vista Accounts: tira de política, badges local/directorio/lock, alta de usuarios y grupos, contraseña, borrado con/sin home y refresco por visibilidad.
- `getent` con timeout de 2 s y drenaje en hilo para directorios grandes o bytes no UTF-8.

## Hallazgos de revisión atendidos

- Fase 1: guarda del último administrador por grupo, verificación real del unlock, conflicto falso por grupo homónimo y serialización de mutaciones con mutex.
- Fase 2: defaults de `home-root`/`default-shell` en la vista y el backend, `not_self` en `set-password`, retirada del verbo CLI que exponía la contraseña, protección de root/operator en el fixture, prueba de `remove-home`, timeout/drenaje de `getent` y botón Password deshabilitado para cuentas protegidas.

## Pruebas

- `apps/accounts/backend`: **37 tests**, `clippy -D warnings` y `fmt --check` limpios.
- `demo/emulator`: **44 tests**, clippy y fmt limpios.
- `apps/accounts/frontend`: `tsc --noEmit` y `bun build` limpios.
- E2E demo por el motor: alta de usuario con ids automáticos, contraseña, grupos, borrado con home y comprobación del subárbol en Files.
- Smoke real del bundle 1.2.0 contra `/etc` y validación del motor para `policy`, `users` y `groups`.
- La validación de mutaciones reales no se ejecuta en CI: queda manual en un contenedor desechable con `shadow-utils`.

## Decisiones registradas

- Cuentas locales primero; `getent` y `source` en la segunda fase.
- Permisos multiusuario y roles por usuario se difieren a un feature de plataforma `identity & permissions` para una versión posterior.
- Política de cuentas por argv congelado en el grant, no configuración editable por app.
- Un fallo de `getent` o un timeout degradan a inventario local, nunca bloquean el servicio.
- La contraseña no se expone como verbo CLI porque el CLI la pondría en argv.

## Fuera de alcance

- Identidad multiusuario, roles y permisos por usuario.
- Manager, auditoría y UI de aprobación (M4-close/M5).
- Paginación o filtrado del inventario grande; el listado devuelve el snapshot completo.
- Validación automatizada de mutaciones reales en el host.

## Pendientes y riesgos

- Validar manualmente crear/borrar usuario, contraseña, grupos y home en un contenedor desechable con `shadow-utils`.
- `getent` puede seguir añadiendo latencia en hosts con directorios muy grandes; valorar caché o paginación en una fase posterior.
- El servicio necesita root o acceso equivalente para mutar; en CI solo se prueba con fixtures.
