//! 功能：本机兼容 FinalShell DES 与 Navicat 旧版 Blowfish/AES 密码字段，不用于 QDS6 加密；时间：2026-11-11；作者：lq。
// 格式参考与独立 Java 向量来源见 docs/密码迁移实现与验证-20261111.md；不包含或执行外部恢复程序。

use aes::cipher::{
    Block, BlockDecrypt, BlockDecryptMut, BlockEncrypt, KeyInit, KeyIvInit, block_padding::Pkcs7,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use md5::{Digest, Md5};
use sha1::Sha1;
use zeroize::{Zeroize, Zeroizing};

/// 功能：只将明文留在自动清零的临时内存中，报告仅暴露格式名；时间：2026-11-11；作者：lq。
pub(super) struct Password {
    pub value: Zeroizing<String>,
    pub format: &'static str,
}

/// 功能：限制单项密码处理资源，未知格式只返回固定错误，不将来源字段当成明文；时间：2026-11-11；作者：lq。
const MAX_SECRET: usize = 65_536;

/// 功能：按 Java Random 规范兼容 48 位 LCG，特别保留 nextLong 低位的有符号扩展；时间：2026-11-11；作者：lq。
struct JavaRandom(u64);

impl JavaRandom {
    /// 功能：采用 Java 构造器相同的种子扰动，不作为安全随机数生成器；时间：2026-11-11；作者：lq。
    fn new(seed: i64) -> Self {
        Self((seed as u64 ^ 0x5deece66d) & ((1 << 48) - 1))
    }

    /// 功能：复现 Java 的指定高位输出；时间：2026-11-11；作者：lq。
    fn next(&mut self, bits: u32) -> u32 {
        self.0 = self.0.wrapping_mul(0x5deece66d).wrapping_add(11) & ((1 << 48) - 1);
        (self.0 >> (48 - bits)) as u32
    }

    /// 功能：复现 Java nextInt 的二次幂快捷分支和拒绝采样；时间：2026-11-11；作者：lq。
    fn next_int(&mut self, bound: u32) -> i32 {
        if bound.is_power_of_two() {
            return ((bound as u64 * self.next(31) as u64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31) as i32;
            let value = bits % bound as i32;
            if bits.wrapping_sub(value).wrapping_add(bound as i32 - 1) >= 0 {
                return value;
            }
        }
    }

    /// 功能：Java 将两个有符号 int 相加，不能直接按无符号位拼接；时间：2026-11-11；作者：lq。
    fn next_long(&mut self) -> i64 {
        ((self.next(32) as i32 as i64) << 32).wrapping_add(self.next(32) as i32 as i64)
    }
}

impl Drop for JavaRandom {
    /// 功能：释放来源格式派生状态时清零；时间：2026-11-11；作者：lq。
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// 功能：解码 FinalShell 的八字节头部与 DES/ECB/PKCS5 字段，校验边界和 UTF-8；时间：2026-11-11；作者：lq。
pub(super) fn finalshell(encoded: &str) -> Result<Password, &'static str> {
    let error = "FinalShell 密码格式暂不支持或已损坏，需手动补填";
    if encoded.len() > (MAX_SECRET + 24) * 4 / 3 {
        return Err(error);
    }
    let mut bytes = Zeroizing::new(STANDARD.decode(encoded).map_err(|_| error)?);
    if bytes.len() < 16 || !(bytes.len() - 8).is_multiple_of(8) {
        return Err(error);
    }
    let signed = |index: usize| bytes[index] as i8 as i64;
    let divisor = JavaRandom::new(signed(5)).next_int(127) as i64;
    if divisor == 0 {
        return Err(error);
    }
    let mut random = JavaRandom::new(3_680_984_568_597_093_857_i64 / divisor);
    for _ in 0..signed(0).max(0) {
        random.next_long();
    }
    let mut seed = random.next_long();
    let mut second_random = JavaRandom::new(seed);
    seed.zeroize();
    let keys = Zeroizing::new([
        signed(4),
        second_random.next_long(),
        signed(7),
        signed(3),
        second_random.next_long(),
        signed(1),
        random.next_long(),
        signed(2),
    ]);
    let mut material = Zeroizing::new(Vec::with_capacity(64));
    for key in keys.iter() {
        material.extend_from_slice(&key.to_be_bytes());
    }
    let key = Zeroizing::new(<[u8; 16]>::from(Md5::digest(&*material)));
    let cipher = des::Des::new_from_slice(&key[..8]).map_err(|_| error)?;
    for chunk in bytes[8..].as_chunks_mut::<8>().0 {
        let mut block = Block::<des::Des>::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        chunk.copy_from_slice(&block);
        block.as_mut_slice().zeroize();
    }
    let padding = *bytes.last().ok_or(error)? as usize;
    if !(1..=8).contains(&padding)
        || !bytes[bytes.len() - padding..]
            .iter()
            .all(|b| *b as usize == padding)
    {
        return Err(error);
    }
    let end = bytes.len() - padding;
    let value = secret_text(&bytes[8..end]).map_err(|_| error)?;
    Ok(Password {
        value,
        format: "FinalShell DES",
    })
}

/// 功能：兼容 Navicat Password 字段的两种已知格式，不推断主密码或加密归档；时间：2026-11-11；作者：lq。
pub(super) fn navicat(encoded: &str) -> Result<Password, &'static str> {
    let error = "Navicat 密码格式暂不支持或已损坏，需手动补填";
    let bytes = hex(encoded).map_err(|_| error)?;
    if bytes.is_empty() {
        return Err(error);
    }
    // 功能：AES 的严格填充校验优先；旧字段无格式标签时仅接受唯一可解释的明文；时间：2026-11-11；作者：lq。
    let aes = navicat_aes(&bytes).ok();
    let blowfish = navicat_blowfish(&bytes).ok();
    match (aes, blowfish) {
        (Some(aes), None) => Ok(aes),
        (None, Some(blowfish)) => Ok(blowfish),
        (Some(aes), Some(blowfish)) if aes.value == blowfish.value => Ok(aes),
        _ => Err(error),
    }
}

/// 功能：严格读取有界十六进制，拒绝奇数长度、非十六进制和过大字段；时间：2026-11-11；作者：lq。
fn hex(encoded: &str) -> Result<Zeroizing<Vec<u8>>, ()> {
    if !encoded.len().is_multiple_of(2) || encoded.len() > (MAX_SECRET + 16) * 2 {
        return Err(());
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(()),
    };
    let mut result = Zeroizing::new(Vec::with_capacity(encoded.len() / 2));
    for pair in encoded.as_bytes().as_chunks::<2>().0 {
        result.push(digit(pair[0])? * 16 + digit(pair[1])?);
    }
    Ok(result)
}

/// 功能：Navicat AES-128-CBC 字段仅按固定格式解码，严格校验 PKCS7，不用于新密码保存；时间：2026-11-11；作者：lq。
fn navicat_aes(bytes: &[u8]) -> Result<Password, ()> {
    if !bytes.len().is_multiple_of(16) {
        return Err(());
    }
    let mut buffer = Zeroizing::new(bytes.to_vec());
    let plaintext =
        cbc::Decryptor::<aes::Aes128>::new(b"libcckeylibcckey".into(), b"libcciv libcciv ".into())
            .decrypt_padded_mut::<Pkcs7>(&mut buffer)
            .map_err(|_| ())?;
    Ok(Password {
        value: secret_text(plaintext)?,
        format: "Navicat AES",
    })
}

/// 功能：Navicat 旧 Blowfish 字段的异或反馈及非整块尾部兼容；旧格式仅支持可打印 ASCII；时间：2026-11-11；作者：lq。
fn navicat_blowfish(bytes: &[u8]) -> Result<Password, ()> {
    let key = Zeroizing::new(<[u8; 20]>::from(Sha1::digest(b"3DC5CA39")));
    let cipher: blowfish::Blowfish = blowfish::Blowfish::new_from_slice(&*key).map_err(|_| ())?;
    let mut iv = Block::<blowfish::Blowfish>::from([0xff; 8]);
    cipher.encrypt_block(&mut iv);
    let mut plaintext = Zeroizing::new(Vec::with_capacity(bytes.len()));
    let (chunks, remainder) = bytes.as_chunks::<8>();
    for chunk in chunks {
        let mut block = Block::<blowfish::Blowfish>::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        for index in 0..8 {
            plaintext.push(block[index] ^ iv[index]);
            iv[index] ^= chunk[index];
        }
        block.as_mut_slice().zeroize();
    }
    if !remainder.is_empty() {
        cipher.encrypt_block(&mut iv);
        for (byte, mask) in remainder.iter().zip(iv.iter()) {
            plaintext.push(byte ^ mask);
        }
    }
    iv.as_mut_slice().zeroize();
    if !plaintext.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        return Err(());
    }
    Ok(Password {
        value: secret_text(&plaintext)?,
        format: "Navicat Blowfish",
    })
}

