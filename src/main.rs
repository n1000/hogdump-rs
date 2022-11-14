use clap::Parser;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Error, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use bytes_cast::{unaligned, BytesCast};

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

const HOG_SIGNATURE: [u8; 3] = *b"DHF";

#[derive(Debug)]
enum HogError {
    OpenHogFailure(io::Error),
    OpenOutputFailure(io::Error),
    OpenInputFailure(io::Error),
    SignatureReadFailure(io::Error),
    SignatureWriteFailure(io::Error),
    InvalidSignature,
    ReadHeaderError(io::Error),
    HeaderDecodeError(bytes_cast::FromBytesError),
    UnexpectedEof,
    InvalidFilename,
    ExtractFailure(io::Error),
    AppendToHogFailure(io::Error),
    SeekFailure(io::Error),
    HogFilenameTooLong,
    FileTooLarge(u64),
    BadHogFilename(String),
}

impl fmt::Display for HogError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HogError::OpenHogFailure(e) => write!(f, "failed to open HOG file: {}", e),
            HogError::OpenOutputFailure(e) => write!(f, "failed to open output file: {}", e),
            HogError::OpenInputFailure(e) => write!(f, "failed to open input file: {}", e),
            HogError::SignatureReadFailure(e) => write!(f, "reading HOG signature failed: {}", e),
            HogError::SignatureWriteFailure(e) => write!(f, "writing HOG signature failed: {}", e),
            HogError::InvalidSignature => write!(f, "file did not have correct HOG signature"),
            HogError::ReadHeaderError(e) => write!(f, "reading HOG record header failed: {}", e),
            HogError::HeaderDecodeError(e) => write!(f, "decoding HOG record header failed: {}", e),
            HogError::UnexpectedEof => write!(f, "unexpected end of file encountered"),
            HogError::InvalidFilename => write!(f, "invalid filename found in HOG record header"),
            HogError::ExtractFailure(e) => write!(f, "failed to save file from HOG to disk: {}", e),
            HogError::AppendToHogFailure(e) => write!(f, "failed to append file to HOG: {}", e),
            HogError::SeekFailure(e) => write!(f, "failed to seek in HOG file: {}", e),
            HogError::HogFilenameTooLong => write!(
                f,
                "filename cannot be stored in HOG file (it must be < 13 ASCII characters long)"
            ),
            HogError::FileTooLarge(len) => write!(
                f,
                "file of {} bytes cannot be stored in HOG (it is too large)",
                len
            ),
            HogError::BadHogFilename(name) => {
                write!(f, "could not find filename basename of file: {}", name)
            }
        }
    }
}

#[derive(BytesCast)]
#[repr(C)]
struct RawHogFileHeader {
    filename: [u8; 13],
    length: unaligned::U32Le,
}

impl RawHogFileHeader {
    fn filename_as_str(&self) -> Result<&str, HogError> {
        let filename_part = self.filename.splitn(2, |x| *x == 0).next().unwrap();

        std::str::from_utf8(filename_part).map_err(|_| HogError::InvalidFilename)
    }
}

// TODO: rename this thing to be called a HogRecordHeader, or similar
struct HogFileHeader {
    filename: PathBuf,
    length: u32,
}

impl TryFrom<&RawHogFileHeader> for HogFileHeader {
    type Error = HogError;

    fn try_from(raw_hdr: &RawHogFileHeader) -> Result<Self, Self::Error> {
        Ok(HogFileHeader {
            filename: raw_hdr.filename_as_str()?.into(),
            length: raw_hdr.length.get(),
        })
    }
}

