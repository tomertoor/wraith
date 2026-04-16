//! Channel multiplexer over a single TCP connection.
//!
//! Frame format: [4-byte channel ID][4-byte length][payload...]
//! Channel IDs are defined in `crate::tunnel::channel::channel`.

use bytes::{BufMut, BytesMut};


/// Encode a framed message for a specific channel.
pub fn encode_frame(channel_id: u32, payload: &[u8]) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(8 + payload.len());
    buf.put_u32(channel_id);
    buf.put_u32(payload.len() as u32);
    buf.put_slice(payload);
    buf.to_vec()
}

/// Decode the channel ID and length from a frame header.
pub fn decode_header(buf: &[u8]) -> Option<(u32, usize)> {
    if buf.len() < 8 {
        return None;
    }
    let channel_id = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    Some((channel_id, len))
}
