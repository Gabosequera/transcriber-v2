# ADR-008 — GES no es autoridad de dominio

- Estado: aceptado | Fecha: 2026-09-07

GES ofrece objetos NLE útiles pero tiene API no thread-safe, commit explícito y reglas de solape. V2 mantiene el modelo propio y usa `GesOwner` de un hilo para construir/reproducir/renderizar snapshots. Si GES rechaza un graph, se devuelve error; no se muta silenciosamente el dominio.
