//! Códec ligero IMA-ADPCM estéreo (compresión 4:1 sin dependencias nativas externas) para audio PCM `16-bit`.

const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

const INDEX_TABLE: [i8; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

#[inline]
fn initial_step_index(abs_delta: i32) -> usize {
    STEP_TABLE
        .iter()
        .position(|&s| s.saturating_mul(2) >= abs_delta)
        .unwrap_or(STEP_TABLE.len() - 1)
}

#[inline]
fn encode_sample(sample: i16, pred: &mut i32, idx: &mut usize) -> u8 {
    let step = STEP_TABLE[*idx];
    let mut diff = (sample as i32) - *pred;
    let mut nibble = 0u8;
    if diff < 0 {
        nibble = 8;
        diff = -diff;
    }

    let mut step_tmp = step;
    if diff >= step_tmp {
        nibble |= 4;
        diff -= step_tmp;
    }
    step_tmp >>= 1;
    if diff >= step_tmp {
        nibble |= 2;
        diff -= step_tmp;
    }
    step_tmp >>= 1;
    if diff >= step_tmp {
        nibble |= 1;
    }

    let _ = decode_nibble(nibble, pred, idx);
    nibble
}

#[inline]
fn decode_nibble(nibble: u8, pred: &mut i32, idx: &mut usize) -> i16 {
    let step = STEP_TABLE[*idx];
    let mut diff = step >> 3;
    if (nibble & 4) != 0 {
        diff += step;
    }
    if (nibble & 2) != 0 {
        diff += step >> 1;
    }
    if (nibble & 1) != 0 {
        diff += step >> 2;
    }

    if (nibble & 8) != 0 {
        *pred -= diff;
    } else {
        *pred += diff;
    }
    *pred = (*pred).clamp(i16::MIN as i32, i16::MAX as i32);
    *idx = (*idx as isize + INDEX_TABLE[(nibble & 0x0F) as usize] as isize).clamp(0, 88) as usize;
    *pred as i16
}

/// Codifica muestras PCM estéreo intercaladas (`L, R, L, R...`) a un bloque compacto IMA-ADPCM.
pub fn encode_stereo_ima_adpcm(pcm: &[i16]) -> Vec<u8> {
    if pcm.len() < 2 {
        return Vec::new();
    }
    let frames = pcm.len() / 2;
    let mut out = Vec::with_capacity(8 + frames.saturating_sub(1));

    let mut l_pred = pcm[0] as i32;
    let mut l_idx = if frames >= 2 {
        initial_step_index((pcm[2] as i32 - pcm[0] as i32).abs())
    } else {
        0
    };
    let mut r_pred = pcm[1] as i32;
    let mut r_idx = if frames >= 2 {
        initial_step_index((pcm[3] as i32 - pcm[1] as i32).abs())
    } else {
        0
    };

    let [l0, l1] = pcm[0].to_le_bytes();
    out.extend_from_slice(&[l0, l1, l_idx as u8, 0]);
    let [r0, r1] = pcm[1].to_le_bytes();
    out.extend_from_slice(&[r0, r1, r_idx as u8, 0]);

    for f in 1..frames {
        let nl = encode_sample(pcm[f * 2], &mut l_pred, &mut l_idx);
        let nr = encode_sample(pcm[f * 2 + 1], &mut r_pred, &mut r_idx);
        out.push((nl & 0x0F) | ((nr & 0x0F) << 4));
    }
    out
}

/// Decodifica un bloque IMA-ADPCM estéreo a muestras PCM `16-bit` intercaladas (`L, R, L, R...`).
pub fn decode_stereo_ima_adpcm(data: &[u8]) -> Vec<i16> {
    if data.len() < 8 {
        return Vec::new();
    }
    let mut l_pred = i16::from_le_bytes([data[0], data[1]]) as i32;
    let mut l_idx = (data[2] as usize).min(88);
    let mut r_pred = i16::from_le_bytes([data[4], data[5]]) as i32;
    let mut r_idx = (data[6] as usize).min(88);

    let mut pcm = Vec::with_capacity((1 + data.len() - 8) * 2);
    pcm.push(l_pred as i16);
    pcm.push(r_pred as i16);

    for &byte in &data[8..] {
        pcm.push(decode_nibble(byte & 0x0F, &mut l_pred, &mut l_idx));
        pcm.push(decode_nibble((byte >> 4) & 0x0F, &mut r_pred, &mut r_idx));
    }
    pcm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_stereo_sine_preserves_length_and_shape() {
        let frames = 960usize;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f32 / 48_000.0;
            let left = ((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 14_000.0) as i16;
            let right = ((2.0 * std::f32::consts::PI * 880.0 * t).sin() * 10_000.0) as i16;
            pcm.push(left);
            pcm.push(right);
        }

        let encoded = encode_stereo_ima_adpcm(&pcm);
        assert_eq!(encoded.len(), 8 + frames - 1);
        assert!(encoded.len() * 3 < pcm.len() * 2);

        let decoded = decode_stereo_ima_adpcm(&encoded);
        assert_eq!(decoded.len(), pcm.len());

        let max_err = pcm
            .iter()
            .zip(decoded.iter())
            .map(|(&a, &b)| (a as i32 - b as i32).abs())
            .max()
            .unwrap_or(0);
        assert!(max_err < 1_500, "distorsión inesperadamente alta: {max_err}");
    }

    #[test]
    fn handles_empty_and_truncated_buffers_safely() {
        assert!(encode_stereo_ima_adpcm(&[]).is_empty());
        assert!(encode_stereo_ima_adpcm(&[123]).is_empty());
        assert!(decode_stereo_ima_adpcm(&[]).is_empty());
        assert!(decode_stereo_ima_adpcm(&[1, 2, 3, 4]).is_empty());
    }
}
