# SSHX — Synthèse des travaux de Jérémy Laviole (fork CATIE)

**Période couverte** : 3 mars 2026 → 15 septembre 2026
**Dépôt** : `github.com/catie-aq/sshx` (fork de `ekzhang/sshx`), branche de travail `claude`
**Volume total** : 12 commits, **~34 400 lignes ajoutées** sur 140 fichiers (dont ~100 fichiers créés), version portée à **0.5.0**

---

## Contexte

sshx est à l'origine un outil de partage de terminal collaboratif via navigateur, chiffré de bout en bout (Argon2id + AES-128-CTR), écrit en Rust (client CLI + serveur Axum/Tonic) avec un frontend SvelteKit/xterm.js. Le dernier commit upstream date de juin 2025 (v0.4.1).

Le fork CATIE transforme cet outil de partage de terminal en **espace de travail collaboratif complet sur canvas infini** : observation de sessions Claude Code en temps réel, cartes de fichiers sources, partage d'écran WebRTC, navigateur déporté, IDE VSCode intégré, notes, dessin, mode diaporama, et outils d'observabilité pour applications web.

L'essentiel du développement a été réalisé en binôme avec Claude Code (co-auteurs successifs : Claude Sonnet 4.6, Claude Opus 4.6, Claude Fable 5).

---

## Chronologie des phases

| Phase | Dates | Commits | Contenu | Livrable détaillé |
|---|---|---|---|---|
| 1. Déploiement & documentation | 3 mars 2026 | `5305c3f`, `dde6437`, `b1f8c36` | Déploiement Tailscale HTTPS, Docker Compose, documentation complète du code (CLAUDE.md + 7 README, 1 362 lignes) | [01](01-deploiement-distribution.md) |
| 2. Analyse workspace & canvas | 5 mars 2026 | `c128d68` (+6 000 l.) | `sshx analyze`, flux d'activité Claude, FileCards, sticky notes, palette de commandes, vue graphe | [02](02-analyse-workspace-et-canvas.md), [03](03-integration-claude-code.md) |
| 3. Partage d'écran & navigateur offscreen | 6 mars 2026 | `a69943e` (+5 557 l.) | WebRTC P2P, crates `sshx-media` et `sshx-browser` (Xvfb + Chromium + VP8) | [04](04-partage-ecran-et-navigateur-offscreen.md) |
| 4. Observabilité web & distribution | 24–25 mars 2026 | `09afeb1` (+6 510 l.), `766961d` | Overlay React/Next.js, scripts de build Linux/macOS, TextBlock (TipTap), tests E2E Playwright | [05](05-observabilite-web.md), [01](01-deploiement-distribution.md) |
| 5. IDE VSCode, persistance, dessin | 1–3 avril 2026 | `ae8a3f8` (très gros), `bbdf3b1`, `138e3ca` | OpenVSCode Server intégré, extension VSCode, persistance de session, DrawingLayer, ClaudeExecutionGraph (1 571 l.), Timeline/diaporama, PKGBUILD Arch | [06](06-ide-vscode.md), [07](07-outils-canvas-texte-dessin-mobile.md) |
| 6. UI & mobile | 9 avril 2026 | `98a7492` | Améliorations UI, gestes tactiles, 471 lignes de tests E2E mobiles | [07](07-outils-canvas-texte-dessin-mobile.md) |
| 7. Administration & partage | 15 sept. 2026 | `6b2b1f1` | API admin `/api/sessions` (token, comparaison temps constant), bouton de partage, gestion des images renommées | [08](08-administration-et-partage.md) |

---

## État d'avancement global

| Feature | État | Remarques |
|---|---|---|
| Déploiement Tailscale + Docker | ✅ Opérationnel | Certificats présents sur le serveur de dev |
| Documentation code (CLAUDE.md, README) | ✅ Complet | Maintenu au fil des features |
| `sshx analyze` + FileCards + canvas | ✅ Fonctionnel | Itérations UX en cours (cf. `tmp.md`) |
| Flux d'activité Claude Code | ✅ Fonctionnel | Ring buffer 200 événements, replay à la reconnexion |
| Graphe d'exécution Claude | ✅ Fonctionnel | Composant de 1 571 lignes, remplace l'ancienne vue graphe |
| Partage d'écran P2P (WebRTC) | ✅ Fonctionnel | Tests d'intégration + E2E Playwright |
| Navigateur offscreen (`sshx-browser`) | ⚠️ Partiel | Ingestion des frames OK ; relais vers les viewers différé |
| Observabilité React/Next.js | ✅ Fonctionnel | Mode dev React uniquement (par conception) |
| IDE VSCode (OpenVSCode Server) | ✅ Fonctionnel | Téléchargement auto, proxy, extension collab, tests dédiés |
| TextBlock / dessin / diaporama | 🔄 En itération | Bugs connus listés dans `tmp.md` (offsets diaporama, reset connexion pencil) |
| Support mobile | ✅ Fonctionnel | Suite de tests E2E mobile dédiée |
| API admin + partage de session | ✅ Fonctionnel | Livré le 15 sept. 2026 |
| Distribution (Linux, macOS, Arch) | ✅ Scripts prêts | Binaire dev v0.5.0 lié depuis la page d'accueil |

### Travaux en cours (non commités au 15 sept. 2026)

Notes de travail dans `tmp.md` :
- Auto-sélection et placeholder à la création d'un TextBlock ;
- UI de sélection des sticky notes (au lieu d'apparaître/disparaître) ;
- Regroupement des couleurs avec les outils Text/Note/Pencil/Highlight, suppression du panneau latéral redondant ;
- Bug « Connection reset » sur l'outil crayon ;
- Mode diaporama : offsets X/Y du recadrage, conservation du déplacement au clic-molette.

---

## Sessions Claude Code retrouvées

Les transcripts des sessions de mars–avril ont été purgés ; il reste deux sessions du **15 septembre 2026** :

1. **Session `0eb006a1`** (12h54) — « *Commit les travaux en cours, juste le code évidemment* » → a produit le commit `6b2b1f1` (API admin, bouton partage, nettoyage des images renommées).
2. **Session `956ce703`** (12h56) — la présente session, production de ces livrables.

Les sessions antérieures sont attestées indirectement par :
- les **trailers de co-auteur** des commits : Claude Sonnet 4.6 (3–5 mars), Claude Opus 4.6 (6 mars), Claude Fable 5 (15 sept.) ;
- la **mémoire persistante Claude** du projet (créée le 5 mars 2026), qui documente les sessions de développement du partage d'écran, du navigateur offscreen et des FileCards ;
- la documentation produite pendant ces sessions (`doc/features/*.md`, `WEBVIEW.md`, `doc/fullstack-feature.md`).

À noter : le flux d'activité Claude intégré à sshx (livrable 03) rend justement ces sessions observables en temps réel — l'outil documente son propre mode de fabrication.
