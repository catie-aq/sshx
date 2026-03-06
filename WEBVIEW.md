# WEBVIEW — Navigateur déporté dans une session sshx

`sshx-browser` est un binaire qui lance Chromium dans un affichage virtuel (Xvfb),
capture l'écran à ~30 fps et le diffuse en temps réel dans une session sshx.
Les participants voient le navigateur comme un widget sur le canvas ; l'un d'eux peut
en prendre le contrôle (clavier + souris) depuis son propre navigateur.

---

## Prérequis

| Dépendance | Rôle |
|---|---|
| `xvfb` (`Xvfb`) | Affichage virtuel X11 |
| `chromium` ou `google-chrome` | Navigateur lancé dans Xvfb |
| `ffmpeg` (avec `libvpx`) | Encodage VP8 des frames capturées |
| `x11rb` (Rust, crate) | Injection clavier/souris via XTest |

Installation rapide sur Debian/Ubuntu :

```bash
sudo apt install xvfb chromium ffmpeg
```

Sur Arch/Manjaro :

```bash
sudo pacman -S xorg-server-xvfb chromium ffmpeg
```

> `sshx-browser` ne fonctionne que sur Linux (X11 requis).

---

## Lancement rapide (méthode recommandée)

La façon la plus simple est d'utiliser le flag `--with-browser` du client `sshx`.
Il lance automatiquement `sshx-browser` avec les bons identifiants de session :

```bash
# Lancer une session sshx avec un navigateur sur example.com
cargo run -p sshx -- --server http://localhost:8051 --with-browser https://example.com

# Ouvrir une appli web spécifique
cargo run -p sshx -- --server http://localhost:8051 --with-browser https://grafana.local:3000

# Avec un navigateur spécifique
cargo run -p sshx -- --server http://localhost:8051 \
  --with-browser https://monapp.local \
  --browser-bin /usr/bin/chromium
```

La sortie affiche :

```
  sshx v0.4.1

  ➜  Link:  http://localhost:8051/s/abcde12345#clefchiffrement
  ➜  Shell: /bin/bash
  ➜  sshx-browser launched (pid 12345)
```

Le widget **"Browser"** apparaît automatiquement sur le canvas pour tous les
participants connectés.

### Lancer n'importe quelle appli web

`--with-browser` accepte n'importe quelle URL :

```bash
# Dashboard Grafana
sshx --server http://localhost:8051 --with-browser http://grafana:3000

# Interface Jupyter
sshx --server http://localhost:8051 --with-browser http://localhost:8888

# Application React en développement
sshx --server http://localhost:8051 --with-browser http://localhost:3000

# Documentation interne
sshx --server http://localhost:8051 --with-browser https://wiki.interne.local

# N'importe quel site public
sshx --server http://localhost:8051 --with-browser https://github.com
```

> Le navigateur déporté tourne sur la même machine que `sshx`. Il peut donc
> accéder aux services locaux (`localhost`, réseau interne) qui ne seraient
> pas accessibles depuis le navigateur des participants.

---

## Compiler `sshx-browser`

```bash
cargo build -p sshx-browser --release
# Binaire : target/release/sshx-browser
```

Le binaire doit se trouver à côté du binaire `sshx` ou dans le PATH pour que
`--with-browser` le trouve automatiquement.

---

## Lancement manuel (méthode avancée)

Si vous voulez lancer `sshx-browser` séparément (par exemple sur une machine
différente), il faut fournir le token d'authentification manuellement.

### Démarrer le serveur avec un secret fixé

```bash
SSHX_SECRET=monsecret cargo run -p sshx-server -- --listen 0.0.0.0
```

### Calculer le token d'authentification

```bash
SESSION=abcde12345
SECRET=monsecret

TOKEN=$(echo -n "$SESSION" | openssl dgst -sha256 -hmac "$SECRET" -binary | base64)
echo "Token : $TOKEN"
```

### Lancer `sshx-browser`

```bash
./target/release/sshx-browser \
  --server   http://localhost:8051 \
  --session  "$SESSION" \
  --token    "$TOKEN" \
  --url      https://example.com
```

---

## Ce que voient les participants

Dès la connexion de `sshx-browser`, un widget **"Browser"** apparaît sur le canvas.
Le flux VP8 est automatiquement relayé par le serveur à tous les viewers connectés.

- Le widget se met à jour en temps réel (~30 fps)
- **Bouton Control** : prend le contrôle exclusif clavier/souris
- **Bouton Release** : libère le contrôle
- **Bouton rouge (×)** : ferme le stream pour **tout le monde**

