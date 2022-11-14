use std::io::{self, Error, ErrorKind, Read, Write};

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
