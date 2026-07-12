//! Creative Voice File (.VOC) parsing — shared by talking-head voices
//! (which need the lip-sync mouth stream) and plain sound effects (which do
//! not). = voc_get_lipsync_data (seg000:a83f) + the type-1 PCM block.

/// A parsed .VOC: the 8-bit unsigned mono PCM and its sample rate, plus the
/// lip-sync mouth stream when present. Sound effects (SN3.VOC, …) carry no
/// type-5 comment block, so `lipsync` is empty for them.
pub struct Voc {
    pub rate: u32,
    pub pcm: Vec<u8>,
    pub lipsync: Vec<u8>,
    pub looping: bool,
}

/// Parse a Creative Voice File: skip the 0x1a-byte header, then walk the typed
/// blocks (`[type u8][size u24][payload]`). Block type 1 (data) carries the
/// audio — `[time-constant u8][codec u8][8-bit unsigned PCM…]`, the time
/// constant giving the sample rate `1e6 / (256 - tc)`. Block type 5 (comment)
/// carries the lip-sync mouth stream starting at payload+2, 0xFF-terminated.
///
/// Returns `None` only when there is no usable audio (no type-1 block); the
/// lip-sync stream is optional, so this also parses effect-only .VOCs.
pub fn parse(data: &[u8]) -> Option<Voc> {
    if data.len() < 0x1a || &data[..19] != b"Creative Voice File" {
        return None;
    }

    let mut rate = None;
    let mut pcm = Vec::new();
    let mut lipsync = Vec::new();
    let mut looping = false;

    let mut pos = 0x1a;
    while pos + 4 <= data.len() {
        let block_type = data[pos];
        if block_type == 0 {
            break; // terminator block
        }
        let size = (data[pos + 1] as usize)
            | ((data[pos + 2] as usize) << 8)
            | ((data[pos + 3] as usize) << 16);
        let payload_start = pos + 4;
        let payload_end = (payload_start + size).min(data.len());
        let payload = &data[payload_start..payload_end];

        match block_type {
            // = type-5 comment block: 2-byte index prefix, then the mouth
            // stream up to the 0xFF terminator.
            5 if payload.len() > 2 => {
                let stream = &payload[2..];
                let end = stream
                    .iter()
                    .position(|&b| b == 0xff)
                    .unwrap_or(stream.len());
                lipsync = stream[..end].to_vec();
            }
            // = type-1 data block: time-constant + codec, then raw samples.
            1 if payload.len() > 2 => {
                let tc = payload[0];
                rate = Some(1_000_000 / (256 - tc as u32));
                pcm = payload[2..].to_vec();
            }
            // = type-6 repeat start block
            6 => {
                looping = true;
            }
            // = type-7 repeat end block
            7 => {
                looping = true;
            }
            _ => {}
        }

        pos = payload_start + size;
    }

    let rate = rate?;
    if pcm.is_empty() {
        return None;
    }
    Some(Voc {
        rate,
        pcm,
        lipsync,
        looping,
    })
}
