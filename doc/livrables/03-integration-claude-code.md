# Livrable 03 — Intégration Claude Code (observation temps réel des sessions IA)

**Début** : 5 mars 2026 (`c128d68`) — graphe d'exécution le 1er avril (`ae8a3f8`)
**État** : ✅ Fonctionnel

---

## Description

Rend visibles, dans le navigateur de tous les participants, les sessions **Claude Code** qui tournent dans le répertoire partagé : chaque appel d'outil, réponse et message utilisateur apparaît en direct, sans aucune configuration.

### Pipeline (`crates/sshx/src/workspace.rs`, 837 l. à la création)

```
Claude Code écrit ~/.claude/projects/<cwd-encodé>/*.jsonl
   → sshx CLI : run_claude_tracker() / tail_transcript()
       (relit les 200 dernières lignes au démarrage, puis tail)
   → gRPC ServerUpdate::ClaudeEvent
   → sshx-server : ring buffer 200 événements par session
       (rejoué à chaque connexion WebSocket → historique immédiat)
   → WebSocket CBOR-X → navigateurs
```

- Boucle de découverte robuste : sshx peut être lancé **avant** Claude — le flux s'active dès que Claude démarre (retry toutes les 5 s).
- Tests dédiés : `crates/sshx/tests/claude_tracker.rs` (237 l.).

### Composants UI

| Composant | Rôle |
|---|---|
| `ClaudeActivityFeed.svelte` (327 l.) | Flux d'activité dans le HUD bas-droit (sous le chat) : outils appelés, fichiers touchés, réponses. Code couleur : indigo = actions IA. |
| `ClaudeInstanceList.svelte` | Panneau listant les instances Claude actives (PID) dans le répertoire. |
| `ClaudeExecutionGraph.svelte` (**1 571 l.**, avril) | Vue graphe de l'exécution d'une session : enchaînement des tours, appels d'outils, fichiers modifiés. Remplace l'ancienne GraphView générique. |
| `Timeline.svelte` (avril) | Frise temporelle de la session. |

Documentation associée : `doc/features/claude-interactions.md`, `code-claude-features.md` (variante TypeScript `sessionTracer.ts` pour d'autres applications).

---

## Implications

- **Pair programming humain/IA observable** : toute l'équipe voit ce que fait l'agent en temps réel, dans le même espace que les terminaux et les fichiers — utile pour la supervision, la formation et la confiance dans le travail de l'agent.
- Le ring buffer serveur garantit qu'un participant qui rejoint tard voit l'historique récent sans dépendre du CLI.
- Positionne sshx comme **console d'observation d'agents IA**, un différenciateur fort par rapport au sshx upstream.
- Combiné à la propriété de base de sshx (accès par simple URL, sans SSH ni installation), c'est la brique centrale de la **mise à disposition de Claude Code à des publics non techniques** : l'opérateur prépare la machine, le bénéficiaire n'a qu'un lien à ouvrir et voit l'agent travailler (cf. section « Positionnement » de la synthèse).
- Auto-référence intéressante : les livrables du projet ont eux-mêmes été produits dans des sessions visibles par cet outil.

## Utilisation

```bash
cd mon-projet
sshx            # dans un répertoire où Claude Code travaille (ou travaillera)
```

Rien d'autre à faire : le flux apparaît dans le panneau « Claude Activity » du navigateur ; le graphe d'exécution et la timeline sont accessibles depuis la toolbar.
