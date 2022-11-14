use std::fmt;
use std::io::{self};

#[derive(Debug)]
pub enum HogError {
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
