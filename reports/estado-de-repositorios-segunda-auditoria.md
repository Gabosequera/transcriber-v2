# Estado de repositorios — segunda auditoría

Fecha de cierre: 2026-09-07T16:13:26-04:00.

## V1

| Campo | Resultado |
|---|---|
| ruta | `G:\TODO\transcriptor-installer-v0.2.0` |
| rama | `main` |
| HEAD | `c3677ba568cfc7d5947ec4695c42fffef6d79e03` |
| working tree | `git status --short` vacío |
| archivos Git | 112 |
| archivos físicos | 96.116 |
| tamaño físico | 29.904.519.458 bytes (~29,9 GB), dominado por runtimes/caches/modelos |
| lenguajes por extensión Git | Python 81; Markdown 15; BAT 5; PowerShell 2; JSON/TXT/YAML/Shell/SVG/PNG y VERSION |

Comparación: el HEAD y el working tree coinciden con la auditoría anterior. No hay evidencia de cambios de V1 entre ambas capturas; cualquier cambio futuro del otro agente debe generar una nueva captura.

## Workspace V2

`G:\TODO\transcriptor-v2` contiene 9.327 archivos físicos (~271,7 MB), incluyendo clones superficiales y documentación. Los clones tienen working tree limpio y permanecen en los commits bloqueados de `research/repositories-lock.json`.

## Referencias

Los ocho HEAD actuales coinciden exactamente con el lock anterior; no se actualizaron checkouts. Gausian conserva la contradicción README/`LICENSE`; OpenCut no se trató como código reutilizable por falta de términos confirmados.

## Limitación metodológica

El tamaño/contador físico incluye `.git`, clones, assets y caches de los clones; el tamaño de V1 no representa el ejecutable. No se ejecutó la aplicación ni comandos que escribieran en V1.
