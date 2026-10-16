# Intégration VSCode (OpenVSCode Server)

## Vue d'ensemble

sshx intègre **OpenVSCode Server** (fork Gitpod de VS Code) comme IDE collaboratif accessible depuis le canvas. Le binaire n'est pas embarqué dans sshx : il est téléchargé à la demande sur la machine du CLI.

---

## Activation

```bash
# Télécharge OpenVSCode Server si absent, puis démarre la session
sshx --ide

# Ou pointer un binaire existant
sshx --openvscode-bin /path/to/openvscode-server

# Ou via variable d'environnement
SSHX_OPENVSCODE_BIN=/path/to/openvscode-server sshx
```

Sans `--ide` ou `--openvscode-bin`, la fonctionnalité IDE est désactivée et aucun widget IDE n'est proposé dans le canvas.

### Sessions inscrites sur SSHX Platform

Une session inscrite sur la plateforme (`sshx platform enroll`) propose l'IDE sans `--ide`
(`Controller::enable_ide_install`, désactivable par `--no-ide`) :

- si la version épinglée est déjà installée, elle est utilisée directement ;
- sinon, la première requête IDE lance `ensure_binary()` en tâche de fond et reçoit une page
  d'attente (`ide::install_page`, HTTP 503, rechargée toutes les 2 s) ; un échec est affiché une fois
  (HTTP 502), la requête suivante relance l'installation.

Le bouton « Ouvrir dans VS Code » de la plateforme ouvre `/ide/s/{session}/1/?tkn=…&folder=<dossier du
terminal>` dans un nouvel onglet : une seule instance OpenVSCode par session, une fenêtre par dossier.

---

## Téléchargement automatique — `crates/sshx/src/openvscode.rs`

| Propriété | Valeur |
|---|---|
| Version pinned | `1.109.5` |
| Source | `github.com/gitpod-io/openvscode-server/releases` |
| Répertoire d'installation | `~/.sshx/openvscode-server/` |
| Marqueur de version | `~/.sshx/openvscode-server/.version` |
| Plateformes supportées | `linux-x64`, `linux-arm64` (Gitpod ne publie rien pour macOS) |

Sur macOS, sshx lance `serve-web` du CLI officiel de VS Code : celui d'un Visual Studio Code installé,
sinon le CLI autonome téléchargé dans `~/.sshx/vscode-cli/` (`openvscode::installed_binary`,
`is_vscode_cli`). Mêmes options (`--server-base-path`, `--connection-token-file`, `--default-folder`),
plus `--accept-server-license-terms` (licence Microsoft). Le CLI télécharge le serveur (~600 Mo) dans
`~/.sshx/vscode-web/` à la première requête, derrière sa propre page d'attente. Il pose deux cookies
(`vscode-tkn`, `vscode-cli-secret-half`) : le tunnel transporte donc les en-têtes répétés, joints par
des sauts de ligne (`tunnel.rs`) puis séparés par sshx-server (`ide_proxy.rs`). L'IDE est lancé dans
son propre groupe de processus, arrêté en entier quand sshx s'arrête.

L'extraction est atomique : le tarball est extrait dans un répertoire `.tmp`, puis un `rename` atomique remplace l'ancienne installation. Un éventuel répertoire `.tmp` résiduel (crash précédent) est nettoyé au démarrage.

---

## Flux d'initialisation

```
sshx --ide
  │
  ├─ ensure_binary()          → télécharge si absent ou version mismatch
  ├─ spawn_sync_server()      → serveur HTTP Axum sur port libre (127.0.0.1)
  └─ gRPC hello: "{name},{token},ide"
       └─ serveur: set_ide_available(true)
              └─ broadcast WsServer::IdeAvailable(true) → tous les browsers
```

---

## Lancement de l'IDE — `crates/sshx/src/ide.rs`

### IdeManager

Chaque widget IDE dans le canvas a un `ide_id: u32` unique. À la première requête HTTP proxy pour cet `ide_id`, un `IdeManager` est créé et `spawn()` est appelé.

`spawn()` :
1. Injecte les paramètres VS Code par défaut dans `~/.sshx/vscode-data/User/settings.json` (seulement si absent) :
   - `keyboard.dispatch = "keyCode"` — compatibilité claviers non-QWERTY
   - `terminal.integrated.enabled = false` — désactive le terminal intégré (les terminaux sshx le remplacent)
2. Trouve un port libre (`TcpListener::bind("127.0.0.1:0")`)
3. Lance `openvscode-server` avec :
   ```
   --port <port>
   --without-connection-token
   --host 127.0.0.1
   --server-base-path /ide/s/{session}/{ide_id}
   --default-folder {workspace_dir}
   --user-data-dir ~/.sshx/vscode-data
   --extensions-dir <bundled extensions>   # si disponible
   ```
