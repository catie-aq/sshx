# Livrable 07 — Outils de canvas : texte riche, dessin, diaporama, mobile

**Début** : 24 mars 2026 (`09afeb1`, `766961d`) — dessin/diaporama le 1er avril (`ae8a3f8`), mobile le 9 avril (`98a7492`)
**État** : 🔄 Fonctionnel, en itération active (bugs connus listés dans `tmp.md`)

---

## Description

Ensemble d'outils qui font évoluer le canvas vers un **tableau blanc collaboratif** complet, utilisable aussi en présentation.

### Texte riche — `TextBlock.svelte` + TipTap (24–25 mars)

- Blocs de texte libres sur le canvas, éditeur **TipTap** (nouvelle dépendance), synchronisés entre participants et persistés côté serveur.
- `OpenFileDialog.svelte` : ouverture de fichiers du workspace ; `ContextMenu.svelte` : menu contextuel du canvas.
- `ImageWidget.svelte` : images posées librement sur le canvas (upload serveur).

### Dessin — `DrawingLayer` + `DrawingToolbar` (1er avril)

- Calque de dessin au-dessus du canvas : **crayon et surligneur**, choix de couleurs, annotations visibles par tous.
- `history.ts` (undo/redo) et `selection.ts` (sélection multiple).

### Présentation — `SlideRegion` + `Timeline` (1er avril)

- **Mode diaporama** : régions du canvas définies comme diapositives, navigation séquentielle — la disposition de travail devient un support de présentation.
- `FontSelector.svelte` + `fonts.ts` : choix de police par élément.

### Mobile & tactile (9 avril)

- Gestes tactiles retravaillés (`touchZoom.ts` : pinch-zoom, pan), toolbar et XTerm adaptés aux petits écrans.
- Suite E2E dédiée : `tests/e2e/mobile.spec.ts` (471 l.) sur Playwright.

---

## Travaux en cours (notes `tmp.md`, non commités)

- TextBlock : auto-sélection + placeholder à la création.
- Sticky notes : véritable UI de sélection.
- Toolbar : couleurs regroupées avec Text/Note/Pencil/Highlight en sous-ligne, suppression du panneau latéral redondant.
- Bug « Connection reset » sur l'outil crayon.
- Diaporama : offsets X/Y du recadrage au clic, conservation du pan au clic-molette.

---

## Implications

- sshx couvre désormais le spectre **atelier technique → restitution** : on travaille sur le canvas (terminaux, fichiers, notes, annotations) puis on le présente directement en mode diaporama, sans export vers un outil de slides.
- Le support mobile permet de **suivre une session depuis une tablette/un téléphone** (revue, démonstration), ce que valide la suite de tests dédiée.
- Les annotations dessinées et les textes riches rendent les sessions utilisables pour la formation et les revues d'architecture.

## Utilisation

Depuis la toolbar du navigateur : outils Text / Note / Pencil / Highlight, insertion d'images, définition de régions de diapositives puis navigation au clavier ; pinch-zoom et pan sur mobile.
