# Livrable 04 — Partage d'écran P2P & navigateur offscreen

**Début** : 6 mars 2026 (`a69943e`, +5 557 lignes) — améliorations le 24 mars (`09afeb1`)
**État** : ✅ Partage d'écran fonctionnel · ⚠️ Navigateur offscreen partiel (relais viewers différé)

---

## Description

### Feature A — Partage d'écran entre participants (WebRTC P2P)

- Bouton « Screen » dans la toolbar → `getDisplayMedia` (écran, fenêtre ou onglet, audio best-effort).
- Le flux apparaît comme un **widget vidéo sur le canvas** (`ScreenShareWidget.svelte`), comme un terminal.
- Plusieurs partages simultanés possibles (type `Vid(u32)` par flux) ; chaque participant choisit les flux qu'il regarde (`WatchStream`/`UnwatchStream`).
- Transport **WebRTC P2P** : le serveur sshx ne relaie que la signalisation (`RtcOffer/Answer/Ice` sur le WebSocket existant) — il ne voit jamais les médias. STUN Google par défaut, TURN configurable.
- Arrêt propre pour tous via message `CloseStream`.
- Corrections notables : race condition `ontrack` (écran noir côté viewer).

### Feature B — Navigateur offscreen (`sshx-browser` + `sshx-media`)

Deux nouveaux crates :

| Crate | Contenu |
|---|---|
| `sshx-media` | Bibliothèque partagée : capture X11 (`xcap`), encodage VP8 (subprocess ffmpeg), gestion Xvfb, injection clavier/souris via XTest (`x11rb`). |
| `sshx-browser` | Binaire : lance Chromium dans un display virtuel Xvfb, capture ~30 fps, streame en VP8 vers le serveur via un nouveau service gRPC `BrowserService` (Join/Stream/Leave). |

- Contrôle exclusif : un participant à la fois peut piloter (souris + clavier) via `RequestBrowserControl`/`BrowserInput` ; les événements sont injectés dans le display X11.
- Auth par token HMAC transmis par l'opérateur ; flag `--with-browser` sur le CLI sshx pour un lancement facile.
- Corrections encodeur : compat libvpx ≥ 1.15 (`-auto-alt-ref 0` pour BGRA), rendu Chrome sous Xvfb (retrait de `--disable-gpu`).
- **Limite actuelle** : l'ingestion des frames (sshx-browser → serveur) fonctionne ; le **relais vers les navigateurs viewers est différé** (`store_browser_frame` est un no-op) — le serveur devra agir en SFU (`webrtc-rs` prévu derrière une feature flag).

### Qualité

- Tests d'intégration serveur : `crates/sshx-server/tests/screen_share.rs` (451 l.).
- Tests E2E Playwright (24 mars) : `tests/e2e/p2p-screen-share.spec.ts`, `server-stream.spec.ts` + helpers (génération de frames VP8, client gRPC de test, serveur de couleurs).

Documentation : `doc/features/screenshare.md` (spécification des besoins, en français), `WEBVIEW.md` (guide d'installation et d'usage).

---

## Implications

- La session sshx devient un **espace de réunion technique complet** : terminaux + écrans partagés + navigateur commun, toujours sans que le serveur n'accède aux contenus (P2P pour les écrans).
- Le navigateur offscreen ouvre des cas d'usage type **démo/test collaboratif d'applications web** hébergées près du serveur, pilotables à tour de rôle.
- L'architecture média (capture/encode/inject) étant dans `sshx-media`, elle est réutilisable pour d'autres sources vidéo.

## Utilisation

```bash
# Partage d'écran : bouton "Screen" dans la toolbar du navigateur.

# Navigateur offscreen (nécessite xvfb, chromium, ffmpeg) :
sshx --with-browser
# ou manuellement :
sshx-browser --server http://... --session <name> --token <hmac> --url https://example.com
```
