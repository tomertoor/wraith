//! Framed I/O utilities — 4-byte BE length prefix.
//! Channel ID is handled separately by the multiplexer.

use bytes::{Buf, BufMut, BytesMut};
use std::io::{Error, ErrorKind, Result};

const LENGTH_PREFIX_LEN: usize = 4;

pub struct FramedReader;

impl FramedReader {
    /// Read a length-prefixed frame from a buffered byte source.
    pub fn read_frame(stream: &mut dyn Buf) -> Result<Vec<u8>> {
        if stream.remaining() < LENGTH_PREFIX_LEN {
            return Err(Error::new(ErrorKind::WouldBlock, "not enough data for length prefix"));
        }

        let len = stream.get_u32() as usize;

        if stream.remaining() < len {
            return Err(Error::new(ErrorKind::WouldBlock, "not enough data for message"));
        }

        let mut buf = vec![0u8; len];
        stream.copy_to_slice(&mut buf);
        Ok(buf)
    }
}

pub struct FramedWriter;

impl FramedWriter {
    /// Wrap a payload in a 4-byte BE length prefix.
    pub fn write_frame(buf: &[u8]) -> Result<BytesMut> {
        let mut framed = BytesMut::with_capacity(LENGTH_PREFIX_LEN + buf.len());
        framed.put_u32(buf.len() as u32);
        framed.put_slice(buf);
        Ok(framed)
    }
}

