# Livrable 02 — Analyse de workspace & widgets du canvas

**Début** : 5 mars 2026 (`c128d68`, +6 000 lignes)
**État** : ✅ Fonctionnel — itérations UX en cours

---

## Description

Cette phase transforme le canvas infini de sshx (jusque-là réservé aux terminaux) en **espace de travail visuel partagé** représentant le code du projet.

### Analyse de sources — `sshx analyze` (`crates/sshx/src/analyze.rs`, 566 l.)

- Nouvelle sous-commande CLI qui analyse le workspace et produit `.sshx-analysis.json` : métadonnées structurelles par fichier (imports, exports, nombre de lignes, descriptions).
- Streaming temps réel vers les navigateurs via un nouveau message gRPC `SourceMetadata`, relayé en WebSocket.

### Widgets sur le canvas (frontend Svelte)

| Widget | Fichier | Rôle |
|---|---|---|
| **FileCard** | `FileCard.svelte` (865 l.) | Carte flottante par fichier source : métadonnées, graphe d'imports, description, aperçu d'image. Upload d'images via `POST /api/s/{name}/upload`. |
| **FileTreePanel** | `FileTreePanel.svelte` | Panneau HUD repliable : arborescence des fichiers analysés, recherche, badges de type, clic-pour-naviguer. |
| **Library cards** | — | Cartes d'usage des bibliothèques tierces dans le code. |
| **Sticky notes** | `StickyNote.svelte` | Post-its colorés avec rendu Markdown (`marked`). |
| **Command palette** | `CommandPalette.svelte` | Palette Cmd+K façon Spotlight : navigation vers terminaux et FileCards. |
| **Vue graphe** | `GraphView`/`GraphOverlay` | Graphe de dépendances runtime superposé au canvas (remplacé en avril par le ClaudeExecutionGraph, cf. livrable 03). |

### Protocole

~15 nouveaux types de messages `WsServer`/`WsClient` (Rust) avec leurs miroirs TypeScript ; extension du proto gRPC.

### Évolutions ultérieures

- **1er avril** (`ae8a3f8`) : `WsFileMetadataUpdate` — position, taille et image d'une FileCard persistées dans `.sshx-analysis.json` via gRPC ; les cartes retrouvent leur disposition d'une session à l'autre.
- **15 sept.** (`6b2b1f1`) : images sauvegardées dans `<cwd>/.sshx/images/` avec assainissement des noms ; renommer une image depuis l'UI supprime l'ancien fichier côté CLI.

---

## Implications

- Le canvas devient une **carte vivante du projet** : on peut disposer côte à côte le terminal, les fichiers concernés par le travail en cours, des notes et des captures — le tout synchronisé entre tous les participants et chiffré de bout en bout.
- La persistance des métadonnées dans le workspace (`.sshx-analysis.json`, `.sshx/images/`) rend la disposition **versionnable avec le projet**.
- C'est le socle sur lequel s'appuient l'observabilité web (livrable 05 : « Show in SSHX » ouvre une FileCard) et le suivi Claude (livrable 03).

## Utilisation

```bash
sshx analyze          # génère/rafraîchit .sshx-analysis.json
sshx                  # la session streame les métadonnées aux navigateurs
```

Dans le navigateur : bouton « analyze » de la toolbar, Cmd+K pour la palette, glisser-déposer des cartes, upload d'image sur une FileCard.
