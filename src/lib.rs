//! This crate provides a zero-allocation iterator for the payload bytes in an HTTP
//! [chunked-encoded](https://tools.ietf.org/html/rfc7230#section-4.1) body. It wraps a
//! given iterator over raw HTTP body bytes, decodes the chunked transfer protocol, and
//! yields the data bytes from each chunk. The result can be fed, for example, into a
//! byte-based parser such as
//! [serde_json::from_iter](https://docs.serde.rs/serde_json/de/fn.from_iter.html).
//!
//! This implementation supports chunk lengths up to that which can be stored by `usize`
//! on the target platform. Chunk extension parameters are discarded, and trailing headers
//! aren't processed, although they can be retrieved from the wrapped source iterator at
//! the end of chunked payload iteration.
//!
//! ## Example
//!
//! ```rust
//! use uhttp_chunked_bytes::ChunkedBytes;
//!
//! // Create a sample json body `{"key": 42}`, split over two chunks.
//! let body = b"4\r\n{\"ke\r\n7\r\ny\": 42}\r\n0\r\n\r\n";
//! let mut stream = body.iter().map(|&b| Ok(b));
//!
//! let mut bytes = ChunkedBytes::new(&mut stream);
//! assert_eq!(bytes.next().unwrap().unwrap(), b'{');
//! assert_eq!(bytes.next().unwrap().unwrap(), b'"');
//! assert_eq!(bytes.next().unwrap().unwrap(), b'k');
//! assert_eq!(bytes.next().unwrap().unwrap(), b'e');
//! assert_eq!(bytes.next().unwrap().unwrap(), b'y');
//! assert_eq!(bytes.next().unwrap().unwrap(), b'"');
//! assert_eq!(bytes.next().unwrap().unwrap(), b':');
//! assert_eq!(bytes.next().unwrap().unwrap(), b' ');
//! assert_eq!(bytes.next().unwrap().unwrap(), b'4');
//! assert_eq!(bytes.next().unwrap().unwrap(), b'2');
//! assert_eq!(bytes.next().unwrap().unwrap(), b'}');
//! assert!(bytes.next().is_none());
//! ```

/// A 64-bit usize number can have at most 16 hex digits.
#[cfg(target_pointer_width = "64")]
type DigitBuf = [u8; 16];

/// A 32-bit usize number can have at most 8 hex digits.
#[cfg(target_pointer_width = "32")]
type DigitBuf = [u8; 8];

/// Iterator over payload bytes in a chunked-encoded stream.
///
/// When the iterator returns `None`, the wrapped stream will typically contain a final
/// CRLF to end the body, but it may also contain [trailing header
/// fields](https://tools.ietf.org/html/rfc7230#section-4.1.2) before the final CRLF.
pub struct ChunkedBytes<I: Iterator<Item = std::io::Result<u8>>> {
    /// Underlying byte stream in chunked transfer-encoding format.
    stream: I,
    /// Number of remaining bytes in the current chunk.
    remain: usize,
}

impl<I: Iterator<Item = std::io::Result<u8>>> ChunkedBytes<I> {
    /// Create a new `ChunkedBytes` iterator over the given byte stream.
    pub fn new(stream: I) -> Self {
        ChunkedBytes {
            stream: stream,
            remain: 0,
        }
    }

    /// Parse the number of bytes in the next chunk.
    fn parse_size(&mut self) -> Option<std::io::Result<usize>> {
        let mut digits = DigitBuf::default();

        let slice = match self.parse_digits(&mut digits[..]) {
            // This is safe because the following call to `from_str_radix` does
            // its own verification on the bytes.
            Some(Ok(s)) => unsafe { std::str::from_utf8_unchecked(s) },
            Some(Err(e)) => return Some(Err(e)),
            None => return None,
        };

        match usize::from_str_radix(slice, 16) {
            Ok(n) => Some(Ok(n)),
            Err(_) => Some(Err(std::io::ErrorKind::InvalidData.into())),
        }
    }

