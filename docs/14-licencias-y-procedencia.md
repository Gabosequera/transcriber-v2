# 14 — Licencias y procedencia

Esto no es asesoría legal. La distribución comercial exige revisión de un abogado, SBOM y auditoría de plugins/codecs.

| Componente | Licencia observada | Copyleft/linking | Comercial | Compatibilidad con PolyForm V1 | Recomendación |
|---|---|---|---|---|---|
| Gausian core | conflicto: `LICENSE` Apache-2.0, README MPL-2.0/pro comercial | no determinar obligaciones hasta aclarar qué archivos cubre cada licencia | indeterminado | indeterminado | tratar todo como bloqueado; pedir aclaración al titular |
| OpenCut | no se confirmó licencia permisiva válida en el commit | derechos no inferidos del código público | no asumir | no copiar | referencia solamente |
| Cutlass | MIT + Apache-2.0 | notices; Apache patent grant | sí, sujeto a terceros | normalmente compatible | adaptar/copy candidate con inventario |
| Kerf | PolyForm Noncommercial | no comercial; titularidad reservada | no para producto comercial sin acuerdo | no concede derechos nuevos | no copiar ni depender |
| GStreamer-RS | MIT/Apache | bindings permisivos; runtime GStreamer/plugins separados | sí con notices y componentes | revisar combinación | dependencia fijada |
| GES/GStreamer | LGPL upstream | obligaciones de LGPL y plugins | sí con cumplimiento | revisar distribución | proceso/plugin bundle auditado |
| FFmpeg | LGPL 2.1+ por defecto; partes GPL opcionales | dynamic/relink/source obligations; GPL si se habilita | sí bajo términos; nonfree no redistribuible | revisar | binario externo configurado sin GPL/nonfree hasta decisión |
| MLT | GPL | copyleft de proyecto | no como dependencia sin estrategia GPL | no | referencia |
| Kdenlive | GPL | copyleft | no copiar | no | referencia |
| Shotcut | GPL | copyleft | no copiar | no | referencia |
| Slint | GPLv3/royalty-free/comercial | condiciones de licencia elegida | revisar pricing/attribution | no asumir | alternativa, decisión legal previa |

Procedencia mínima: URL canónica, commit, archivo, hash, SPDX, copyright, cambios y destino. No usar código sin licencia explícita. Las patentes de codecs son un riesgo separado de copyright.