fn read_record_header(r: &mut impl Read) -> Result<Option<HogFileHeader>, HogError> {
    const HDR_LEN: usize = std::mem::size_of::<RawHogFileHeader>();
    let mut raw_bytes = [0; HDR_LEN];
    let mut offset = 0;

    // Read in the entire header.
    loop {
        match r.read(&mut raw_bytes[offset..]) {
            Ok(len) => {
                if len == 0 {
                    if offset == 0 {
                        return Ok(None);
                    } else {
                        return Err(HogError::UnexpectedEof);
                    }
                } else {
                    offset += len;

                    if offset == HDR_LEN {
                        let (raw_hdr, _) = RawHogFileHeader::from_bytes(&raw_bytes)
                            .map_err(HogError::HeaderDecodeError)?;

                        return Ok(Some(raw_hdr.try_into()?));
                    }
                }
            }
            Err(e) => match e.kind() {
                ErrorKind::Interrupted => continue,
                _ => return Err(HogError::ReadHeaderError(e)),
            },
        };
    }
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
    let mut hog_file = HogFileReader::new(path)?;
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
    let mut hog_file = HogFileReader::new(path)?;
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

struct HogFileWriter {
    file: BufWriter<File>,
}

impl HogFileWriter {
    /// Opens an existing HOG file.
    ///
    /// If this function encounters an error opening the file, or validating the magic signature,
    /// it returns an Err.
    fn new(path: &impl AsRef<Path>) -> Result<Self, HogError> {
        let file = File::create(path).map_err(HogError::OpenHogFailure)?;
        let mut file = BufWriter::new(file);

        file.write_all(&HOG_SIGNATURE)
            .map_err(HogError::SignatureWriteFailure)?;

        Ok(Self { file })
    }

    fn append_file(&mut self, path: &impl AsRef<Path>) -> Result<u64, HogError> {
        let in_file = File::open(path).map_err(HogError::OpenInputFailure)?;
        let mut in_file = BufReader::new(in_file);
        let file_len = in_file
            .get_ref()
            .metadata()
            .map_err(HogError::AppendToHogFailure)?
            .len();

        if file_len > u32::MAX.into() {
            return Err(HogError::FileTooLarge(file_len));
        }

        let file_name = match path.as_ref().file_name() {
            Some(x) => x.to_string_lossy(),
            None => {
                return Err(HogError::BadHogFilename(
                    path.as_ref().to_string_lossy().into_owned(),
                ))
            }
        };

        let mut out_filename: Vec<u8> = file_name.bytes().collect();
        if out_filename.len() >= 13 {
            return Err(HogError::HogFilenameTooLong);
        }

        out_filename.resize(13, 0);

        let hdr = RawHogFileHeader {
            filename: out_filename.try_into().unwrap(),
            length: unaligned::U32Le::from(file_len as u32),
        };

        self.file
            .write_all(hdr.as_bytes())
            .map_err(HogError::AppendToHogFailure)?;

        std::io::copy(&mut in_file, &mut self.file).map_err(HogError::AppendToHogFailure)
    }
}

struct HogFileReader {
    file: BufReader<File>,
}

impl HogFileReader {
    /// Opens an existing HOG file.
    ///
    /// If this function encounters an error opening the file, or validating the magic signature,
    /// it returns an Err.
    fn new(path: &impl AsRef<Path>) -> Result<Self, HogError> {
        let file = File::open(path).map_err(HogError::OpenHogFailure)?;
        let mut file = BufReader::new(file);
        let mut signature = [0; 3];

        file.read_exact(&mut signature)
            .map_err(HogError::SignatureReadFailure)?;

        if signature != HOG_SIGNATURE {
            return Err(HogError::InvalidSignature);
        }

        Ok(Self { file })
    }

    /// Returns an iterator over the HOG file records.
    ///
    /// The underlying file is rewound first, meaning the iterator always starts at the beginning
    /// of the file. If the rewind fails, an error will be returned instead of the iterator.
    fn records(&mut self) -> Result<HogRecordIter, HogError> {
        self.file
            .seek(SeekFrom::Start(3))
            .map_err(HogError::SeekFailure)?;

        Ok(HogRecordIter {
            hogfile: self,
            cur_file_len: None,
            hit_error: false,
        })
    }
}

struct HogRecordIter<'a> {
    hogfile: &'a mut HogFileReader,
    cur_file_len: Option<u64>,
    hit_error: bool,
}

