use rnote_engine::engine::export::{DocExportFormat, DocExportPrefs};
use rnote_engine::engine::EngineSnapshot;
use smol::fs::File;
use smol::io::{AsyncReadExt, AsyncWriteExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};

use rnote_engine::RnoteEngine;

/// rnote-cli
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Imports the specified input file and saves it as a rnote save file.{n}
    /// Currently only `.xopp` files can be imported.
    Import {
        /// the rnote save file
        rnote_file: PathBuf,
        /// the import input file
        #[arg(short = 'i', long)]
        input_file: PathBuf,
        /// When importing a .xopp file, the import dpi can be specified.{n}
        /// Else the default (96) is used.
        #[arg(long)]
        xopp_dpi: Option<f64>,
    },
    /// Exports the Rnote file(s) and saves it in the desired format.{n}
    /// When using --output-file, only one input file can be given.{n}
    /// The export format is recognized from the file extension of the output file.{n}
    /// When using --output-format, the same file name is used with the extension changed.{n}
    /// --output-file and --output-format are mutually exclusive but one of them is required.{n}
    /// Currently `.svg`, `.xopp` and `.pdf` are supported.{n}
    /// Usages: {n}
    /// rnote-cli export --output-file [filename.(svg|xopp|pdf)] [1 file]{n}
    /// rnote-cli export --output-format [svg|xopp|pdf] [list of files]
    Export {
        /// the rnote save file
        rnote_files: Vec<PathBuf>,
        /// the export output file. Only allows for one input file. Exclusive with output-format.
        #[arg(short = 'o', long, conflicts_with("output_format"), required(true))]
        output_file: Option<PathBuf>,
        /// the export output format. Exclusive with output-file.
        #[arg(short = 'f', long, conflicts_with("output_file"), required(true))]
        output_format: Option<String>,
        /// export with background
        #[arg(short = 'b', long)]
        with_background: Option<bool>,
        /// export with background pattern
        #[arg(short = 'p', long)]
        with_pattern: Option<bool>,
    },
}

pub(crate) async fn run() -> anyhow::Result<()> {
    let mut engine = RnoteEngine::default();

    let cli = Cli::parse();

    match cli.command {
        Commands::Import {
            rnote_file,
            input_file,
            xopp_dpi,
        } => {
            // apply given arguments to import prefs
            if let Some(xopp_dpi) = xopp_dpi {
                engine.import_prefs.xopp_import_prefs.dpi = xopp_dpi;
            }

            // setup progress bar
            let pb = indicatif::ProgressBar::new_spinner().with_message(format!(
                "Importing \"{}\" to: \"{}\"",
                input_file.display(),
                rnote_file.display()
            ));
            pb.set_draw_target(indicatif::ProgressDrawTarget::stdout());

            // import file
            println!("Importing..");
            pb.enable_steady_tick(Duration::from_millis(8));
            if let Err(e) = import_file(&mut engine, input_file, rnote_file).await {
                pb.abandon();
                println!("Import failed, Err: {e:?}");
                return Err(e);
            } else {
                pb.finish();
                println!("Import finished!");
            }
        }
        Commands::Export {
            rnote_files,
            output_file,
            output_format,
            with_background,
            with_pattern,
        } => {
            // apply given arguments to export prefs
            engine.export_prefs.doc_export_prefs = create_doc_export_prefs_from_args(
                output_file.as_deref(),
                output_format.as_deref(),
                with_background,
                with_pattern,
            )?;

            match output_file {
                Some(ref output) => match rnote_files.get(0) {
                    Some(file) => {
                        if rnote_files.len() > 1 {
                            return Err(anyhow::anyhow!("Was expecting only 1 file. Use --output-format when exporting multiple files."));
                        }

                        // setup progress bar
                        let pb = indicatif::ProgressBar::new_spinner().with_message(format!(
                            "Exporting \"{}\" to: \"{}\"",
                            file.display(),
                            output.display()
                        ));
                        pb.set_draw_target(indicatif::ProgressDrawTarget::stdout());

                        // export file
                        println!("Exporting..");
                        pb.enable_steady_tick(Duration::from_millis(8));
                        if let Err(e) = export_to_file(&mut engine, file, output).await {
                            pb.abandon();
                            println!("Export failed, Err: {e:?}");
                            return Err(e);
                        } else {
                            pb.finish();
                            println!("Export finished!");
                        }
                    }
                    None => {
                        return Err(anyhow::anyhow!("Failed to get filename from rnote_files."))
                    }
                },
                None => {
                    let output_files = rnote_files
                        .iter()
                        .map(|file| {
                            let mut output = file.clone();
                            output.set_extension(
                                engine
                                    .export_prefs
                                    .doc_export_prefs
                                    .export_format
                                    .file_ext(),
                            );
                            output
                        })
                        .collect::<Vec<PathBuf>>();

                    // setup progress bars
                    let multiprogress = indicatif::MultiProgress::with_draw_target(
                        indicatif::ProgressDrawTarget::stdout(),
                    );
                    let progresses = rnote_files
                        .iter()
                        .zip(output_files.iter())
                        .map(|(file, output)| {
                            multiprogress
                                .add(indicatif::ProgressBar::new_spinner())
                                .with_message(format!(
                                    "Exporting \"{}\" to: \"{}\"",
                                    file.display(),
                                    output.display()
                                ))
                        })
                        .collect::<Vec<indicatif::ProgressBar>>();

                    // export files
                    println!("Exporting..");
                    for (i, (file, output)) in
                        rnote_files.iter().zip(output_files.iter()).enumerate()
                    {
                        progresses[i].enable_steady_tick(Duration::from_millis(8));

                        if let Err(e) = export_to_file(&mut engine, &file, &output).await {
                            progresses[i].abandon();
                            println!("Export failed, Err: {e:?}");
                            continue;
                        } else {
                            progresses[i].finish();
                        }
                    }
                    println!("Export finished!");
                }
            }
        }
    }

    Ok(())
}

