//! 加密：把内容锁起来，锁的方式跟上游一样
//! Encryption: lock content up, the same way the far end does
//!
//! 上游用的是一种叫 AES-CTR 的锁法。说白了就是：拿一个 16 字节的计数器加密一
//! 遍，得到一串"遮罩"，再把内容跟遮罩逐字节异或。计数器每处理 16 字节就整体
//! 加一（当成一个大整数看，从最后一字节往前进位）。
//!
//! 这里没有直接用现成的 CTR 库，因为库通常把计数器拆成"前缀 + 计数"两段，跟
//! 上游把整个 16 字节当一个大数递增的做法不一样。差一点点，对面就验不过，所以
//! 老老实实照着写。
//!
//! The far end uses a lock style called AES-CTR. In plain words: encrypt a
//! 16-byte counter to get a "mask", then combine the content with that mask byte
//! by byte. Every 16 bytes the counter goes up by one, treated as one big number
//! with the carry moving from the last byte backwards.
//!
//! We do not use an off-the-shelf CTR helper here, because those usually split
//! the counter into "prefix + count", which is not how the far end does it — it
//! bumps all 16 bytes as one big number. Even a small difference fails their
//! check, so we spell it out.

use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;

/// 用 AES-CTR 加密一段内容。
///
/// `key` 和 `iv` 都必须正好 16 字节。`iv` 就是计数器的起始值。
///
/// 顺便说一句：这种锁法加密和解密是同一个动作，同样的 key 和 iv 跑两遍就还原了。
///
/// Encrypt some content with AES-CTR.
///
/// `key` and `iv` must both be exactly 16 bytes. `iv` is where the counter
/// starts.
///
/// Worth knowing: with this lock style, locking and unlocking are the same
/// action — run it twice with the same key and iv and you get the original back.
pub fn aes_ctr(key: &[u8], iv: &[u8], data: &[u8]) -> Vec<u8> {
    assert_eq!(key.len(), 16, "密钥必须为 16 字节 / key must be 16 bytes");
    assert_eq!(iv.len(), 16, "初始值必须为 16 字节 / iv must be 16 bytes");

    let cipher = Aes128::new_from_slice(key).expect("16 字节密钥 / 16-byte key");

    let mut counter = [0u8; 16];
    counter.copy_from_slice(iv);

    let mut out = Vec::with_capacity(data.len());
    let mut offset = 0;

    while offset < data.len() {
        // 加密当前计数器，得到这一段的遮罩。
        // Encrypt the current counter to get this chunk's mask.
        let mut mask = counter;
        cipher.encrypt_block((&mut mask).into());

        // 内容跟遮罩逐字节异或。最后一段可能不满 16 字节，按实际长度来。
        // Combine content with mask byte by byte. The last chunk may be shorter
        // than 16 bytes, so use whatever is left.
        let take = (data.len() - offset).min(16);
        for i in 0..take {
            out.push(data[offset + i] ^ mask[i]);
        }

        bump_counter(&mut counter);
        offset += 16;
    }

    out
}

/// 计数器加一：从最后一字节开始加，满 256 就进位到前一字节。
///
/// 全部都是 255 的话，加完变成全 0，就不再往前进位了 —— 跟上游的写法一致。
///
/// Add one to the counter: start at the last byte, and carry into the byte before
/// it when one overflows.
///
/// If every byte is 255 it wraps to all zeros and stops carrying — same as the
/// far end does it.
fn bump_counter(counter: &mut [u8; 16]) {
    let mut i = 15usize;
    loop {
        counter[i] = counter[i].wrapping_add(1);
        if counter[i] != 0 {
            // 没溢出，不用进位，结束。
            // No overflow, no carry needed, done.
            break;
        }
        if i == 0 {
            // 已经到最前面了，不再往前。
            // Already at the front, nowhere left to carry.
            break;
        }
        i -= 1;
    }
}

/// 把字节转成十六进制字符串，上游要的就是这个格式。
/// Turn bytes into a hex string, which is the format the far end wants.
pub fn to_hex(data: &[u8]) -> String {
    hex::encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 加解密可还原_encrypt_then_decrypt_gives_original() {
        let key = *b"0123456789abcdef";
        let iv = *b"fedcba9876543210";
        let plain = b"hello world, this is a test payload!";

        let locked = aes_ctr(&key, &iv, plain);

        // 同样的 key 和 iv 再跑一遍就还原了。
        // Running it again with the same key and iv gives the original back.
        assert_eq!(aes_ctr(&key, &iv, &locked), plain);

        // 加密后的内容跟原文不一样，说明真的动过。
        // The locked content differs from the original, so something happened.
        assert_ne!(locked.as_slice(), plain.as_slice());
    }

    #[test]
    fn 计数器进位正常_counter_carries_correctly() {
        let mut c = [0u8; 16];

        c[15] = 255;
        bump_counter(&mut c);
        assert_eq!(c[15], 0, "最后一字节应该归零 / last byte should wrap to 0");
        assert_eq!(c[14], 1, "应该进位到前一字节 / carry should reach the byte before");

        // 全部 255 时，加完应该全 0，而且不会崩。
        // All 255s should wrap to all zeros without panicking.
        let mut all_max = [255u8; 16];
        bump_counter(&mut all_max);
        assert_eq!(all_max, [0u8; 16]);
    }

    #[test]
    fn 长度不足十六字节也行_short_input_works() {
        let key = *b"0123456789abcdef";
        let iv = *b"0123456789abcdef";

        // 只有 3 字节，不满一整段。
        // Only 3 bytes, less than one full chunk.
        let out = aes_ctr(&key, &iv, b"abc");
        assert_eq!(out.len(), 3, "输出长度应该跟输入一样 / output length should match input");
        assert_eq!(aes_ctr(&key, &iv, &out), b"abc");
    }

    #[test]
    fn 跨段进位正常_carry_across_chunks_works() {
        let key = *b"0123456789abcdef";
        let mut iv = *b"0123456789abcdef";
        iv[15] = 0xfe; // 第二段就会触发进位 / the second chunk triggers a carry

        let data = vec![0u8; 48]; // 三整段 / three full chunks
        let out = aes_ctr(&key, &iv, &data);

        assert_eq!(out.len(), 48);
        assert_eq!(aes_ctr(&key, &iv, &out), data);
    }
}
