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

use std::fs::File;
use std::io::{BufReader, BufWriter, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use bytemuck::{Pod, Zeroable};

use crate::error::HogError;
use crate::util;

const HOG_SIGNATURE: [u8; 3] = *b"DHF";

// The "raw" HOG file record format, as contained in the HOG file on disk
#[derive(Pod, Zeroable, Copy, Clone)]
#[repr(C, packed)]
struct RawHogRecord {
    filename: [u8; 13],

    // On disk, this is little endian.
    length: u32,
}

impl RawHogRecord {
    // In the raw, on disk record, the end of the filename is padded with "0"
    // bytes. This function attempts to strip off the null padding, and then
    // interpret the extracted bytes as UTF-8.
    fn filename_as_str(&self) -> Result<&str, HogError> {
        // Strip off the null padding, if present.
        let filename_part = self.filename.splitn(2, |x| *x == 0).next().unwrap();

        // Attempt to interpret as UTF-8, failing if decoding fails.
        std::str::from_utf8(filename_part).map_err(|_| HogError::InvalidFilename)
    }
}

// An easier to use HogRecord, derived from the RawHogRecord, taking care of
// things such as endianness of the length field, and sanitizing the filename.
pub struct HogRecord {
    pub filename: PathBuf,
    pub length: u32,
}

// Convert from a RawHogRecord to a HogRecord
impl TryFrom<&RawHogRecord> for HogRecord {
    type Error = HogError;

    fn try_from(raw_hdr: &RawHogRecord) -> Result<Self, Self::Error> {
        Ok(HogRecord {
            filename: raw_hdr.filename_as_str()?.into(),

            // Raw record format is little endian, so convert to platform
            // native.
            length: u32::from_le(raw_hdr.length),
        })
    }
}

// Attempt to read a HOG file record header, consuming just the header.
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
                        let raw_hdr: &RawHogRecord = bytemuck::from_bytes(&raw_bytes);

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

// A helper struct used to create new HOG files on disk.
pub struct HogFileWriter {
    file: BufWriter<File>,
}

impl HogFileWriter {
    /// Creates a new HOG file and opens it in write-only mode.
    ///
    /// If this function encounters an error opening the file, or writing the
    /// magic signature bytes, it returns an Err.
    pub fn create(path: &impl AsRef<Path>) -> Result<Self, HogError> {
        let file = File::create(path).map_err(HogError::OpenHogFailure)?;
        let mut file = BufWriter::new(file);

        file.write_all(&HOG_SIGNATURE)
            .map_err(HogError::SignatureWriteFailure)?;

        Ok(Self { file })
    }

    /// Appends a HOG file record header and the files contents to this HOG
    /// file.
    ///
    /// Note that thare are special restrictions on the filenames that can be
    /// added to a HOG file.  In general, the file name is made up of 13 or
    /// fewer ASCII characters. This function will return an error if the
    /// filename cannot be represented in a HOG file.
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

            // Convert to LE when storing into the raw record.
            length: u32::to_le(file_len as u32),
        };

        self.file
            .write_all(bytemuck::bytes_of(&hdr))
            .map_err(HogError::AppendToHogFailure)?;

        std::io::copy(&mut in_file, &mut self.file).map_err(HogError::AppendToHogFailure)
    }
}

// A helper struct used to read HOG files from disk.
pub struct HogFileReader {
    file: BufReader<File>,
}

impl HogFileReader {
    /// Opens an existing HOG file.
    ///
    /// If this function encounters an error opening the file, or validating the magic signature,
    /// it returns an Err.
    pub fn open(path: &impl AsRef<Path>) -> Result<Self, HogError> {
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

// A HogRecord Iterator that cann be used to walk over the individual files in
// the HOG file.
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

// While iterating over each HogRecord, copy_cur_file() can be called on the
// iterator to save the contents of the last emitted HogRecord to an arbitrary
// Writer. If copy_cur_file() is not called on the item, then the body of the
// file is simply skipped by the Iterator when moving on to the next file.
//
// This function is implemented for the iterator, rather than the emitted
// element, because the iterator needs to keep track of the cursor position in
// the on-disk HOG file, so that it can easily advance to the next element.
//
// TODO: Explore having the iterator simply store the file offset it needs to
// process next, and always seek back to that position to yield the next element
// when next() is called. This should have the advantage of allowing
// copy_cur_file to be implemented on HogRecord itself, which could move the
// file cursor without impacting the iterator behavior.
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
