//! Retrait des protections d'un classeur Excel (.xlsx / .xlsm).
//!
//! Un .xlsx est un zip : la « protection » (feuille, structure du classeur,
//! lecture seule recommandée) n'est qu'une balise XML. On la retire et on
//! recopie tout le reste tel quel (formules, styles, graphiques, macros...).

use regex::bytes::Regex;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Signature d'un fichier OLE2 : c'est le format des classeurs *chiffrés*
/// avec un mot de passe d'ouverture (et des anciens .xls).
const SIGNATURE_OLE2: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

#[derive(Debug)]
pub struct Resultat {
    pub sortie: PathBuf,
    pub protections_retirees: usize,
}

fn motifs() -> Vec<Regex> {
    [
        // Protection d'une feuille (auto-fermante ou avec balise de fin)
        r"<sheetProtection\b[^>]*?(?:/>|>.*?</sheetProtection>)",
        // Protection de la structure du classeur
        r"<workbookProtection\b[^>]*?(?:/>|>.*?</workbookProtection>)",
        // « Lecture seule recommandée »
        r"<fileSharing\b[^>]*?(?:/>|>.*?</fileSharing>)",
        // Protection des feuilles de graphique
        r"<chartsheetProtection\b[^>]*?/>",
    ]
    .iter()
    .map(|p| Regex::new(&format!("(?s){p}")).expect("regex valide"))
    .collect()
}

fn a_nettoyer(nom: &str) -> bool {
    nom.ends_with(".xml")
        && (nom == "xl/workbook.xml"
            || nom.starts_with("xl/worksheets/")
            || nom.starts_with("xl/chartsheets/"))
}

/// Chemin de sortie : `nom_delocked.xlsx` à côté du fichier source,
/// sans jamais écraser un fichier existant.
pub fn chemin_sortie(src: &Path) -> PathBuf {
    let dossier = src.parent().unwrap_or_else(|| Path::new("."));
    let nom = src.file_stem().and_then(|s| s.to_str()).unwrap_or("fichier");
    let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("xlsx");

    let mut candidat = dossier.join(format!("{nom}_delocked.{ext}"));
    let mut n = 2;
    while candidat.exists() {
        candidat = dossier.join(format!("{nom}_delocked_{n}.{ext}"));
        n += 1;
    }
    candidat
}

pub fn deverrouiller(src: &Path) -> Result<Resultat, String> {
    // Vérifie d'abord que ce n'est pas un classeur chiffré.
    let mut entete = [0u8; 8];
    File::open(src)
        .and_then(|mut f| f.read_exact(&mut entete))
        .map_err(|e| format!("Lecture impossible : {e}"))?;
    if entete == SIGNATURE_OLE2 {
        return Err(
            "Fichier chiffré par mot de passe d'ouverture (ou ancien format .xls) : \
             impossible sans le mot de passe."
                .into(),
        );
    }

    let fichier = File::open(src).map_err(|e| format!("Ouverture impossible : {e}"))?;
    let mut zip_in = zip::ZipArchive::new(fichier)
        .map_err(|_| "Ce fichier n'est pas un classeur Excel valide.".to_string())?;

    let sortie = chemin_sortie(src);
    let out = File::create(&sortie).map_err(|e| format!("Création impossible : {e}"))?;
    let mut zip_out = zip::ZipWriter::new(out);

    let motifs = motifs();
    let mut total = 0usize;

    let resultat: Result<(), String> = (|| {
        for i in 0..zip_in.len() {
            let mut entree = zip_in.by_index(i).map_err(|e| e.to_string())?;
            let nom = match entree.name() {
                Ok(n) => n.into_owned(),
                Err(_) => String::from_utf8_lossy(entree.name_raw()).into_owned(),
            };

            if a_nettoyer(&nom) {
                let mut donnees = Vec::with_capacity(entree.size() as usize);
                entree.read_to_end(&mut donnees).map_err(|e| e.to_string())?;
                for m in &motifs {
                    let avant = donnees.len();
                    let remplace = m.replace_all(&donnees, &b""[..]).into_owned();
                    if remplace.len() != avant {
                        total += m.find_iter(&donnees).count();
                    }
                    donnees = remplace;
                }
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated);
                zip_out.start_file(nom, options).map_err(|e| e.to_string())?;
                zip_out.write_all(&donnees).map_err(|e| e.to_string())?;
            } else {
                // Copie brute : aucun changement, aucune recompression.
                zip_out.raw_copy_file(entree).map_err(|e| e.to_string())?;
            }
        }
        zip_out.finish().map_err(|e| e.to_string())?;
        Ok(())
    })();

    if let Err(e) = resultat {
        let _ = std::fs::remove_file(&sortie);
        return Err(format!("Échec du traitement : {e}"));
    }

    Ok(Resultat {
        sortie,
        protections_retirees: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Fabrique un mini .xlsx protégé pour les tests.
    fn xlsx_protege(chemin: &Path) {
        let f = File::create(chemin).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("xl/workbook.xml", o).unwrap();
        z.write_all(br#"<workbook><workbookProtection lockStructure="1" workbookHashValue="abc"/><sheets/></workbook>"#).unwrap();
        z.start_file("xl/worksheets/sheet1.xml", o).unwrap();
        z.write_all(br#"<worksheet><sheetData><row><c r="A1"><f>SUM(B1:B2)</f><v>3</v></c></row></sheetData><sheetProtection sheet="1" password="CC1A"/></worksheet>"#).unwrap();
        z.start_file("xl/styles.xml", o).unwrap();
        z.write_all(b"<styleSheet/>").unwrap();
        z.finish().unwrap();
    }

    fn lire(zip_path: &Path, nom: &str) -> String {
        let mut z = zip::ZipArchive::new(File::open(zip_path).unwrap()).unwrap();
        let mut s = String::new();
        z.by_name(nom).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn retire_protections_et_garde_formules() {
        let dir = std::env::temp_dir().join("xlsx_deverr_test");
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("test.xlsx");
        let _ = std::fs::remove_file(dir.join("test_delocked.xlsx"));
        xlsx_protege(&src);

        let r = deverrouiller(&src).unwrap();
        assert_eq!(r.protections_retirees, 2);

        let feuille = lire(&r.sortie, "xl/worksheets/sheet1.xml");
        assert!(!feuille.contains("sheetProtection"));
        assert!(feuille.contains("<f>SUM(B1:B2)</f>"));
        let classeur = lire(&r.sortie, "xl/workbook.xml");
        assert!(!classeur.contains("workbookProtection"));
        assert_eq!(lire(&r.sortie, "xl/styles.xml"), "<styleSheet/>");
    }

    #[test]
    fn refuse_un_fichier_chiffre() {
        let dir = std::env::temp_dir().join("xlsx_deverr_test");
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("chiffre.xlsx");
        let mut contenu = SIGNATURE_OLE2.to_vec();
        contenu.extend_from_slice(&[0u8; 64]);
        std::fs::write(&src, contenu).unwrap();
        assert!(deverrouiller(&src).unwrap_err().contains("chiffré"));
    }

    /// Test manuel sur un vrai fichier : XLSX_TEST_FILE=/chemin/fichier.xlsx cargo test -- --ignored --nocapture
    #[test]
    #[ignore]
    fn fichier_reel() {
        let src = PathBuf::from(std::env::var("XLSX_TEST_FILE").expect("XLSX_TEST_FILE"));
        let r = deverrouiller(&src).unwrap();
        println!("{r:?}");
    }
}
