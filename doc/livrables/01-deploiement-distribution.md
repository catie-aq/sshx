# Livrable 01 — Déploiement, distribution & documentation

**Début** : 3 mars 2026 (`5305c3f`) — compléments les 24 mars (`09afeb1`) et 1er avril (`bbdf3b1`)
**État** : ✅ Opérationnel

---

## Description

Ensemble des travaux d'industrialisation du fork : déploiement HTTPS interne via Tailscale, environnements Docker, scripts de packaging multi-plateformes, et documentation exhaustive du code pour l'onboarding humain et IA.

### Déploiement Tailscale (`Tailscale.md`, `compose.yaml`)

- Exposition d'une instance sshx sur le tailnet CATIE avec **MagicDNS + certificats Let's Encrypt** émis par Tailscale (`<machine>.<tailnet>.ts.net`), sans avertissement navigateur.
- Chaîne : appareil tailnet → Vite dev server (TLS, port 5173) → proxy `/api` → `sshx-server` (8051) → Redis (12601, localhost).
- Deux workflows documentés : conteneurisé (Docker Compose) et manuel (Rust + Node locaux).
- `Dockerfile.backend-dev` et `compose-dev.yaml` pour le développement.

### Distribution (`distribution/`)

- `linux/build.sh` (144 l.) et `macos/build.sh` : construction des binaires de release.
- `arch/PKGBUILD` : paquet Arch Linux.
- `.env.example` : configuration type (token admin, serveur, etc.).
- Version du workspace portée à **0.5.0** ; binaire de dev lié depuis la page d'accueil (15 sept.).

### Documentation (`b1f8c36`, 1 362 lignes)

- `CLAUDE.md` : guide maître du dépôt (architecture, fichiers clés, protocoles, guide d'extension, principes UI) — sert de contexte permanent aux sessions Claude Code.
- README par dossier : `crates/`, `sshx-core`, `sshx`, `sshx-server`, `src/`, `src/lib/`, `src/lib/ui/`.
- Docs de référence ajoutées ensuite : `doc/sshx-cli.md`, `doc/sshx-server.md`, `doc/svelte-frontend.md`, `doc/fullstack-feature.md` (guide pas-à-pas pour ajouter une feature full-stack).

---

## Implications

- L'instance de dev est accessible de manière sécurisée depuis n'importe quel appareil du tailnet — démonstrations et tests multi-appareils (mobile inclus) sans exposition publique.
- La documentation systématique est un **multiplicateur pour le développement assisté par IA** : chaque session Claude démarre avec une carte complète du code, ce qui explique la vélocité des phases suivantes (~6 000 lignes/commit).
- Les scripts de distribution préparent une diffusion des binaires au-delà de la machine de dev.

## Utilisation

```bash
# Dev local multi-processus
mprocs

# Déploiement tailnet : voir Tailscale.md (génération certs + compose.yaml)
docker compose up

# Packaging
distribution/linux/build.sh
distribution/macos/build.sh
```
