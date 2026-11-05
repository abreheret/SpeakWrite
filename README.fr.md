[English](README.md) | **Français**

# SpeakWrite

Outil de bureau **local** pour Windows, Linux et macOS, qui fonctionne dans les deux sens :

- **Lecture** : sélectionner un texte n'importe où, appuyer sur un raccourci, et il est lu à voix haute, avec en option un enregistrement MP3.
- **Dictée** : maintenir une touche, parler, et le texte s'écrit dans le champ actif.

Les deux sens reposent sur les modèles open source de [Kyutai](https://kyutai.org) et tournent sur la machine, sans service en ligne.

La lecture reconnaît la langue du texte, ligne par ligne : français et anglais par défaut. Chaque langue est lue par son propre modèle Pocket TTS, avec la même voix, y compris la vôtre.

## État du projet

Phase 4 : **finitions**.

| Brique | Statut | Windows | Linux | macOS |
|---|---|---|---|---|
| Lecture : F9 lire, F10 pause, Maj+F10 arrêter (Pocket TTS, voix clonée) | ✅ Fonctionne | ✅ | ⏳ Non testée | ✅ |
| Enregistrement MP3 des lectures (Maj+F9, ou chaque lecture) | ✅ Fonctionne | ✅ | ⏳ | ⏳ |
| Dictée : F8 maintenu (Kyutai STT, streaming) | ✅ Fonctionne | ✅ CUDA | ⏳ (CUDA / CPU) | ✅ Metal |
| Spike : Kyutai TTS 1.6B `tts-1.6b-en_fr` (Python) | ✅ Validé | ✅ CUDA | ⏳ (CUDA) | ⏳ (MLX) |

`legacy/` contient l'ancien prototype (AutoHotkey + PowerShell + edge-tts, Windows uniquement).

## Fonctionnement

```
speakwrite (Rust, un seul exécutable, icône dans la barre système)
├─ LECTURE : raccourci → copie de la sélection → Kyutai Pocket TTS → haut-parleurs (+ MP3 en option)
└─ DICTÉE  : touche maintenue → micro → Kyutai STT (streaming) → texte écrit dans le champ actif
```

Briques Rust :

| Rôle | Crate |
|---|---|
| Boucle d'événements | `tao` |
| Raccourcis globaux (appui et relâchement) | `global-hotkey` |
| Icône, menu, boîtes de dialogue | `tray-icon` (+ `muda`), `rfd` |
| Presse-papier, frappe simulée | `arboard`, `enigo` |
| Lecture audio, capture micro | `rodio`, `cpal` |
| Voix : serveur local officiel `pocket-tts serve`, piloté en HTTP | `ureq` |
| Langue du texte à lire | `lingua` |
| Dictée : inférence STT | `moshi` + `candle` (CUDA / Metal / processeur) |
| Décodage audio, encodage MP3 | `symphonia`, `mp3lame-encoder` |
| Configuration | `serde`, `toml`, `toml_edit`, `directories` |

Organisation de `app/src` :

| Module | Rôle |
|---|---|
| `main.rs` | Boucle d'événements, raccourcis, menu, commandes de diagnostic |
| `reader.rs` | Thread de lecture. Chaque demande porte un numéro de génération : un nouveau F9 ou un arrêt annule la précédente |
| `selection.rs` | Ctrl+C simulé (Cmd+C sur macOS), attente du presse-papier, restauration. Sélection primaire sous Linux |
| `language.rs` | Découpe le texte en suites de lignes d'une même langue. Les lignes courtes (titres) suivent la ligne longue d'après ; un texte indécidable prend la première langue |
| `textnorm/` | Écriture en toutes lettres pour le français : nombres, heures, ordinaux, %, €, touches F1 à F24, dictionnaire de prononciation. Autres langues : Markdown et espaces seulement, leurs modèles lisent eux-mêmes les chiffres |
| `tts/` | Interface `TtsEngine`, un moteur par langue démarré à la demande, moteur Pocket TTS, téléchargement vérifié du binaire, lecture du WAV en streaming |
| `tray.rs` | Icône et menu (voix, langue, vitesse, MP3, dictée), reconstruit à chaque changement |
| `voices.rs` | Catalogue : voix fournies et enregistrements du dossier `voices/`, import d'un nouveau fichier, renommage |
| `voiceprep.rs` | Prépare chaque enregistrement à cloner : mono, longues pauses raccourcies, les 25 s les plus parlées, filtre passe-haut, volume harmonisé, mise en cache. Pocket TTS prend sinon le fichier entier : 4 min ont mené à une allocation de 73 Go et à un plantage |
| `stretch.rs` | Vitesse de lecture sans changer la hauteur de la voix : étirement temporel WSOLA en streaming, en Rust pur |
| `audio.rs` | Lecture sans coupure entre les morceaux, pause, arrêt instantané |
| `recorder.rs` | MP3 (LAME) encodé pendant la synthèse dans un fichier `.part`, renommé à la fin : jamais de fichier tronqué |
| `dictation.rs` | Thread de dictée : le modèle reste chargé, le micro ne s'ouvre que pendant l'appui, la capture continue 300 ms après le relâchement |
| `stt.rs` | Kyutai STT `stt-1b-en_fr` via le crate `moshi`. Mots définitifs, environ 0,5 s après la parole |
| `mic.rs`, `resample.rs` | Capture micro, passage en mono, rééchantillonnage à 24 kHz (sinc fenêtré) |
| `inject.rs` | Écriture dans le champ actif : frappe Unicode, ou collage avec restauration du presse-papier |
| `process.rs` | Le serveur de voix meurt avec l'app, même en cas de plantage (job object Windows, `PR_SET_PDEATHSIG` Linux) |
| `config.rs` | Fichier TOML commenté, emplacements standard de chaque OS, migration des anciens fichiers |

### Contraintes par plateforme

| | Windows | Linux | macOS |
|---|---|---|---|
| Accélération | CUDA (NVIDIA) | CUDA (NVIDIA) | Metal (Apple silicon) |
| Raccourcis globaux | OK | X11 : OK. Wayland : portail `GlobalShortcuts` selon le bureau | Autorisations « Surveillance de l'entrée » et « Accessibilité » |
| Injection du texte | OK, sauf dans les fenêtres lancées en administrateur (UIPI) | X11 : OK. Wayland : `wtype` / `ydotool`, ou presse-papier | Autorisation « Accessibilité » |
| Icône système | OK | `libayatana-appindicator` + GTK | OK |
| Micro | OK | ALSA / PulseAudio / PipeWire | Autorisation « Micro » |

## Application

### Compiler et lancer

**Guide pas à pas, d'une machine vierge jusqu'au premier lancement : [INSTALL.fr.md](INSTALL.fr.md)**. Il couvre les outils, la compilation, les modèles téléchargés, la voix personnelle et les problèmes fréquents.

Les scripts activent tout seuls l'accélération graphique de la dictée : CUDA si `nvcc` est présent, Metal sous macOS, processeur sinon.

```sh
scripts\build.cmd                    # Windows : charge l'environnement Visual Studio, CUDA si présent
./scripts/build.sh                   # Linux / macOS
./target/release/speakwrite          # Windows : target\release\speakwrite.exe
```

Sans script : `cargo build --release -p speakwrite --features cuda`, ou `--features metal`, ou aucune option pour le processeur.

Prérequis :
- Rust ≥ 1.88 et CMake ;
- Windows : Visual Studio Build Tools (C++) ; CUDA Toolkit 12.x pour la dictée sur la carte graphique ;
- Linux : `build-essential cmake pkg-config libssl-dev libgtk-3-dev libayatana-appindicator3-dev libxdo-dev libasound2-dev`.

Au premier lancement, l'app :
1. crée le fichier de configuration ;
2. télécharge le binaire officiel `pocket-tts` (version v4.0.1, empreinte SHA-256 vérifiée) ;
3. le démarre en serveur local, un par langue de lecture (français, puis anglais) ; chacun télécharge les poids de son modèle, environ 430 Mo ;
4. télécharge le modèle de dictée Kyutai STT, environ 2,4 Go, depuis le cache Hugging Face.

Couleurs de l'icône :
- grise : démarrage ;
- bleue : prête ;
- verte : lecture ;
- ambre : pause ;
- orange : écoute (dictée) ;
- rouge : erreur.

### Utilisation

| Touche | Action |
|---|---|
| **F9** | Lit le texte sélectionné dans n'importe quelle application. Sans sélection, lit le presse-papier |
| **F10** | Met la lecture en pause, ou la reprend. La synthèse continue d'avancer pendant la pause, donc la reprise est immédiate |
| **Maj+F10** | Arrête la lecture. Un nouvel appui sur F9 pendant une lecture passe à la nouvelle sélection |
| **Maj+F9** | Lit la sélection **et l'enregistre en MP3**, tel qu'on l'entend (voix, vitesse) : `2026-11-05 14-32-10 Premiers mots du texte.mp3`. Une lecture interrompue n'est pas enregistrée |
| **F8**, maintenu | Dictée. Le texte s'écrit au fil de la parole, ponctuation et majuscules comprises. La lecture en cours s'arrête, pour que le micro ne l'entende pas |

Le **menu de l'icône** règle tout, sans toucher au fichier de configuration :
- **Voix** :
  - ses propres enregistrements ;
  - les 27 voix fournies par Kyutai ;
  - « Ajouter une voix… », avec un sélecteur de fichier ;
  - « Ouvrir le dossier des voix ».

  Changer de voix redémarre les moteurs (environ 3 s), puis fait entendre un exemple.
- **Langue** : automatique (français et anglais, ligne par ligne), ou toujours la même. Appliquée tout de suite, avec un exemple.
- **Vitesse** : de 0,8× à 2×, appliquée tout de suite, avec un exemple.
- **Enregistrement MP3** :
  - enregistrer aussi chaque lecture F9 ;
  - ouvrir le dernier enregistrement ou le dossier ;
  - choisir le dossier (par défaut `Musique/SpeakWrite`).
- **Dictée** :
  - écrire au fil de la parole, ou tout coller au relâchement, pour les applications qui ignorent la frappe simulée ;
  - ajouter une espace après chaque dictée.
- **Écouter un exemple**, langue du menu (`Menu in English` / `Menu en français`), réglages avancés (fichier), recharger, journaux, quitter.

Chaque choix est enregistré dans `config.toml`, en conservant les commentaires du fichier.

### Configuration

| OS | Fichier de configuration | Journaux, binaire pocket-tts |
|---|---|---|
| Windows | `%APPDATA%\SpeakWrite\config\config.toml` | `%LOCALAPPDATA%\SpeakWrite\data\` |
| macOS | `~/Library/Application Support/SpeakWrite/config.toml` | `~/Library/Application Support/SpeakWrite/` |
| Linux | `~/.config/speakwrite/config.toml` | `~/.local/share/speakwrite/` |

Le fichier est commenté. Il règle :
- les raccourcis ;
- le comportement du presse-papier ;
- la vitesse de lecture (`[playback] speed`) ;
- l'enregistrement MP3 (`[recording]` : activation, dossier, débit de 32 à 160 kbit/s) ;
- la dictée (`[dictation]` : touche, mode d'insertion, matériel, modèle) ;
- la langue de lecture (`[pocket] language` : `"auto"` ou une langue fixe) et les langues reconnues en mode `"auto"` (`auto_languages`, français et anglais par défaut ; aussi allemand, espagnol, italien, portugais et néerlandais). Chaque langue fait tourner son propre modèle, 0,5 à 0,8 Go de mémoire ;
- la voix.

Il contient aussi le dictionnaire de prononciation, dans la section `[pronunciation]`, appliqué aux textes en français.

**Pour lire avec sa propre voix** :
1. Accepter les conditions sur [`kyutai/pocket-tts`](https://huggingface.co/kyutai/pocket-tts), puis lancer `uvx --from huggingface_hub hf auth login`.
2. Menu de l'icône → Voix → « Ajouter une voix… ».

N'utiliser que sa propre voix, ou celle d'une personne qui a donné son accord explicite.

### Diagnostic

```sh
speakwrite --normalize "Appuie sur F9 : 3,5 % en 2026"   # affiche le texte tel qu'il sera lu, avec sa langue
speakwrite --say "Bonjour"                               # lit une phrase et mesure la latence
speakwrite --transcribe audio.wav                        # passe un fichier par la chaîne de dictée
SPEAKWRITE_SAY_WAV=sortie.wav speakwrite --say "..."     # enregistre aussi l'audio reçu (WAV)
SPEAKWRITE_SAY_MP3=sortie.mp3 speakwrite --say "..."     # idem en MP3
SPEAKWRITE_LOG=debug speakwrite                          # journaux détaillés
```

Mesures sous Windows 11, RTX 4090 :

| Lecture (Pocket TTS, processeur, voix clonée) | Dictée (Kyutai STT, CUDA) |
|---|---|
| Moteur prêt en 3 à 5 s (français), anglais 3 s plus tard | Modèle chargé en environ 1,4 s une fois en cache |
| Premier son au bout d'environ 100 ms | Mots environ 0,5 s après la parole |
| Génération environ 6 fois plus rapide que le temps réel | Calcul environ 8 fois plus rapide que le temps réel ; derniers mots environ 0,45 s après le relâchement |

## Spikes (phase 1)

### Prérequis

Communs : [Rust](https://rustup.rs), [uv](https://docs.astral.sh/uv/), CMake ≥ 3.5, environ 10 Go d'espace disque pour les modèles (cache Hugging Face).

- **Windows** : Visual Studio 2022 ou Build Tools avec le composant « Développement Desktop en C++ ». CUDA Toolkit 12.x, facultatif (sinon tout tourne sur le processeur).
- **Linux** : `build-essential`, `cmake`, `pkg-config`, `libportaudio2`. CUDA Toolkit 12.x, facultatif.
- **macOS** : Xcode Command Line Tools, `cmake` (Homebrew). Apple silicon nécessaire pour MLX.

### Dictée : `spikes/stt-rs`

Exemple officiel de Kyutai (Rust + Candle). Il choisit tout seul CUDA, puis Metal, puis le processeur.

```sh
# Windows
spikes\stt-rs\build.cmd
spikes\stt-rs\target\release\kyutai-stt-rs.exe chemin\vers\audio.mp3

# Linux / macOS
./spikes/stt-rs/build.sh
./spikes/stt-rs/target/release/kyutai-stt-rs chemin/vers/audio.mp3
```

Options : `--cpu` (forcer le processeur), `--timestamps` (horodatage des mots), `--vad` (détection de fin de phrase).

### Lecture 1.6B : `spikes/tts-kyutai`

Scripts officiels de Kyutai. `uv` installe la bonne version selon le système :
- Windows : PyTorch CUDA 12.6 ;
- Linux : PyTorch depuis PyPI (CUDA inclus) ;
- macOS : MLX.

```sh
cd spikes/tts-kyutai
uv sync

# Windows / Linux (GPU NVIDIA)
uv run tts_pytorch.py texte_test.txt sortie.wav
uv run tts_pytorch.py texte_test.txt -          # lecture directe sur les haut-parleurs

# macOS (Apple silicon)
uv run tts_mlx.py texte_test.txt sortie.wav
```

Voix françaises (option `--voice`, chemins relatifs au dépôt [`kyutai/tts-voices`](https://huggingface.co/kyutai/tts-voices)) :
- `unmute-prod-website/developpeuse-3.wav` (voix par défaut)
- `unmute-prod-website/fabieng-enhanced-v2.wav`
- `cml-tts/fr/*.wav`, environ 70 locuteurs, chacun aussi disponible en version `_enhanced` (nettoyée)

### Mesures (Windows 11, RTX 4090)

| Test | Résultat |
|---|---|
| STT : 16,3 s de parole | 3,6 s au total, **chargement du modèle compris**. Ponctuation et majuscules correctes. Erreurs : noms propres rares (« Kyutai » → « QT »), homophones sans contexte (« où » → « ou »). |
| TTS 1.6B : 17,4 s d'audio | Générées en 5,6 à 7,9 s, sans `torch.compile` (Triton n'existe pas sous Windows). Chiffres et sigles bien prononcés : la transcription STT du fichier produit est quasi identique au texte d'origine. |

### Clonage de voix

- **TTS 1.6B : impossible.** Le module qui transforme un enregistrement en empreinte vocale (`speaker_wavs.compression_model`) n'est volontairement pas publié. Seules les voix précalculées de `kyutai/tts-voices` sont utilisables.
- **Pocket TTS : possible**, officiellement, en local et sur le processeur. C'est le moteur de l'app.

## Feuille de route

1. ✅ **Validation** : Kyutai STT, TTS 1.6B, Pocket TTS avec sa propre voix.
2. ✅ **Lecture**
   - icône, raccourcis, capture de la sélection ;
   - écriture en toutes lettres ;
   - Pocket TTS en streaming ;
   - voix et vitesse dans le menu ;
   - pause ;
   - enregistrement MP3 ;
   - reconnaissance automatique de la langue (français / anglais).
3. ✅ **Dictée** : Kyutai STT en streaming, texte écrit dans le champ actif.
4. ⏸️ **TTS 1.6B dans l'app** : bloqué en pur Rust. Le modèle publié utilise `demux_second_stream` et un *weights-per-step schedule* que le crate `moshi` 0.6.1 n'implémente pas, et Kyutai ne publie pas de poids convertis pour Rust. Pistes : un serveur Python lancé par l'app, ou un fork du crate `moshi`.
5. 🔜 **Finitions**
   - démarrage automatique avec la session ;
   - reprise si la sortie audio change (casque branché en cours de route) ;
   - tests automatiques TTS → STT sur un corpus de phrases ;
   - mise à jour vers les dernières crates après `rustup update` (tray-icon 0.26 exige Rust 1.90) ;
   - tests sous Linux et macOS ;
   - paquets par OS.

## Licences

- Code SpeakWrite : **MIT**, voir [LICENSE](LICENSE).
- Encodeur MP3 : LAME, via `mp3lame-encoder` / `mp3lame-sys`, **LGPL**, lié statiquement. Le code source reste sous MIT, mais un binaire de SpeakWrite diffusé doit respecter la LGPL pour LAME, c'est-à-dire permettre de le relier avec une autre version de LAME.
- Reconnaissance de la langue : `lingua`, licence Apache-2.0.
- `app/src/stt.rs` et `spikes/stt-rs` : dérivés de [kyutai-labs/delayed-streams-modeling](https://github.com/kyutai-labs/delayed-streams-modeling), licences MIT / Apache-2.0 (fichiers inclus dans `spikes/stt-rs`).
- `spikes/tts-kyutai` : scripts Kyutai adaptés, licence MIT (fichier inclus).
- Modèles Kyutai (`stt-1b-en_fr`, `tts-1.6b-en_fr`, Pocket TTS) : **CC-BY 4.0**, citer Kyutai en cas de diffusion.
- Voix de `kyutai/tts-voices` : licence propre à chaque jeu de données (voir le dépôt Hugging Face).
