# 06 — Sistema de capas editoriales

## Contrato

`editorial-layer/1` de V1 contiene `layer_id`, `kind`, `name`, `color`, `media_fingerprint`, `source_master_digest`, `revision` e `items`. Cada item tiene `item_id`, `label`, `comment`, `state`, `edited`, `parent_id` y uno o más `{t_ini,t_fin}`; puede llevar `origin`, `evidence` y campos de procedencia. La validación exige ids únicos, rangos ordenados y jerarquía acíclica dentro de 32 niveles.

## Diseño V2

Usar `LayerKind::{Author, Topics, Trims, Signals, Agent, Review}` y `ItemState::{Proposed, Accepted, Disabled, Deleted}`. Las señales derivadas (risa/arousal/intensidad/transcript/cara/visión) son read-only; las capas de autor son fuente humana; las de AI son propuestas. Un item aceptado por humano queda protegido frente a merge AI. Un tombstone evita resurrección.

El canvas ofrece un índice temporal por capa y LOD por píxel. El inspector puede navegar de item a utterance/word/evidence; una selección conjunta nunca implica que una operación multimedia modifique una capa semántica sin comando explícito.

## Adaptadores

Marcas, chunks y trims pueden proyectarse como capas virtuales, como hace `editorial_layers.adapters`, pero la proyección se regenera y no se edita directamente. La UI debe escribir en el contrato originario (`marcas`, `trims`, plan o layer) y recalcular la vista.

## Conflictos

`source_master_digest` protege identidad del master; `source_layers_digest` protege el snapshot que leyó la AI; `revision` detecta modificación externa; `edited`/`accepted` protege intención humana. Merge, rechazo y stale son eventos auditables, nunca sobreescritura silenciosa.
