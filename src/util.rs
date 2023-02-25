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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_copy_n() {
        let mut r1: Vec<u8> = Vec::new();
        let mut r2: Vec<u8> = Vec::new();
        let mut w: Vec<u8> = Vec::new();

        r1.extend(b"testing");
        r2.extend(b"elephant");
        w.extend(b"original_input");

        // Don't copy anything
        let result = copy_n(&mut r1.as_slice(), &mut w.as_mut_slice(), 0);
        assert_eq!(result.unwrap(), 0);
        assert_eq!(b"original_input", &w[..]);

        // Copy 4 bytes
        let result = copy_n(&mut r1.as_slice(), &mut w.as_mut_slice(), 4);
        assert_eq!(result.unwrap(), 4);
        assert_eq!(b"testinal_input", &w[..]);

        // Copy 1 byte
        let result = copy_n(&mut r2.as_slice(), &mut w.as_mut_slice(), 1);
        assert_eq!(result.unwrap(), 1);
        assert_eq!(b"eestinal_input", &w[..]);

        // Attempt to copy 100 bytes (ends early)
        let result = copy_n(&mut r2.as_slice(), &mut w.as_mut_slice(), 100);
        assert_eq!(result.unwrap(), 8);
        assert_eq!(b"elephant_input", &w[..]);
    }

    #[test]
    fn test_copy_exactly_n() {
        let mut r1: Vec<u8> = Vec::new();
        let mut r2: Vec<u8> = Vec::new();
        let mut w: Vec<u8> = Vec::new();

        r1.extend(b"testing");
        r2.extend(b"elephant");
        w.extend(b"original_input");

        // Don't copy anything
        let result = copy_exactly_n(&mut r1.as_slice(), &mut w.as_mut_slice(), 0);
        assert_eq!(result.unwrap(), 0);
        assert_eq!(b"original_input", &w[..]);

        // Copy 4 bytes
        let result = copy_exactly_n(&mut r1.as_slice(), &mut w.as_mut_slice(), 4);
        assert_eq!(result.unwrap(), 4);
        assert_eq!(b"testinal_input", &w[..]);

        // Copy 1 byte
        let result = copy_exactly_n(&mut r2.as_slice(), &mut w.as_mut_slice(), 1);
        assert_eq!(result.unwrap(), 1);
        assert_eq!(b"eestinal_input", &w[..]);

        // Attempt to copy 8 bytes (exact string length)
        let result = copy_exactly_n(&mut r2.as_slice(), &mut w.as_mut_slice(), 8);
        assert_eq!(result.unwrap(), 8);
        assert_eq!(b"elephant_input", &w[..]);

        // Attempt to copy 100 bytes (ends early)
        let result = copy_exactly_n(&mut r1.as_slice(), &mut w.as_mut_slice(), 100);
        assert!(result.is_err(), "too many bytes requested, should fail");
        assert_eq!(b"testingt_input", &w[..]);
    }
}
