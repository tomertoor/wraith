use bytes::{BufMut, BytesMut};
use std::io::Result;

pub struct FramedWriter;

impl FramedWriter {
    pub fn write_frame(data: &[u8]) -> Result<Vec<u8>> {
        let mut framed = BytesMut::with_capacity(4 + data.len());
        framed.put_u32(data.len() as u32);
        framed.put_slice(data);
        Ok(framed.to_vec())
    }
}

pub fn read_frame_len(buf: &[u8]) -> Result<usize> {
    if buf.len() < 4 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "insufficient data for frame length",
        ));
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    Ok(len)
}