> Le flux vidéo VP8 transite par le serveur (gRPC → WebSocket), ce n'est pas
> du WebRTC P2P contrairement au screen share utilisateur.

---

## Prendre le contrôle du navigateur

1. Cliquer **Control** dans la barre titre du widget Browser
2. Le bandeau **"In control"** apparaît en haut à droite du widget
3. Cliquer dans la zone vidéo pour lui donner le focus
4. Taper normalement : les frappes sont transmises au navigateur déporté via XTest
5. Cliquer **Release** pour rendre le contrôle disponible

> Seul un utilisateur avec accès en écriture peut demander le contrôle.
> Un seul utilisateur contrôle à la fois.

---

## Redimensionnement du widget

### Le widget dans le navigateur (côté viewer)

Le widget peut être **redimensionné librement** par les participants en tirant
le coin en bas à droite. Le redimensionnement est synchronisé entre tous les
participants (tout le monde voit la même taille).

Cependant, le redimensionnement du widget **ne change pas** la résolution du
display virtuel Xvfb : l'image est simplement étirée/réduite via CSS
(`object-contain`). La résolution native reste celle définie au lancement
(`--width` / `--height`, par défaut 1280×720).

### La résolution du display virtuel

La résolution Xvfb est **fixée au démarrage** de `sshx-browser` et ne peut pas
être changée à chaud. Pour une résolution différente :

```bash
# Résolution Full HD
sshx --server http://localhost:8051 --with-browser https://example.com
# (utilise 1280×720 par défaut)

# Lancement manuel avec résolution personnalisée
./target/release/sshx-browser \
  --server http://localhost:8051 \
  --session "$SESSION" \
  --token "$TOKEN" \
  --url https://example.com \
  --width 1920 \
  --height 1080
```

> **En résumé** : le widget se resize visuellement, mais la résolution réelle
> du navigateur déporté est fixe. Pour changer la résolution, il faut relancer
> `sshx-browser` avec les nouvelles dimensions.

---

## Référence des options

```
sshx-browser [OPTIONS]

  --server    <URL>          URL du serveur sshx  [env: SSHX_SERVER]
  --session   <NOM>          Nom de la session sshx
  --token     <TOKEN>        Token HMAC-SHA256 (Base64) de la session
  --url       <URL>          URL initiale à charger dans Chromium
                             [défaut: https://example.com]
  --width     <PX>           Largeur de l'affichage virtuel  [défaut: 1280]
  --height    <PX>           Hauteur de l'affichage virtuel  [défaut: 720]
  --fps       <N>            Images par seconde             [défaut: 30]
  --display-num <N>          Numéro du display Xvfb         [défaut: 99]
  --bitrate-kbps <N>         Débit vidéo VP8 en kbps        [défaut: 2000]
  --browser <BINAIRE>        Binaire du navigateur à utiliser
                             [env: SSHX_BROWSER]
                             [ex: google-chrome-stable, chromium, /usr/bin/chrome]
                             Par défaut : premier trouvé dans PATH parmi
                             google-chrome-stable → google-chrome → chromium → …
```

Options du client `sshx` liées au navigateur :

```
sshx [OPTIONS]

  --with-browser <URL>       Lancer sshx-browser avec l'URL donnée
                             Le token et la session sont transmis automatiquement
  --browser-bin <BINAIRE>    Binaire du navigateur [env: SSHX_BROWSER]
                             Transmis à sshx-browser via --browser
```

---

## Dépannage

| Symptôme | Cause probable | Remède |
|---|---|---|
| `could not find a browser` | Aucun navigateur trouvé dans PATH | `sudo apt install google-chrome-stable` ou `--browser /chemin/vers/chrome` |
| `BrowserService.Join RPC failed` | Token invalide ou session inconnue | Vérifier `SESSION` et `SECRET` |
| Écran gris sans contenu | Chrome avec `--disable-gpu` ne rend pas sur Xvfb | Ne pas ajouter `--disable-gpu` manuellement ; sshx-browser utilise les bons flags |
| `Transparency encoding with auto_alt_ref` | libvpx refuse BGRA+auto_alt_ref | Corrigé dans le code (flag `-auto-alt-ref 0` ajouté) |
| `Server is already active for display 99` | Ancien Xvfb encore actif | `pkill Xvfb; rm -f /tmp/.X99-lock` ou utiliser `--display-num 100` |
| Widget affiche "Connecting..." | Frames VP8 n'arrivent pas | Vérifier les logs sshx-browser pour des erreurs ffmpeg |
| Clavier non injecté | `xtest` non disponible | Vérifier que `xorg-server-xvfb` inclut XTest |
