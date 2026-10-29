// Pas de console noire sur Windows en version release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod unprotect;

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
enum Etat {
    EnAttente,
    Ok { sortie: PathBuf, retirees: usize },
    Erreur(String),
}

#[derive(Clone)]
struct Ligne {
    source: PathBuf,
    etat: Etat,
}

#[derive(Default)]
struct App {
    lignes: Arc<Mutex<Vec<Ligne>>>,
}

fn est_excel(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("xlsx") | Some("xlsm")
    )
}

impl App {
    /// Ajoute des fichiers et lance leur traitement en parallèle.
    fn ajouter(&self, ctx: &egui::Context, chemins: Vec<PathBuf>) {
        let mut a_traiter: Vec<(usize, PathBuf)> = Vec::new();
        {
            let mut lignes = self.lignes.lock().unwrap();
            for chemin in chemins {
                let idx = lignes.len();
                if est_excel(&chemin) {
                    lignes.push(Ligne { source: chemin.clone(), etat: Etat::EnAttente });
                    a_traiter.push((idx, chemin));
                } else {
                    lignes.push(Ligne {
                        source: chemin,
                        etat: Etat::Erreur("Format non pris en charge (.xlsx / .xlsm uniquement)".into()),
                    });
                }
            }
        }
        if a_traiter.is_empty() {
            return;
        }

        // Un petit nombre de threads qui se partagent la file de travail.
        let file = Arc::new(Mutex::new(a_traiter));
        let nb = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).min(4);
        for _ in 0..nb {
            let file = Arc::clone(&file);
            let lignes = Arc::clone(&self.lignes);
            let ctx = ctx.clone();
            std::thread::spawn(move || loop {
                let tache = file.lock().unwrap().pop();
                let Some((idx, chemin)) = tache else { break };
                let etat = match unprotect::deverrouiller(&chemin) {
                    Ok(r) => Etat::Ok { sortie: r.sortie, retirees: r.protections_retirees },
                    Err(e) => Etat::Erreur(e),
                };
                lignes.lock().unwrap()[idx].etat = etat;
                ctx.request_repaint();
            });
        }
    }

    fn contenu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // Fichiers déposés sur la fenêtre
        let deposes: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !deposes.is_empty() {
            self.ajouter(&ctx, deposes);
        }
        let survol = ctx.input(|i| !i.raw.hovered_files.is_empty());

        ui.add_space(8.0);
        ui.heading("Delocker");
        ui.label("Retire la protection des feuilles : formules et mise en forme conservées.");
        ui.add_space(10.0);

        // Zone de dépôt
        let fond = if survol {
            egui::Color32::from_rgba_unmultiplied(80, 140, 255, 50)
        } else {
            ui.visuals().faint_bg_color
        };
        let reponse = egui::Frame::new()
            .fill(fond)
            .stroke(egui::Stroke::new(1.5, ui.visuals().weak_text_color()))
            .corner_radius(10.0)
            .inner_margin(24.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Glissez vos fichiers Excel ici").size(18.0));
                    ui.label("ou");
                    if ui.button("Choisir des fichiers…").clicked() {
                        if let Some(choix) = rfd::FileDialog::new()
                            .add_filter("Classeurs Excel", &["xlsx", "xlsm"])
                            .pick_files()
                        {
                            self.ajouter(&ctx, choix);
                        }
                    }
                });
            });
        let _ = reponse;

        ui.add_space(10.0);

        let lignes = self.lignes.lock().unwrap().clone();
        if !lignes.is_empty() {
            ui.horizontal(|ui| {
                let finis = lignes.iter().filter(|l| !matches!(l.etat, Etat::EnAttente)).count();
                ui.label(format!("{finis} / {} fichier(s) traité(s)", lignes.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Vider la liste").clicked() {
                        self.lignes.lock().unwrap().retain(|l| matches!(l.etat, Etat::EnAttente));
                    }
                });
            });
            ui.separator();
        }

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for l in lignes.iter().rev() {
                let nom = l.source.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                ui.horizontal_wrapped(|ui| match &l.etat {
                    Etat::EnAttente => {
                        ui.spinner();
                        ui.label(format!("{nom} — en cours…"));
                    }
                    Etat::Ok { sortie, retirees } => {
                        ui.colored_label(egui::Color32::from_rgb(60, 170, 90), "✔");
                        let sortie_nom = sortie.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                        let info = if *retirees == 0 { "aucune protection trouvée" } else { "déverrouillé" };
                        ui.label(format!("{nom} → {sortie_nom} ({info})"));
                    }
                    Etat::Erreur(e) => {
                        ui.colored_label(egui::Color32::from_rgb(210, 70, 70), "✖");
                        ui.label(format!("{nom} — {e}"));
                    }
                });
            }
        });

        if lignes.is_empty() {
            ui.add_space(4.0);
            ui.weak("Chaque fichier donne un nouveau fichier « …_deverrouille.xlsx » à côté de l'original.");
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| self.contenu(ui));
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 460.0])
            .with_min_inner_size([420.0, 340.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Delocker",
        options,
        Box::new(|cc| {
            let app = App::default();
            // Fichiers glissés sur l'icône du programme (ou passés en argument)
            let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
            if !args.is_empty() {
                app.ajouter(&cc.egui_ctx, args);
            }
            Ok(Box::new(app))
        }),
    )
}
