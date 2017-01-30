# uhttp\_chunked\_bytes -- Iterator for HTTP chunked body bytes

[Documentation](https://docs.rs/uhttp_chunked_bytes)

This crate provides a zero-allocation iterator for the payload bytes in an HTTP
[chunked-encoded](https://tools.ietf.org/html/rfc7230#section-4.1) body. It wraps a
given iterator over raw HTTP body bytes, decodes the chunked transfer protocol, and
yields the data bytes from each chunk. The result can be fed, for example, into a
byte-based parser such as
[serde_json::from_iter](https://docs.serde.rs/serde_json/de/fn.from_iter.html).

This implementation supports chunk lengths up to that which can be stored by `usize`
on the target platform. Chunk extension parameters are discarded, and trailing headers
aren't processed, although they can be retrieved from the wrapped source iterator at
the end of chunked payload iteration.

## Example

```rust
use uhttp_chunked_bytes::ChunkedBytes;

// Create a sample json body `{"key": 42}`, split over two chunks.
let body = b"4\r\n{\"ke\r\n7\r\ny\": 42}\r\n0\r\n\r\n";
let mut stream = body.iter().map(|&b| Ok(b));

let mut bytes = ChunkedBytes::new(&mut stream);
assert_eq!(bytes.next().unwrap().unwrap(), b'{');
assert_eq!(bytes.next().unwrap().unwrap(), b'"');
assert_eq!(bytes.next().unwrap().unwrap(), b'k');
assert_eq!(bytes.next().unwrap().unwrap(), b'e');
assert_eq!(bytes.next().unwrap().unwrap(), b'y');
assert_eq!(bytes.next().unwrap().unwrap(), b'"');
assert_eq!(bytes.next().unwrap().unwrap(), b':');
assert_eq!(bytes.next().unwrap().unwrap(), b' ');
assert_eq!(bytes.next().unwrap().unwrap(), b'4');
assert_eq!(bytes.next().unwrap().unwrap(), b'2');
assert_eq!(bytes.next().unwrap().unwrap(), b'}');
assert!(bytes.next().is_none());
```

## Usage

This [crate](https://crates.io/crates/uhttp_chunked_bytes) can be used through cargo by
adding it as a dependency in `Cargo.toml`:

```toml
[dependencies]
uhttp_chunked_bytes = "0.5.0"
```
and importing it in the crate root:

```rust
extern crate uhttp_chunked_bytes;
```
