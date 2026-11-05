[English](INSTALL.md) | **Français**

# Installer SpeakWrite

Ce guide part d'une machine vierge : outils, compilation, modèles, premier lancement.

## En bref

```sh
git clone https://github.com/abreheret/SpeakWrite.git
cd SpeakWrite
scripts\build.cmd                    # Windows
./scripts/build.sh                   # Linux, macOS
target\release\speakwrite.exe        # Windows ; ailleurs : ./target/release/speakwrite
```

Au premier lancement, l'application télécharge ses modèles, environ 3,5 Go. L'icône de la barre système passe du gris au bleu quand la lecture est prête.

Il n'y a rien à télécharger à la main, et aucun compte n'est nécessaire, sauf pour lire avec sa propre voix (voir la [section 5](#5-lire-avec-sa-propre-voix-optionnel)).

## 1. Prérequis

| | Windows | Linux | macOS |
|---|---|---|---|
| Système | Windows 10 ou 11, x64 | x86_64 ou aarch64 | Intel ou Apple silicon |
| État | ✅ Testé | ⏳ Non testé | ⏳ Non testé |
| Dictée sur la carte graphique | NVIDIA (CUDA) | NVIDIA (CUDA) | Apple silicon (Metal) |

Windows sur ARM n'est pas pris en charge : il n'existe pas de binaire pocket-tts pour cette plateforme.

Matériel :
- **Mémoire vive** : 8 Go minimum, 16 Go conseillés. L'application en utilise environ 2,2 Go : 0,7 Go pour elle-même et 0,7 Go par langue de lecture.
- **Carte graphique** : facultative. La dictée occupe environ 2,6 Go de mémoire graphique en CUDA. Sans carte graphique, elle tourne sur le processeur, plus lentement (non mesuré).
- **Lecture** : elle tourne toujours sur le processeur, environ 6 fois plus vite que le temps réel.
- **Disque** :
  - environ 3,5 Go pour les modèles ;
  - environ 8 Go pendant la compilation (dossier `target/`, effaçable ensuite) ;
  - 67 Mo pour l'exécutable.
- **Réseau** : uniquement au premier lancement (GitHub et Hugging Face). Ensuite, les modèles sont lus depuis le cache local.

## 2. Installer les outils

Il faut :
- Git ;
- Rust 1.88 ou plus récent ;
- CMake 3.5 ou plus récent (CMake 4 convient) ;
- un compilateur C/C++.

CUDA est facultatif.

### Windows

Dans un terminal PowerShell :

```powershell
winget install Git.Git
winget install Kitware.CMake
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive --wait"
```

Le dernier paquet installe le compilateur C++ de Microsoft (charge de travail « Développement Desktop en C++ »). Une installation complète de Visual Studio 2022 avec cette charge de travail convient aussi.

**Facultatif, pour la dictée sur carte NVIDIA :**
- installer un pilote NVIDIA à jour ;
- installer le [CUDA Toolkit 12.x](https://developer.nvidia.com/cuda-toolkit-archive), testé avec la 12.6 ;
- vérifier dans un nouveau terminal que `nvcc --version` répond.

Fermer puis rouvrir le terminal pour prendre en compte le `PATH`, puis vérifier :

```powershell
rustc --version   # 1.88 ou plus
cmake --version
```

### Linux (Debian, Ubuntu)

```sh
sudo apt install build-essential cmake pkg-config git curl libssl-dev \
    libgtk-3-dev libayatana-appindicator3-dev libxdo-dev libasound2-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Sur les autres distributions, installer les paquets équivalents : compilateur C++, CMake, pkg-config, OpenSSL, GTK 3, Ayatana AppIndicator, libxdo et ALSA, en versions de développement.

Facultatif : CUDA Toolkit 12.x et un pilote NVIDIA, avec `nvcc` dans le `PATH`.

### macOS

```sh
xcode-select --install
brew install cmake
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Sur Apple silicon, la dictée utilise automatiquement Metal.

## 3. Récupérer et compiler

```sh
git clone https://github.com/abreheret/SpeakWrite.git
cd SpeakWrite
scripts\build.cmd          # Windows
./scripts/build.sh         # Linux, macOS
```

Les scripts choisissent l'accélération eux-mêmes :
- **Windows** : `build.cmd` charge l'environnement Visual Studio, puis compile avec CUDA si `nvcc` est trouvé.
- **Linux** : `build.sh` compile avec CUDA si `nvcc` est trouvé.
- **macOS** : `build.sh` compile avec Metal.
- **Sinon** : la dictée passe sur le processeur.

Les dossiers `legacy/` et `spikes/` ne sont pas nécessaires.

La première compilation prend plusieurs minutes, davantage avec CUDA qui compile ses noyaux. Les suivantes sont rapides. Le fichier `Cargo.lock` du dépôt fige les versions des dépendances : vous compilez exactement les mêmes que l'auteur.

Sans les scripts :

```sh
cargo build --release -p speakwrite                    # processeur
cargo build --release -p speakwrite --features cuda    # NVIDIA (Windows : depuis un terminal « x64 Native Tools »)
cargo build --release -p speakwrite --features metal   # Apple silicon
```

Pour compiler avec CUDA sur une machine sans carte NVIDIA, indiquer la génération de la carte cible, par exemple `CUDA_COMPUTE_CAP=89` pour une RTX 40xx ou `86` pour une RTX 30xx.

## 4. Les modèles

Tout se télécharge automatiquement au premier lancement :

| Élément | Rôle | Taille | Source | Compte requis |
|---|---|---|---|---|
| `pocket-tts` 3.3.0 | Serveur local de lecture | 22 Mo | [GitHub, kyutai-labs/pocket-tts](https://github.com/kyutai-labs/pocket-tts/releases/tag/v4.0.1), empreinte SHA-256 vérifiée | Non |
| Pocket TTS, un modèle par langue | Lecture avec les voix fournies | Environ 430 Mo par langue | [`kyutai/pocket-tts-without-voice-cloning`](https://huggingface.co/kyutai/pocket-tts-without-voice-cloning) | Non |
| Pocket TTS avec clonage | Lecture avec votre voix | Environ 300 Mo par langue | [`kyutai/pocket-tts`](https://huggingface.co/kyutai/pocket-tts) | Oui, conditions à accepter |
| Kyutai STT `stt-1b-en_fr-candle` | Dictée | 2,4 Go | [`kyutai/stt-1b-en_fr-candle`](https://huggingface.co/kyutai/stt-1b-en_fr-candle) | Non |
| Reconnaissance de la langue (`lingua`) | Choix du modèle de lecture | Incluse dans l'exécutable | — | — |

Par défaut, deux langues sont chargées : le français et l'anglais. Le français est prêt en premier ; l'anglais se charge ensuite en arrière-plan.

Emplacements :

| | Modèles (cache Hugging Face) | Binaire pocket-tts, journaux |
|---|---|---|
| Windows | `%USERPROFILE%\.cache\huggingface\hub` | `%LOCALAPPDATA%\SpeakWrite\data\` |
| Linux | `~/.cache/huggingface/hub` | `~/.local/share/speakwrite/` |
| macOS | `~/.cache/huggingface/hub` | `~/Library/Application Support/SpeakWrite/` |

La variable d'environnement `HF_HOME` déplace le cache Hugging Face, par exemple sur un autre disque.

Pour tout télécharger d'avance, par exemple avant de travailler hors ligne :
1. lancer l'application une fois ;
2. attendre l'icône bleue, puis encore une minute, le temps que l'anglais finisse de charger.

## 5. Lire avec sa propre voix (optionnel)

Le clonage de voix demande un accès au dépôt `kyutai/pocket-tts`, qui est gratuit mais soumis à conditions :

1. Créer un compte sur [huggingface.co](https://huggingface.co).
2. Ouvrir [kyutai/pocket-tts](https://huggingface.co/kyutai/pocket-tts) et accepter les conditions.
3. Enregistrer un jeton d'accès sur la machine :
   ```sh
   uvx --from huggingface_hub hf auth login
   ```
   Cette commande demande [uv](https://docs.astral.sh/uv/). Variante : `pip install -U huggingface_hub`, puis `hf auth login`. Le jeton est enregistré dans le cache Hugging Face, où pocket-tts et SpeakWrite le trouvent.
4. Menu de l'icône → Voix → « Ajouter une voix… », puis choisir 10 à 30 secondes de parole naturelle (WAV, MP3, M4A, FLAC ou OGG).

N'utilisez que votre propre voix, ou celle d'une personne qui a donné son accord explicite.

Sans accès, la lecture échoue avec une erreur `403` / `GatedRepoError` dans les journaux. Les voix fournies, elles, fonctionnent toujours.

## 6. Premier lancement et vérification

1. Lancer `target/release/speakwrite`. L'icône apparaît dans la barre système : grise au démarrage, bleue quand la lecture est prête.
2. **Lecture** : sélectionner un texte dans n'importe quelle application, puis appuyer sur **F9**.
3. **Dictée** : cliquer dans un champ de texte, maintenir **F8**, parler, puis relâcher.

Tous les réglages sont dans le menu de l'icône. Le fichier de configuration s'ouvre depuis « Réglages avancés ». Les raccourcis et les réglages sont décrits dans le [README](README.fr.md#utilisation).

Commandes de diagnostic :

```sh
speakwrite --say "Bonjour"                 # lit une phrase et mesure la latence
speakwrite --normalize "Appuie sur F9"     # affiche le texte tel qu'il sera lu, avec sa langue
speakwrite --transcribe audio.wav          # passe un fichier par la chaîne de dictée
```

Sous Windows, l'exécutable de la version release n'a pas de console. Pour voir sa sortie, passer par un tube :
- dans PowerShell : `.\target\release\speakwrite.exe --say "Bonjour" | Out-Host` ;
- dans cmd : `target\release\speakwrite.exe --say Bonjour | more`.

## 7. Problèmes fréquents

| Symptôme | Cause et solution |
|---|---|
| `Visual Studio C++ build tools not found` | Installer la charge de travail « Développement Desktop en C++ » (section 2) |
| `nvcc not found: dictation will run on the CPU` | CUDA absent ou pas dans le `PATH`. Ce n'est pas bloquant : la dictée tourne sur le processeur |
| Une touche ne réagit pas ; les journaux indiquent `cannot register hotkey` | Une autre application utilise déjà la touche (AutoHotkey, logiciel de capture…). La fermer, ou changer la touche dans `[hotkeys]` |
| `403` ou `GatedRepoError` | Voix clonée sans accès à `kyutai/pocket-tts` : voir la section 5, ou choisir une voix fournie |
| Premier lancement très long | Téléchargement des modèles (environ 3,5 Go). La progression est dans les journaux, menu → « Ouvrir les journaux » |
| La dictée n'écrit rien dans certaines fenêtres (Windows) | Les fenêtres lancées en administrateur refusent la frappe simulée. Lancer SpeakWrite en administrateur aussi, ou passer en mode collage (Dictée → « Coller tout au relâchement ») |
| Linux : pas d'icône | GNOME demande l'extension AppIndicator. Sous Wayland, les raccourcis globaux et la frappe simulée dépendent du bureau ; X11 fonctionne |
| macOS : touches ou dictée sans effet | Autoriser SpeakWrite dans Réglages Système → Confidentialité et sécurité : « Accessibilité », « Surveillance de l'entrée », « Micro » |

Les journaux sont dans `logs/`, à côté du binaire pocket-tts (section 4) :
- `speakwrite.log.<date>` pour l'application ;
- `pocket-tts-<langue>.log` pour chaque serveur de lecture.

## Désinstaller

Il suffit de supprimer :
- le dossier du dépôt ;
- la configuration : `%APPDATA%\SpeakWrite` sous Windows, `~/.config/speakwrite` sous Linux, `~/Library/Application Support/SpeakWrite` sous macOS ;
- les données : `%LOCALAPPDATA%\SpeakWrite` sous Windows, `~/.local/share/speakwrite` sous Linux ;
- dans le cache Hugging Face, les dossiers `models--kyutai--pocket-tts*` et `models--kyutai--stt-1b-en_fr-candle`.
