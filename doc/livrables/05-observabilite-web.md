# Livrable 05 — Observabilité web (React / Next.js)

**Début** : 24 mars 2026 (`09afeb1`)
**État** : ✅ Fonctionnel (mode développement React, par conception)

---

## Description

Pont entre une application web en développement et la session sshx : on **survole un élément dans l'application React**, l'overlay affiche le composant et son fichier source, et « Show in SSHX » ouvre la **FileCard correspondante sur le canvas collaboratif**.

### Composants (`observability/nextjs/`, servis aussi par le serveur sshx)

| Fichier | Rôle |
|---|---|
| `sshx-overlay.js` (~490 l.) | Overlay injecté dans l'app cible : lit `_debugSource` dans les internals React fiber (dev uniquement), surligne l'élément survolé, affiche composant + fichier:ligne. |
| `sshx-connect.js` (~340 l.) | Connexion WebSocket vers la session sshx : envoie les demandes « Show in SSHX ». |
| `inject-layout.ts` | Helper d'injection pour Next.js. |
| `AppOverlayWidget.svelte` | Widget côté canvas sshx représentant l'application observée. |

- Les scripts sont **servis directement par le serveur sshx** (`static/sshx-connect.js`, `static/sshx-overlay.js`) : intégration sans copie de fichiers via `NEXT_PUBLIC_SSHX_SERVER` + deux balises `<Script>` dans le layout Next.js. Option B : copie/symlink dans `public/`.
- Documentation fournie : `observability/nextjs/README.md`, `README-REACT.md`, et guides détaillés `doc/observability/NextJS.md` (892 l.) et `ReactJS.md` (394 l.).

---

## Implications

- Boucle **UI ↔ code** instantanée en revue collaborative : un participant montre un élément d'interface, tous voient le fichier source concerné apparaître sur le canvas — particulièrement efficace combiné au flux Claude (l'agent modifie le fichier, on voit l'élément et la carte côte à côte).
- Limité au mode dev React (dépendance à `_debugSource`), ce qui est cohérent avec l'usage visé : sessions de développement, pas de production.
- Le pattern est générique : d'autres frameworks pourraient être supportés en ajoutant un extracteur de source équivalent.

## Utilisation

```bash
# Dans l'app Next.js (.env.local) :
NEXT_PUBLIC_SSHX_SERVER=https://mon-serveur-sshx:8051
```

```tsx
// app/layout.tsx (dev uniquement)
<Script src={`${sshxServer}/sshx-overlay.js`} strategy="afterInteractive" />
<Script src={`${sshxServer}/sshx-connect.js`} strategy="afterInteractive" />
```

Lancer l'app en dev (`npm run dev`), ouvrir la session sshx, survoler un élément → « Show in SSHX ».