    /// Extract the hex digits for the current chunk size.
    fn parse_digits<'a>(&mut self, digits: &'a mut [u8])
        -> Option<std::io::Result<&'a [u8]>>
    {
        // Number of hex digits that have been extracted.
        let mut len = 0;

        loop {
            let b = match self.stream.next() {
                Some(Ok(b)) => b,
                Some(Err(e)) => return Some(Err(e)),
                None => return if len == 0 {
                    // If EOF at the beginning of a new chunk, the stream is finished.
                    None
                } else {
                    Some(Err(std::io::ErrorKind::UnexpectedEof.into()))
                },
            };

            match b {
                b'\r' => if let Err(e) = self.consume_lf() {
                    return Some(Err(e));
                } else {
                    break;
                },
                b';' => if let Err(e) = self.consume_ext() {
                    return Some(Err(e));
                } else {
                    break;
                },
                _ => {
                    match digits.get_mut(len) {
                        Some(d) => *d = b,
                        None => return Some(Err(std::io::ErrorKind::Other.into())),
                    }

                    len += 1;
                },
            }
        }

        Some(Ok(&digits[..len]))
    }

    /// Consume and discard current chunk extension.
    ///
    /// This doesn't check whether the characters up to CRLF actually have correct syntax.
    fn consume_ext(&mut self) -> std::io::Result<()> {
        loop {
            match self.stream.next() {
                Some(Ok(b'\r')) => return self.consume_lf(),
                Some(Ok(_)) => {},
                Some(Err(e)) => return Err(e),
                None => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            }
        }
    }

    /// Verify the next bytes in the stream are CRLF.
    fn consume_crlf(&mut self) -> std::io::Result<()> {
        match self.stream.next() {
            Some(Ok(b'\r')) => self.consume_lf(),
            Some(Ok(_)) => Err(std::io::ErrorKind::InvalidData.into()),
            Some(Err(e)) => Err(e),
            None => Err(std::io::ErrorKind::UnexpectedEof.into()),
        }
    }

    /// Verify the next byte in the stream is LF.
    fn consume_lf(&mut self) -> std::io::Result<()> {
        match self.stream.next() {
            Some(Ok(b'\n')) => Ok(()),
            Some(Ok(_)) => Err(std::io::ErrorKind::InvalidData.into()),
            Some(Err(e)) => Err(e),
            None => Err(std::io::ErrorKind::UnexpectedEof.into()),
        }
    }
}

impl<I: Iterator<Item = std::io::Result<u8>>> Iterator for ChunkedBytes<I> {
    type Item = std::io::Result<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remain == 0 {
            let size = match self.parse_size() {
                Some(Ok(s)) => s,
                Some(Err(e)) => return Some(Err(e)),
                None => return None,
            };

            // If chunk size is zero (final chunk), the stream is finished [RFC7230§4.1].
            if size == 0 {
                return None;
            }

            self.remain = size;
        }

        let next = self.stream.next();
        self.remain -= 1;

        // If current chunk is finished, verify it ends with CRLF [RFC7230§4.1].
        if self.remain == 0 {
            if let Err(e) = self.consume_crlf() {
                return Some(Err(e));
            }
        }

        next
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std;

