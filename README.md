# Déverrouilleur Excel

Petit programme (Windows / macOS) : glissez un ou plusieurs classeurs `.xlsx` / `.xlsm`
protégés, il génère pour chacun un `nom_deverrouille.xlsx` modifiable, à côté de l'original.
Formules, mise en forme, graphiques et onglets sont conservés ; l'original n'est jamais modifié.

Limite : un fichier chiffré par mot de passe d'ouverture ne peut pas être ouvert sans ce mot de passe.

## Compiler

    cargo build --release        # -> target/release/xlsx-deverrouilleur(.exe)
    cargo test                   # tests de la logique

Les exécutables Windows et macOS (universel Intel + Apple Silicon) sont produits par
GitHub Actions (`.github/workflows/build.yml`) : onglet *Actions* → *Build* → *Run workflow*,
puis télécharger les artefacts.
