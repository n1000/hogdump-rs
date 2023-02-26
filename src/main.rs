//
// Copyright (c) 2022-2023 Nathaniel Houghton <nathan@brainwerk.org>
//
// Permission to use, copy, modify, and distribute this software for
// any purpose with or without fee is hereby granted, provided that
// the above copyright notice and this permission notice appear in all
// copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL
// WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
// AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL
// DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA
// OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
// TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
// PERFORMANCE OF THIS SOFTWARE.
//

//!
//! HOG File Dump Utility
//!
//! This utility can extract and create Descent 1 HOG files.
//!

use clap::Parser;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, ErrorKind};
use std::path::{Path, PathBuf};

mod error;
mod hog;
mod util;

use crate::error::HogError;
use crate::hog::{HogFileReader, HogFileWriter};

#[derive(Parser)]
#[command(author, version, about, long_about = None, arg_required_else_help(true))]
struct Cli {
    /// Extract the contents of the provided hog file(s)
    #[arg(short = 'x', long)]
    extract: bool,

    /// Create hog file out of the provided file(s)
    #[arg(short = 'c', long)]
    create: Option<PathBuf>,

    /// Overwrite files
    #[arg(short = 'o', long)]
    overwrite: bool,

    /// Display more information during processing
    #[arg(short = 'v', long)]
    verbose: bool,

    /// The files to operate on (1 or more)
    #[arg(required = true)]
    file: Vec<PathBuf>,
}

struct HogExtractInfo {
    files_processed: u64,
    files_extracted: u64,
    files_skipped: u64,
    bytes_extracted: u64,
}

impl HogExtractInfo {
    fn new() -> Self {
        Self {
            files_processed: 0,
            files_extracted: 0,
            files_skipped: 0,
            bytes_extracted: 0,
        }
    }
}

fn hog_dump(path: &impl AsRef<Path>, overwrite: bool) -> Result<HogExtractInfo, HogError> {
    let mut hog_file = HogFileReader::open(path)?;
    let mut hog_extract_info = HogExtractInfo::new();
    let mut iter = hog_file.records()?;

    loop {
        match iter.next() {
            Some(Ok(hdr)) => {
                print!(
                    "  {}: {}: ",
                    path.as_ref().display(),
                    hdr.filename.display()
                );

                hog_extract_info.files_processed += 1;

                // Create the output file
                let mut out_f = if overwrite {
                    let f = File::create(hdr.filename).map_err(HogError::OpenOutputFailure)?;
                    BufWriter::new(f)
                } else {
                    match OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(hdr.filename)
                    {
                        Ok(f) => BufWriter::new(f),
                        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                            println!("skipping (already exists)");

                            hog_extract_info.files_skipped += 1;

                            continue;
                        }
                        Err(e) => return Err(HogError::OpenOutputFailure(e)),
                    }
                };

                iter.copy_cur_file(&mut out_f)?;

                println!("wrote {} bytes", hdr.length);

                hog_extract_info.bytes_extracted += u64::from(hdr.length);
                hog_extract_info.files_extracted += 1;
            }
            Some(Err(e)) => {
                return Err(e);
            }
            None => {
                break;
            }
        }
    }

    Ok(hog_extract_info)
}

struct HogInfoSummary {
    num_files: u64,
    num_bytes: u64,
}

impl HogInfoSummary {
    fn new() -> Self {
        Self {
            num_files: 0,
            num_bytes: 0,
        }
    }
}

fn hog_info(path: &impl AsRef<Path>, verbose: bool) -> Result<HogInfoSummary, HogError> {
    let mut hog_file = HogFileReader::open(path)?;
    let mut hog_info_summary = HogInfoSummary::new();
    let mut iter = hog_file.records()?;

    loop {
        match iter.next() {
            Some(Ok(hdr)) => {
                if verbose {
                    println!(
                        "  {}: {}: {} bytes",
                        path.as_ref().display(),
                        hdr.filename.display(),
                        hdr.length
                    );
                }

                hog_info_summary.num_files += 1;
                hog_info_summary.num_bytes += u64::from(hdr.length);
            }
            Some(Err(e)) => {
                return Err(e);
            }
            None => {
                break;
            }
        }
    }

    Ok(hog_info_summary)
}

fn extract_hog_files(files: &[impl AsRef<Path>], overwrite: bool) {
    for file in files {
        match hog_dump(file, overwrite) {
            Ok(extract_info) => {
                println!(
                    "Processed {} files, extracted {} files ({} bytes), skipped {} files.",
                    extract_info.files_processed,
                    extract_info.files_extracted,
                    extract_info.bytes_extracted,
                    extract_info.files_skipped
                );
            }
            Err(e) => {
                eprintln!(
                    "error while processing HOG file \"{}\": {}",
                    file.as_ref().display(),
                    e
                );
            }
        }
    }
}

fn display_hog_info(files: &[impl AsRef<Path>], verbose: bool) {
    for file in files {
        match hog_info(file, verbose) {
            Ok(hog_info_summary) => {
                println!(
                    "{}: contains {} files ({} bytes).",
                    file.as_ref().display(),
                    hog_info_summary.num_files,
                    hog_info_summary.num_bytes,
                );
            }
            Err(e) => {
                eprintln!(
                    "error while processing HOG file \"{}\": {}",
                    file.as_ref().display(),
                    e
                );
            }
        }
    }
}

// TODO: add summary stats at the end, similar to display_hog_info() and extract_hog_files()
fn create_hog_file(out_path: &impl AsRef<Path>, files: &[impl AsRef<Path>], _verbose: bool) {
    let mut hog_file = match HogFileWriter::create(out_path) {
        Ok(x) => x,
        Err(e) => {
            eprintln!(
                "error creating output HOG file \"{}\": {}",
                out_path.as_ref().display(),
                e
            );

            std::process::exit(1);
        }
    };

    for file in files {
        match hog_file.append_file(file) {
            Ok(length) => {
                println!(
                    "{}: added file \"{}\" ({} bytes).",
                    out_path.as_ref().display(),
                    file.as_ref().display(),
                    length,
                );
            }
            Err(e) => {
                eprintln!(
                    "error occurred while appending \"{}\" to HOG file \"{}\": {}",
                    file.as_ref().display(),
                    out_path.as_ref().display(),
                    e
                );
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();

    if cli.extract && cli.create.is_some() {
        eprintln!("error: --extract and --create are mutually exclusive operations.");
        std::process::exit(1);
    }

    if cli.extract {
        extract_hog_files(&cli.file, cli.overwrite);
    } else if let Some(out_file) = cli.create {
        create_hog_file(&out_file, &cli.file, cli.verbose);
    } else {
        display_hog_info(&cli.file, cli.verbose);
    }
}