    #[test]
    fn test_chunked_bytes() {
        let stream = b"A\r\nabcdefghij\r\n2\r\n42\r\n";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert_eq!(c.next().unwrap().unwrap(), b'a');
        assert_eq!(c.next().unwrap().unwrap(), b'b');
        assert_eq!(c.next().unwrap().unwrap(), b'c');
        assert_eq!(c.next().unwrap().unwrap(), b'd');
        assert_eq!(c.next().unwrap().unwrap(), b'e');
        assert_eq!(c.next().unwrap().unwrap(), b'f');
        assert_eq!(c.next().unwrap().unwrap(), b'g');
        assert_eq!(c.next().unwrap().unwrap(), b'h');
        assert_eq!(c.next().unwrap().unwrap(), b'i');
        assert_eq!(c.next().unwrap().unwrap(), b'j');
        assert_eq!(c.next().unwrap().unwrap(), b'4');
        assert_eq!(c.next().unwrap().unwrap(), b'2');
        assert!(c.next().is_none());

        let stream = b"a\r\nabc\r\nfghij\r\n2\r\n42\r\n";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert_eq!(c.next().unwrap().unwrap(), b'a');
        assert_eq!(c.next().unwrap().unwrap(), b'b');
        assert_eq!(c.next().unwrap().unwrap(), b'c');
        assert_eq!(c.next().unwrap().unwrap(), b'\r');
        assert_eq!(c.next().unwrap().unwrap(), b'\n');
        assert_eq!(c.next().unwrap().unwrap(), b'f');
        assert_eq!(c.next().unwrap().unwrap(), b'g');
        assert_eq!(c.next().unwrap().unwrap(), b'h');
        assert_eq!(c.next().unwrap().unwrap(), b'i');
        assert_eq!(c.next().unwrap().unwrap(), b'j');
        assert_eq!(c.next().unwrap().unwrap(), b'4');
        assert_eq!(c.next().unwrap().unwrap(), b'2');
        assert!(c.next().is_none());

        let stream = b"4\r\nabcd\r\n0\r\n\r\n";
        let mut iter = stream.iter().map(|&x| Ok(x));

        {
            let mut c = ChunkedBytes::new(&mut iter);
            assert_eq!(c.next().unwrap().unwrap(), b'a');
            assert_eq!(c.next().unwrap().unwrap(), b'b');
            assert_eq!(c.next().unwrap().unwrap(), b'c');
            assert_eq!(c.next().unwrap().unwrap(), b'd');
            assert!(c.next().is_none());
        }

        assert_eq!(iter.next().unwrap().unwrap(), b'\r');
        assert_eq!(iter.next().unwrap().unwrap(), b'\n');
        assert!(iter.next().is_none());

        let stream = b"4\r\nabcd\r\n0\r\nA: B\r\n\r\n";
        let mut iter = stream.iter().map(|&x| Ok(x));

        {
            let mut c = ChunkedBytes::new(&mut iter);
            assert_eq!(c.next().unwrap().unwrap(), b'a');
            assert_eq!(c.next().unwrap().unwrap(), b'b');
            assert_eq!(c.next().unwrap().unwrap(), b'c');
            assert_eq!(c.next().unwrap().unwrap(), b'd');
            assert!(c.next().is_none());
        }

        assert_eq!(iter.next().unwrap().unwrap(), b'A');
        assert_eq!(iter.next().unwrap().unwrap(), b':');
        assert_eq!(iter.next().unwrap().unwrap(), b' ');
        assert_eq!(iter.next().unwrap().unwrap(), b'B');
        assert_eq!(iter.next().unwrap().unwrap(), b'\r');
        assert_eq!(iter.next().unwrap().unwrap(), b'\n');
        assert_eq!(iter.next().unwrap().unwrap(), b'\r');
        assert_eq!(iter.next().unwrap().unwrap(), b'\n');
        assert!(iter.next().is_none());

        let stream = b"";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert!(c.next().is_none());

        let stream = b"0\r\n\r\n";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert!(c.next().is_none());

        let stream = b"h\r\n";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert!(c.next().unwrap().is_err());

        let stream = b"\r\na";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert!(c.next().unwrap().is_err());

        let stream = b"4\r\nabcdefg";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert_eq!(c.next().unwrap().unwrap(), b'a');
        assert_eq!(c.next().unwrap().unwrap(), b'b');
        assert_eq!(c.next().unwrap().unwrap(), b'c');
        assert!(c.next().unwrap().is_err());
    }


    #[cfg(target_pointer_width = "64")]
    #[test]
    fn test_max_size() {
        let stream = b"FFFFFFFFFFFFFFFF\r\na";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert_eq!(c.next().unwrap().unwrap(), b'a');
        assert_eq!(c.remain, std::usize::MAX - 1);

        let stream = b"FFFFFFFFFFFFFFFFF\r\na";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert!(c.next().unwrap().is_err());
    }

    #[cfg(target_pointer_width = "32")]
    #[test]
    fn test_max_size() {
        let stream = b"FFFFFFFF\r\na";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert_eq!(c.next().unwrap().unwrap(), b'a');
        assert_eq!(c.remain, std::usize::MAX - 1);

        let stream = b"FFFFFFFFF\r\na";
        let mut c = ChunkedBytes::new(stream.iter().map(|&x| Ok(x)));
        assert!(c.next().unwrap().is_err());
    }
}