impl<'a> Iterator for HogRecordIter<'a> {
    type Item = Result<HogFileHeader, HogError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.hit_error {
            return None;
        }

        match self.cur_file_len.take() {
            Some(length) => {
                // User did not copy on skip the file, so just skip it.
                match self.hogfile.file.seek(SeekFrom::Current(length as i64)) {
                    Ok(_) => {}
                    Err(e) => {
                        self.hit_error = true;

                        return Some(Err(HogError::SeekFailure(e)));
                    }
                }
            }
            None => {}
        }

        let hdr = read_record_header(&mut self.hogfile.file);

        match hdr {
            Ok(Some(hdr)) => {
                self.cur_file_len = Some(hdr.length.into());

                Some(Ok(hdr))
            }
            Ok(None) => None,
            Err(x) => Some(Err(x)),
        }
    }
}

/// Copies up to "n" bytes from reader to writer. If reader runs  out of bytes before "n" bytes
/// have been transfered, or if "n" bytes are transferred, Ok is returned.
///
/// If ErrorKind::Interrupted occurs during reading or writing, this function will retry.
///
/// If any other error is encountered, it is returned, and the number of bytes copied is
/// unspecified.
pub fn copy_n<R, W>(reader: &mut R, writer: &mut W, n: u64) -> io::Result<u64>
where
    R: Read + ?Sized,
    W: Write + ?Sized,
{
    let mut buf = [0; 4096];
    let mut copied = 0;

    while copied < n {
        let max_read: usize = std::cmp::min(n - copied, buf.len() as u64)
            .try_into()
            .unwrap();

        match reader.read(&mut buf[0..max_read]) {
            Ok(len) if len == 0 => {
                return Ok(copied);
            }
            Ok(len) => {
                writer.write_all(&buf[0..len])?;

                copied += u64::try_from(len).unwrap();
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(copied)
}

/// Copies exactly "n" bytes from reader to writer. If reader runs out of bytes before "n" bytes
/// have been transfered, it is an error.
///
/// If ErrorKind::Interrupted occurs during reading or writing, this function will retry.
///
/// If any other error is encountered, it is returned, and the number of bytes copied is
/// unspecified.
pub fn copy_exactly_n<R, W>(reader: &mut R, writer: &mut W, n: u64) -> io::Result<u64>
where
    R: Read + ?Sized,
    W: Write + ?Sized,
{
    match copy_n(reader, writer, n) {
        Ok(copied) if copied == n => Ok(copied),
        Ok(copied) => Err(Error::new(
            ErrorKind::UnexpectedEof,
            format!("expected {} bytes, found {}", n, copied),
        )),
        Err(e) => Err(e),
    }
}

impl<'a> HogRecordIter<'a> {
    /// Copy the last encountered file to the destation buffer.
    fn copy_cur_file(&mut self, out_f: &mut impl Write) -> Result<(), HogError> {
        match self.cur_file_len.take() {
            Some(length) => {
                copy_exactly_n(&mut self.hogfile.file, out_f, length as u64)
                    .map_err(HogError::ExtractFailure)?;

                Ok(())
            }
            None => panic!("attempted to copy file without first scanning for the header"),
        }
    }
}

fn hog_dump_files(files: &[impl AsRef<Path>], overwrite: bool) {
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

fn hog_dump_info(files: &[impl AsRef<Path>], verbose: bool) {
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

fn hog_create(out_path: &impl AsRef<Path>, files: &[impl AsRef<Path>], verbose: bool) {
    let mut hog_file = match HogFileWriter::new(out_path) {
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
                    "{}: appended file \"{}\" ({} bytes).",
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
        hog_dump_files(&cli.file, cli.overwrite);
    } else if let Some(out_file) = cli.create {
        hog_create(&out_file, &cli.file, cli.verbose);
    } else {
        hog_dump_info(&cli.file, cli.verbose);
    }
}