/// 功能：完整保留密码中的空格和 Unicode，拒绝无效 UTF-8、NUL 和超长文本；时间：2026-11-11；作者：lq。
fn secret_text(bytes: &[u8]) -> Result<Zeroizing<String>, ()> {
    if bytes.len() > MAX_SECRET || bytes.contains(&0) {
        return Err(());
    }
    Ok(Zeroizing::new(
        std::str::from_utf8(bytes).map_err(|_| ())?.to_owned(),
    ))
}

// 功能：独立 JDK 向量和畸形输入验证，不接触用户密码或系统凭据库；时间：2026-11-11；作者：lq。
#[cfg(test)]
mod tests {
    use super::*;

    /// 功能：只反序列化仓库中的公开合成测试口令，不提供 Debug 防止误用于秘密输出；时间：2026-11-11；作者：lq。
    #[derive(serde::Deserialize)]
    struct Vector {
        format: String,
        plaintext: String,
        encoded: String,
    }

    /// 功能：JDK 标准库生成的 36 条向量覆盖符号扩展、整块、尾部、空密码与 Unicode；时间：2026-11-11；作者：lq。
    #[test]
    fn independent_java_vectors_decode_exactly() {
        let vectors: Vec<Vector> =
            serde_json::from_str(include_str!("fixtures/passwords.json")).unwrap();
        assert_eq!(vectors.len(), 36);
        for (index, vector) in vectors.into_iter().enumerate() {
            let password = if vector.format == "finalshell" {
                finalshell(&vector.encoded)
            } else {
                navicat(&vector.encoded)
            }
            .unwrap_or_else(|_| panic!("synthetic vector {index} failed"));
            assert!(
                password.value.as_str() == vector.plaintext,
                "synthetic vector {index} mismatch"
            );
        }
        // 功能：同时核对上游公开已知答案，避免两种语言误用同一套错误常量；时间：2026-11-11；作者：lq。
        assert!(navicat("15057D7BA390").unwrap().value.as_str() == "123456");
    }