pub(crate) async fn import_file(
    engine: &mut RnoteEngine,
    input_file: PathBuf,
    rnote_file: PathBuf,
) -> anyhow::Result<()> {
    let mut input_bytes = vec![];
    let Some(rnote_file_name) = rnote_file.file_name().map(|s| s.to_string_lossy().to_string()) else {
        return Err(anyhow::anyhow!("Failed to get filename from rnote_file."));
    };

    let mut ifh = File::open(input_file).await?;
    ifh.read_to_end(&mut input_bytes).await?;

    let snapshot =
        EngineSnapshot::load_from_xopp_bytes(input_bytes, engine.import_prefs.xopp_import_prefs)
            .await?;

    let _ = engine.load_snapshot(snapshot);

    let rnote_bytes = engine.save_as_rnote_bytes(rnote_file_name)?.await??;

    let mut ofh = File::create(rnote_file).await?;
    ofh.write_all(&rnote_bytes).await?;
    ofh.sync_all().await?;

    Ok(())
}

fn get_export_format(format: &str) -> anyhow::Result<DocExportFormat> {
    match format {
        "svg" => Ok(DocExportFormat::Svg),
        "xopp" => Ok(DocExportFormat::Xopp),
        "pdf" => Ok(DocExportFormat::Pdf),
        ext => Err(anyhow::anyhow!(
            "Could not create doc export prefs, unsupported export file extension `{ext}`"
        )),
    }
}

pub(crate) fn create_doc_export_prefs_from_args(
    output_file: Option<impl AsRef<Path>>,
    output_format: Option<&str>,
    with_background: Option<bool>,
    with_pattern: Option<bool>,
) -> anyhow::Result<DocExportPrefs> {
    let format = match (output_file, output_format) {
        (Some(file), None) => match file.as_ref().extension().and_then(|ext| ext.to_str()) {
            Some(extension) => get_export_format(extension),
            None => {
                return Err(anyhow::anyhow!(
                    "Output file needs to have an extension to determine the file type"
                ))
            }
        },
        (None, Some(out_format)) => get_export_format(out_format),
        // unreachable because they are exclusive (conflicts_with)
        (Some(_), Some(_)) => {
            return Err(anyhow::anyhow!(
                "--output-file and --output-format are mutually exclusive."
            ))
        }
        // unreachable because they are required
        (None, None) => {
            return Err(anyhow::anyhow!(
                "--output-file or --output-format is required."
            ))
        }
    }?;

    let mut prefs = DocExportPrefs {
        export_format: format,
        ..Default::default()
    };

    if let Some(with_background) = with_background {
        prefs.with_background = with_background;
    }
    if let Some(with_pattern) = with_pattern {
        prefs.with_pattern = with_pattern;
    }

    Ok(prefs)
}

pub(crate) async fn export_to_file(
    engine: &mut RnoteEngine,
    rnote_file: impl AsRef<Path>,
    output_file: impl AsRef<Path>,
) -> anyhow::Result<()> {
    let Some(export_file_name) = output_file.as_ref().file_name().map(|s| s.to_string_lossy().to_string()) else {
        return Err(anyhow::anyhow!("Failed to get filename from output_file."));
    };

    let mut rnote_bytes = vec![];
    File::open(rnote_file)
        .await?
        .read_to_end(&mut rnote_bytes)
        .await?;

    let engine_snapshot = EngineSnapshot::load_from_rnote_bytes(rnote_bytes).await?;
    let _ = engine.load_snapshot(engine_snapshot);

    // We applied the prefs previously to the engine
    let export_bytes = engine.export_doc(export_file_name, None).await??;

    let mut fh = File::create(output_file).await?;
    fh.write_all(&export_bytes).await?;
    fh.sync_all().await?;

    Ok(())
}
