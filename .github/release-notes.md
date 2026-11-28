## Installer / Install

| Système / System | Fichier / File |
|---|---|
| Windows 10/11, x64 | `speakwrite-windows-x64.msi` |
| macOS, Apple silicon | `speakwrite-macos-arm64.dmg` |
| Linux x64 | `speakwrite-linux-x64.tar.gz` |

**Français.** Le MSI installe SpeakWrite pour l'utilisateur courant, sans droits administrateur, avec un raccourci dans le menu Démarrer. Sur le DMG, glisser SpeakWrite dans Applications. Les installeurs ne sont pas signés : Windows SmartScreen → « Informations complémentaires » → « Exécuter quand même » ; macOS → Réglages Système → Confidentialité et sécurité → « Ouvrir quand même ». Au premier lancement, l'application télécharge ses modèles, environ 3,5 Go. Dans ces versions, la dictée tourne sur le processeur sous Windows et Linux ; pour une carte NVIDIA, compiler avec CUDA. Détails : [INSTALL.fr.md](https://github.com/abreheret/SpeakWrite/blob/master/INSTALL.fr.md#sans-compiler--les-installeurs).

**English.** The MSI installs SpeakWrite for the current user, without administrator rights, with a Start menu shortcut. From the DMG, drag SpeakWrite into Applications. The installers are not signed: Windows SmartScreen → "More info" → "Run anyway"; macOS → System Settings → Privacy & Security → "Open Anyway". On first launch, the app downloads its models, about 3.5 GB. In these builds, dictation runs on the CPU on Windows and Linux; for an NVIDIA card, build with CUDA. Details: [INSTALL.md](https://github.com/abreheret/SpeakWrite/blob/master/INSTALL.md#without-building-the-installers).

The `.zip` and `.tar.gz` archives hold the executable alone. Le binaire inclut l'encodeur MP3 LAME (LGPL), lié statiquement ; le code source complet de SpeakWrite permet de le relier à une autre version de LAME. / The binary statically links the LAME MP3 encoder (LGPL); the full SpeakWrite source allows relinking it with another LAME version.
