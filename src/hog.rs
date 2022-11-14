use std::fs::File;
use std::io::{BufReader, BufWriter, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use bytes_cast::{unaligned, BytesCast};

use crate::error::HogError;
use crate::util;

const HOG_SIGNATURE: [u8; 3] = *b"DHF";

#[derive(BytesCast)]
#[repr(C)]
struct RawHogRecord {
    filename: [u8; 13],
    length: unaligned::U32Le,
}

impl RawHogRecord {
    fn filename_as_str(&self) -> Result<&str, HogError> {
        let filename_part = self.filename.splitn(2, |x| *x == 0).next().unwrap();

        std::str::from_utf8(filename_part).map_err(|_| HogError::InvalidFilename)
    }
}

pub struct HogRecord {
    pub filename: PathBuf,
    pub length: u32,
}

impl TryFrom<&RawHogRecord> for HogRecord {
    type Error = HogError;

    fn try_from(raw_hdr: &RawHogRecord) -> Result<Self, Self::Error> {
        Ok(HogRecord {
            filename: raw_hdr.filename_as_str()?.into(),
            length: raw_hdr.length.get(),
        })
    }
}

fn read_record_header(r: &mut impl Read) -> Result<Option<HogRecord>, HogError> {
    const HDR_LEN: usize = std::mem::size_of::<RawHogRecord>();
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
                        let (raw_hdr, _) = RawHogRecord::from_bytes(&raw_bytes)
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

pub struct HogFileWriter {
    file: BufWriter<File>,
}

impl HogFileWriter {
    /// Opens an existing HOG file.
    ///
    /// If this function encounters an error opening the file, or validating the magic signature,
    /// it returns an Err.
    pub fn new(path: &impl AsRef<Path>) -> Result<Self, HogError> {
        let file = File::create(path).map_err(HogError::OpenHogFailure)?;
        let mut file = BufWriter::new(file);

        file.write_all(&HOG_SIGNATURE)
            .map_err(HogError::SignatureWriteFailure)?;

        Ok(Self { file })
    }

    pub fn append_file(&mut self, path: &impl AsRef<Path>) -> Result<u64, HogError> {
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

        let hdr = RawHogRecord {
            filename: out_filename.try_into().unwrap(),
            length: unaligned::U32Le::from(file_len as u32),
        };

        self.file
            .write_all(hdr.as_bytes())
            .map_err(HogError::AppendToHogFailure)?;

        std::io::copy(&mut in_file, &mut self.file).map_err(HogError::AppendToHogFailure)
    }
}

pub struct HogFileReader {
    file: BufReader<File>,
}

impl HogFileReader {
    /// Opens an existing HOG file.
    ///
    /// If this function encounters an error opening the file, or validating the magic signature,
    /// it returns an Err.
    pub fn new(path: &impl AsRef<Path>) -> Result<Self, HogError> {
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
    pub fn records(&mut self) -> Result<HogRecordIter, HogError> {
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

pub struct HogRecordIter<'a> {
    hogfile: &'a mut HogFileReader,
    cur_file_len: Option<u64>,
    hit_error: bool,
}

impl<'a> Iterator for HogRecordIter<'a> {
    type Item = Result<HogRecord, HogError>;

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

impl<'a> HogRecordIter<'a> {
    /// Copy the last encountered file to the destation buffer.
    pub fn copy_cur_file(&mut self, out_f: &mut impl Write) -> Result<(), HogError> {
        match self.cur_file_len.take() {
            Some(length) => {
                util::copy_exactly_n(&mut self.hogfile.file, out_f, length as u64)
                    .map_err(HogError::ExtractFailure)?;

                Ok(())
            }
            None => panic!("attempted to copy file without first scanning for the header"),
        }
    }
}
