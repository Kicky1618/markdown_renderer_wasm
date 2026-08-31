use crate::{StreamDelta, TokenOrigin};

pub const MAGIC: &[u8; 4] = b"SMT1";

pub fn encode_delta_into(delta: &StreamDelta, out: &mut Vec<u8>) {
    out.clear();
    out.extend_from_slice(MAGIC);
    put_u32(out, delta.retract.min(u32::MAX as usize) as u32);
    put_u32(out, delta.push.len().min(u32::MAX as usize) as u32);
    for token in delta.push.iter().take(u32::MAX as usize) {
        put_u64(out, token.start as u64);
        put_u64(out, token.end as u64);
        put_u16(out, token.tag);
        out.push(match token.origin {
            TokenOrigin::Lexicon => 0,
            TokenOrigin::Unknown => 1,
        });
        out.push(0);
        put_i32(out, token.word_cost);
        put_str(out, &token.surface);
        put_str(out, &token.lemma);
        put_str(out, &token.reading);
    }
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    put_u32(out, bytes.len().min(u32::MAX as usize) as u32);
    out.extend_from_slice(&bytes[..bytes.len().min(u32::MAX as usize)]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StreamDelta, Token, TokenOrigin};
    use std::sync::Arc;

    #[test]
    fn wire_header_and_lengths_are_stable() {
        let delta = StreamDelta {
            retract: 2,
            push: vec![Token {
                start: 3,
                end: 9,
                surface: Arc::from("東京"),
                lemma: Arc::from("東京"),
                reading: Arc::from("トウキョウ"),
                tag: 9,
                word_cost: -123,
                origin: TokenOrigin::Lexicon,
            }],
        };
        let mut out = Vec::new();
        encode_delta_into(&delta, &mut out);
        assert_eq!(&out[..4], b"SMT1");
        assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(out[8..12].try_into().unwrap()), 1);
        assert_eq!(u64::from_le_bytes(out[12..20].try_into().unwrap()), 3);
        assert_eq!(u64::from_le_bytes(out[20..28].try_into().unwrap()), 9);
    }
}
