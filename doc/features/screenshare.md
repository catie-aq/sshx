# Partage d'écran & Navigateur Offscreen — Besoins utilisateurs

## Feature 1 : Partage d'écran depuis les navigateurs

### Besoin utilisateur
N'importe quel participant à une session sshx peut partager son écran (et son audio) avec les autres.
Plusieurs utilisateurs peuvent partager simultanément.

### Comportement attendu
- Bouton "Screen" dans la barre d'outils. Clic → demande de permission au navigateur (`getDisplayMedia`).
- L'utilisateur choisit : écran entier, fenêtre ou onglet.
- Le flux apparaît comme un widget sur le canvas (comme un terminal ou une FileCard).
- Tous les autres participants voient la liste des flux actifs et peuvent choisir de regarder.
- Plusieurs personnes peuvent partager en même temps (chacune a son propre `Vid`).
- Audio inclus (best-effort selon OS/navigateur).
- Arrêt : clic sur "Stop Share" ou fermeture du widget.

### Contraintes techniques
- Transport : WebRTC P2P (signalisation via le WebSocket sshx existant).
- Le serveur sshx ne touche pas aux paquets média (relay de signalisation uniquement).
- Nécessite STUN (configuré par défaut sur `stun.l.google.com`).
- TURN optionnel pour les réseaux d'entreprise / 4G (configurable dans `ServerOptions`).

---

## Feature 2 : Navigateur offscreen (sshx-browser)

### Besoin utilisateur
Un navigateur Chromium tourne côté serveur dans un display virtuel (Xvfb).
Les participants peuvent le regarder et l'un d'eux à la fois peut le piloter (souris + clavier).

### Comportement attendu
- `sshx-browser` est un binaire Rust séparé, lancé manuellement par l'opérateur :
  ```
  sshx-browser --server http://... --session <name> --token <hmac> --url https://example.com
  ```
- Le flux vidéo apparaît dans la session sshx comme un widget canvas.
- Tous les participants voient le navigateur en temps réel (~30 fps).
- Un seul utilisateur à la fois peut avoir le contrôle (bouton "Prendre le contrôle").
- Le contrôleur envoie ses événements souris/clavier → injectés dans le display X11 via XTest.
- V1 : un seul navigateur par session, pas de gestion avancée de la concurrence.

### Contraintes techniques
- Xvfb + Chromium gérés par `sshx-browser` (pas de Docker).
- Capture d'écran : crate `xcap` (Rust pur, wrapping X11).
- Encodage vidéo : subprocess ffmpeg (VP8).
- Injection clavier/souris : `x11rb` avec extension XTest (bas niveau, pas de subprocess).
- Streaming : le serveur sshx agit comme SFU WebRTC pour les flux venant de `sshx-browser`
  (using `webrtc-rs` crate, isolé derrière une feature flag Cargo).
- Auth : token HMAC transmis du CLI sshx à sshx-browser par l'opérateur.

---

## Module vidéo partagé : `sshx-media`

Nouveau crate `crates/sshx-media/` utilisé par les deux features :
- Abstraction de capture d'écran (`FrameSource` trait).
- Wrapper encodage VP8 (ffmpeg subprocess).
- Gestion du cycle de vie Xvfb.
- Injection input X11 (XTest).

---

## Non-inclus en V1
- Audio pour le navigateur offscreen.
- TURN serveur intégré (configurable externellement).
- Multi-navigateurs par session.
- Gestion de la concurrence de contrôle avancée (file d'attente, timeout).
- Redis mesh pour les flux vidéo.
