# Livrable 06 — IDE collaboratif VSCode (OpenVSCode Server)

**Début** : 1er avril 2026 (`ae8a3f8`) — finitions les 3 et 9 avril
**État** : ✅ Fonctionnel, testé

---

## Description

Intègre **OpenVSCode Server** (fork Gitpod de VS Code) comme widget IDE sur le canvas sshx : un vrai VS Code, tournant sur la machine du CLI, accessible à tous les participants à travers le tunnel sshx existant.

### Architecture côté CLI (`crates/sshx/`)

| Module | Rôle |
|---|---|
| `openvscode.rs` (196 l.) | Téléchargement à la demande du binaire (version pinnée `1.109.5`, GitHub Releases Gitpod), installation atomique dans `~/.sshx/openvscode-server/`, 4 plateformes (linux/darwin × x64/arm64). |
| `ide.rs` (468 l.) | `IdeManager` : un process OpenVSCode par widget (`ide_id`), spawn à la première requête proxy. |
| `tunnel.rs` (398 l.) | Tunnel HTTP entre le serveur sshx et l'IDE local — le trafic IDE passe par la connexion sshx existante, aucun port supplémentaire à exposer. |

### Côté serveur & frontend

- `web/ide_proxy.rs` (382 l.) : proxy HTTP/WebSocket des requêtes IDE vers le CLI.
- `IdeEditorWidget.svelte` : le widget iframe IDE sur le canvas, avec le chrome de fenêtre standard (drag, resize, boutons).
- Annonce de disponibilité : hello gRPC `"{name},{token},ide"` → broadcast `WsServer::IdeAvailable` à tous les navigateurs.

### Extension VSCode `sshx-collab` (`extensions/sshx-collab/`)

Extension (428 l. à la création) embarquée dans l'IDE pour la dimension collaborative (suivi de la session sshx depuis l'éditeur).

### Persistance de session (même commit)

Gros chantier connexe : `session/snapshot.rs` étendu (+276 l.) et suite de tests `session_persistence.rs` (535 l.) — l'état des sessions (widgets, notes, etc.) survit aux redémarrages/reprises.

### Qualité

Tests serveur dédiés `tests/ide.rs` (287 l.) ; documentation `doc/features/vscode-integration.md` (194 l., ajoutée le 3 avril).

---

## Implications

- Complète la gamme d'outils du canvas : **terminal + fichiers + écran + IDE complet**, le tout dans une session chiffrée unique. Un participant peut éditer dans VS Code pendant que les autres observent, sans installation côté client.
- Le binaire IDE n'est **pas embarqué** dans sshx (téléchargé à la demande) : binaire sshx léger, mise à jour de l'IDE indépendante.
- Le mécanisme de tunnel HTTP générique (`tunnel.rs`) est réutilisable pour exposer d'autres services locaux dans la session.

## Utilisation

```bash
sshx --ide                                   # télécharge et lance OpenVSCode si besoin
sshx --openvscode-bin /path/to/openvscode    # binaire existant
SSHX_OPENVSCODE_BIN=... sshx                 # via variable d'environnement
```

Sans ces options, la fonctionnalité est désactivée et aucun widget IDE n'est proposé.
