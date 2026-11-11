/* 功能：使用独立 JDK 标准加密库生成纯合成跨语言密码迁移向量；时间：2026-11-11；作者：lq。
 * 只生成测试口令，不读取用户配置；格式来源和许可记录见 docs/密码迁移实现与验证-20261111.md。
 */
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.Base64;
import java.util.HexFormat;
import java.util.Random;
import javax.crypto.Cipher;
import javax.crypto.spec.IvParameterSpec;
import javax.crypto.spec.SecretKeySpec;

public class GenerateMigrationPasswords {
    // 功能：测试 FinalShell 有符号头部、Java nextLong 和 DES 字段协议；时间：2026-11-11；作者：lq。
    static String finalshell(String plaintext, byte[] head) throws Exception {
        int divisor = new Random((long)head[5]).nextInt(127);
        Random random = new Random(3680984568597093857L / divisor);
        for (int i = 0; i < head[0]; i++) random.nextLong();
        Random secondRandom = new Random(random.nextLong());
        long[] parts = {head[4], secondRandom.nextLong(), head[7], head[3],
            secondRandom.nextLong(), head[1], random.nextLong(), head[2]};
        ByteBuffer material = ByteBuffer.allocate(64);
        for (long part : parts) material.putLong(part);
        byte[] key = Arrays.copyOf(MessageDigest.getInstance("MD5").digest(material.array()), 8);
        Cipher cipher = Cipher.getInstance("DES/ECB/PKCS5Padding");
        cipher.init(Cipher.ENCRYPT_MODE, new SecretKeySpec(key, "DES"));
        byte[] encrypted = cipher.doFinal(plaintext.getBytes(StandardCharsets.UTF_8));
        return Base64.getEncoder().encodeToString(ByteBuffer.allocate(8 + encrypted.length).put(head).put(encrypted).array());
    }

    // 功能：标准 JCE AES/CBC/PKCS5Padding 独立验证 Navicat 新字段；时间：2026-11-11；作者：lq。
    static String navicatAes(String plaintext) throws Exception {
        Cipher cipher = Cipher.getInstance("AES/CBC/PKCS5Padding");
        cipher.init(Cipher.ENCRYPT_MODE,
            new SecretKeySpec("libcckeylibcckey".getBytes(StandardCharsets.US_ASCII), "AES"),
            new IvParameterSpec("libcciv libcciv ".getBytes(StandardCharsets.US_ASCII)));
        return HexFormat.of().withUpperCase().formatHex(cipher.doFinal(plaintext.getBytes(StandardCharsets.UTF_8)));
    }

    // 功能：标准 JCE Blowfish/ECB 与格式反馈规则覆盖整块和尾部；时间：2026-11-11；作者：lq。
    static String navicatBlowfish(String plaintext) throws Exception {
        byte[] key = MessageDigest.getInstance("SHA-1").digest("3DC5CA39".getBytes(StandardCharsets.US_ASCII));
        Cipher cipher = Cipher.getInstance("Blowfish/ECB/NoPadding");
        cipher.init(Cipher.ENCRYPT_MODE, new SecretKeySpec(key, "Blowfish"));
        byte[] iv = new byte[8];
        Arrays.fill(iv, (byte)0xff);
        iv = cipher.doFinal(iv);
        byte[] bytes = plaintext.getBytes(StandardCharsets.US_ASCII);
        byte[] out = new byte[bytes.length];
        int offset = 0;
        for (; offset + 8 <= bytes.length; offset += 8) {
            byte[] block = Arrays.copyOfRange(bytes, offset, offset + 8);
            for (int i = 0; i < 8; i++) block[i] ^= iv[i];
            block = cipher.doFinal(block);
            System.arraycopy(block, 0, out, offset, 8);
            for (int i = 0; i < 8; i++) iv[i] ^= block[i];
        }
        if (offset < bytes.length) {
            byte[] mask = cipher.doFinal(iv);
            for (int i = offset; i < bytes.length; i++) out[i] = (byte)(bytes[i] ^ mask[i-offset]);
        }
        return HexFormat.of().withUpperCase().formatHex(out);
    }

    // 功能：输出固定合成 JSON，不依赖额外序列化工具；时间：2026-11-11；作者：lq。
    static String quote(String value) {
        return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"")
            .replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t").replace("\0", "\\u0000") + "\"";
    }

    // 功能：生成格式、预期口令和密文均为公开合成数据的向量；时间：2026-11-11；作者：lq。
    static void vector(String format, String plaintext, String encoded, boolean first) {
        System.out.print((first ? "" : ",\n") + "  {\"format\":" + quote(format)
            + ",\"plaintext\":" + quote(plaintext) + ",\"encoded\":" + quote(encoded) + "}");
    }

    // 功能：固定输入使跨语言向量可重复生成，包含空格、Unicode 和八字节边界；时间：2026-11-11；作者：lq。
    public static void main(String[] args) throws Exception {
        System.out.println("[");
        String[] values = {"", "123456", "12345678", "qds6-fixture-only", " 密码 🚀 ", "  preserve spaces  "};
        byte[][] heads = {
            {1,2,3,4,5,6,7,8},
            {-128,127,-3,4,-5,126,7,-8},
            {0,-1,0,4,0,-128,7,127},
            {127,-128,1,4,127,-128,7,-128}
        };
        boolean first = true;
        for (byte[] head : heads) for (String value : values) {
            vector("finalshell", value, finalshell(value, head), first);
            first = false;
        }
        for (String value : values) vector("navicat-aes", value, navicatAes(value), false);
        for (String value : new String[]{"a", "123456", "12345678", "123456789", "qds6-fixture-only", "  preserve spaces  "}) {
            vector("navicat-blowfish", value, navicatBlowfish(value), false);
        }
        System.out.println("\n]");
    }
}