    /// 功能：固定错误不包含来源数据，拒绝无效编码、长度、填充、NUL 和超限输入；时间：2026-11-11；作者：lq。
    #[test]
    fn invalid_password_fields_fail_closed_without_echoing() {
        for encoded in [
            "sensitive-source-value",
            "",
            "AAAA",
            "AAAAAAAAAAAAAAAAAAAAAA==",
            "***",
        ] {
            let error = finalshell(encoded).err().expect("invalid FinalShell field");
            assert_eq!(error, "FinalShell 密码格式暂不支持或已损坏，需手动补填");
        }
        for encoded in [
            "",
            "X0",
            "123",
            "not-a-password",
            "00".repeat(MAX_SECRET + 17).as_str(),
        ] {
            assert!(navicat(encoded).is_err());
        }
        assert!(finalshell(&"A".repeat(MAX_SECRET * 2)).is_err());
        assert!(secret_text(b"password\0suffix").is_err());
        assert!(secret_text(&[0xff]).is_err());
        assert!(secret_text(&vec![b'a'; MAX_SECRET + 1]).is_err());
        assert!(secret_text(b"  preserved  ").unwrap().as_str() == "  preserved  ");
    }

    /// 功能：有界伪随机数据覆盖头部和块边界，未知字段不得导致越界或恐慌；时间：2026-11-11；作者：lq。
    #[test]
    fn arbitrary_ciphertext_does_not_panic() {
        use rand::{RngCore, SeedableRng};
        let mut random = rand::rngs::StdRng::seed_from_u64(20261111);
        for length in 0..256 {
            let mut bytes = vec![0; length];
            random.fill_bytes(&mut bytes);
            let _ = finalshell(&STANDARD.encode(&bytes));
            let encoded = bytes.iter().map(|b| format!("{b:02X}")).collect::<String>();
            let _ = navicat(&encoded);
        }
    }
}
