use clap::Parser;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use bytes_cast::{unaligned, BytesCast};

#[derive(Parser)]
#[command(author, version, about, long_about = None, arg_required_else_help(true))]
struct Cli {
    /// Extract the contents of the hog file
    #[arg(short = 'x', long)]
    extract: bool,

    /// The files to operate on (1 or more)
    #[arg(required = true)]
    file: Vec<PathBuf>,
}

const HOG_SIGNATURE: [u8; 3] = *b"DHF";

#[derive(Debug)]
enum HogError {
    OpenFailure(io::Error),
    SignatureReadFailure(io::Error),
    InvalidSignature,
    ReadHeaderError(io::Error),
    HeaderDecodeError(bytes_cast::FromBytesError),
    UnexpectedEof,
    InvalidFilename,
    ExtractFailure(io::Error),
    SeekFailure(io::Error),
}

impl fmt::Display for HogError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HogError::OpenFailure(e) => write!(f, "failed to open HOG file: {}", e),
            HogError::SignatureReadFailure(e) => write!(f, "reading HOG signature failed: {}", e),
            HogError::InvalidSignature => {
                write!(f, "file did not have correct HOG signature")
            }
            HogError::ReadHeaderError(e) => write!(f, "reading HOG record header failed: {}", e),
            HogError::HeaderDecodeError(e) => write!(f, "decoding HOG record header failed: {}", e),
            HogError::UnexpectedEof => write!(f, "unexpected end of file encountered"),
            HogError::InvalidFilename => write!(f, "invalid filename found in HOG record header"),
            HogError::ExtractFailure(e) => write!(f, "failed to save file from HOG to disk: {}", e),
            HogError::SeekFailure(e) => write!(f, "failed to seek in HOG file: {}", e),
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

fn hog_dump(path: &Path) -> Result<(), HogError> {
    println!("[{}]", path.to_string_lossy());

    let f = File::open(path).map_err(HogError::OpenFailure)?;
    let mut f = BufReader::new(f);

    let mut signature = [0; 3];

    f.read_exact(&mut signature)
        .map_err(HogError::SignatureReadFailure)?;

    if signature != HOG_SIGNATURE {
        return Err(HogError::InvalidSignature);
    }

    while let Some(hdr) = read_record_header(&mut f)? {
        print!("  {}: ", hdr.filename.display());

        // Create the output file
        let out_f = File::create(hdr.filename).map_err(HogError::OpenFailure)?;
        let mut out_f = BufWriter::new(out_f);

        let mut take = f.take(hdr.length as u64);
        std::io::copy(&mut take, &mut out_f).map_err(HogError::ExtractFailure)?;
        f = take.into_inner();

        println!("wrote {} bytes", hdr.length);
    }

    Ok(())
}

fn hog_info(path: &Path) -> Result<(), HogError> {
    println!("[{}]", path.to_string_lossy());

    let f = File::open(path).map_err(HogError::OpenFailure)?;
    let mut f = BufReader::new(f);

    let mut signature = [0; 3];

    f.read_exact(&mut signature)
        .map_err(HogError::SignatureReadFailure)?;

    if signature != HOG_SIGNATURE {
        return Err(HogError::InvalidSignature);
    }

    while let Some(hdr) = read_record_header(&mut f)? {
        println!("  {}: {} bytes", hdr.filename.display(), hdr.length);
        f.seek(SeekFrom::Current(hdr.length as i64))
            .map_err(HogError::SeekFailure)?;
    }

    Ok(())
}

fn main() {
    let cli = Cli::parse();

    for file in &cli.file {
        match if cli.extract {
            hog_dump(file)
        } else {
            hog_info(file)
        } {
            Ok(_) => {}
            Err(e) => {
                eprintln!(
                    "error while processing file \"{}\": {}",
                    file.to_string_lossy(),
                    e
                );
            }
        }
    }
}