4. Variables d'environnement pour l'extension :
   - `SSHX_SYNC_PORT` — port du serveur de sync local
   - `SSHX_IDE_ID` — identifiant de l'instance IDE

### Serveur de sync (sync server)

Un second serveur HTTP Axum tourne sur un port libre distinct, accessible uniquement en local :

| Endpoint | Rôle |
|---|---|
| `POST /state` | Reçoit l'état éditeur de l'extension, le relaie via gRPC |
| `GET /events` | SSE stream : pousse les états distants à l'extension |
| `POST /lock` | Demande de verrouillage d'édition |
| `POST /unlock` | Libération du verrou |

Le paramètre `?ide_id=N` sur `/events` supprime l'auto-écho : l'extension ne reçoit pas ses propres événements.

---

## Tunnel HTTP — `crates/sshx/src/tunnel.rs` + `crates/sshx-server/src/web/ide_proxy.rs`

Le browser ne communique jamais directement avec le processus OpenVSCode. Tout passe par le tunnel gRPC :

```
Browser iframe → GET /ide/s/{name}/{id}/...
                       │
             sshx-server (ide_proxy.rs)
                       │  HttpTunnelRequest (gRPC)
             sshx CLI (controller.rs)
                       │
             IdeManager → http://127.0.0.1:{port}
                            (OpenVSCode Server)
```

- **HTTP** : la requête est sérialisée en `HttpTunnelRequest`, envoyée via gRPC, la réponse est réassemblée côté serveur (timeout 30 s).
- **WebSocket** (pour le hot-reload VS Code) : upgrade détecté, relayé bidirectionnellement via `WsTunnelFrame`.

---

## Extension de collaboration — `extensions/sshx-collab/`

Une extension VS Code est incluse dans le dépôt et chargée automatiquement via `--extensions-dir`.

Elle lit `SSHX_SYNC_PORT` et `SSHX_IDE_ID` au démarrage, puis :

- **`POST /state`** (debounced) : envoie les onglets ouverts, le fichier actif, les curseurs, sélections, ranges visibles, état sidebar/panel.
- **`GET /events`** (SSE) : reçoit l'état des autres instances IDE, affiche les curseurs distants.
- **`POST /lock`** : quand l'utilisateur commence à taper, demande un verrou d'édition au serveur.
- **Status bar** : affiche l'état du verrou (`🔒 locked by …`).

---

## Protocole WebSocket — messages IDE

### Serveur → Browser

| Message | Contenu |
|---|---|
| `IdeAvailable(bool)` | Indique si le CLI a un binaire OpenVSCode |
| `IdeStates(Vec<(Wid, WsIdeState)>)` | Snapshot envoyé à la connexion |
| `IdeStateDiff(Wid, Option<WsIdeState>)` | Mise à jour incrémentale |

### Browser → Serveur

| Message | Contenu |
|---|---|
| `OpenIdeEditor(x, y, workspace)` | Crée un nouveau widget IDE sur le canvas |
| `UpdateIdeState(Wid, WsIdeState)` | Envoie l'état local depuis le browser |

### WsIdeState

```typescript
{
  openFiles: string[];
  activeFile: string | null;
  cursors: [string, number, number][];        // [path, line, col]
  selections: [string, number, number, number, number][];
  visibleRanges: [string, number, number][];
  sidebarVisible: boolean;
  sidebarView: string | null;
  panelVisible: boolean;
  workspaceFolder: string | null;
  workspacePath: string | null;
  ideId: number;
}
```

---

## Widget frontend — `src/lib/ui/IdeEditorWidget.svelte`

Simple `<iframe>` avec `src="/ide/s/{sessionName}/{ideId}/"` et sandbox :
```
allow-scripts allow-same-origin allow-forms allow-popups allow-downloads
```

Affiche en superposition :
- Les **dots de curseurs** des autres utilisateurs ayant cet IDE ouvert.
- Un **badge de verrou** indiquant qui détient l'edit lock.
- Les **CircleButtons** (fermer / minimiser / maximiser).

---

## Données persistantes sur le client

| Chemin | Contenu |
|---|---|
| `~/.sshx/openvscode-server/` | Binaire OpenVSCode Server |
| `~/.sshx/openvscode-server/.version` | Version installée |
| `~/.sshx/vscode-data/` | User data VS Code (settings, extensions state, history) |

---

## Limites et non-inclus

- Authentification IDE : protégée uniquement par l'accès à la session sshx (E2E encryption).
- Un seul `user-data-dir` partagé entre toutes les instances IDE de la session.
- Le terminal intégré VS Code est désactivé intentionnellement.
- Pas de persistance des `ide_states` dans `SerializedSession` (perdu au restart serveur).
- Support plateforme : Linux et macOS uniquement (pas Windows).
