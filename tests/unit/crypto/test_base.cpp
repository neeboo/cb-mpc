#include <gtest/gtest.h>

#include <cbmpc/crypto/base.h>

#include "utils/test_macros.h"

using namespace coinbase;
using namespace coinbase::crypto;

TEST(BaseTest, TestError) {
  auto err = error("Test error");
  EXPECT_NE(err, 0);  // Just check that it returns something non-zero if that's expected
}

TEST(BaseTest, TestOpensslError) {
  // Simulate an error:
  auto err = openssl_error("Simulated openssl error");
  EXPECT_NE(err, 0);

  // Another version with int return
  auto err2 = openssl_error(-1, "Another error");
  EXPECT_NE(err2, 0);

  // Check the error string
  auto err_str = openssl_get_last_error_string();
  // The actual string might differ on your setup; just ensure it isn't empty.
  EXPECT_FALSE(err_str.empty());
}

TEST(BaseTest, TestSeedRandomAndGenRandom) {
  buf_t seed = buf_t("test");
  seed_random(seed);

  buf_t random_data = gen_random(32);  // Generate 32 random bytes
  ASSERT_EQ(random_data.size(), 32);
  seed_random(seed);
  buf_t random_data2 = gen_random(32);  // Generate 32 random bytes
  ASSERT_EQ(random_data2.size(), 32);
  EXPECT_NE(random_data, random_data2);
}

TEST(BaseTest, TestGenRandomBitlen) {
  buf_t bit_data = gen_random_bitlen(128);  // 128 bits
  // 128 bits = 16 bytes
  EXPECT_EQ(bit_data.size(), 16);
}

TEST(BaseTest, TestGenRandomHelpers) {
  // Test gen_random_bits
  bits_t bits = gen_random_bits(10);
  EXPECT_EQ(bits.count(), 10);

  // Test gen_random_bufs128
  auto bufs128 = gen_random_bufs128(5);
  EXPECT_EQ(bufs128.size(), 5);

  // Test gen_random_bool
  bool random_bool = gen_random_bool();
  SUCCEED() << "Generated a random bool: " << (random_bool ? "true" : "false");

  // Test gen_random_int<uint32_t>
  auto r_int = gen_random_int<uint32_t>();
  SUCCEED() << "Generated a random int: " << r_int;
}

TEST(BaseTest, TestSecureEqu) {
  byte_t arr1[] = {0x01, 0x02, 0x03};
  byte_t arr2[] = {0x01, 0x02, 0x03};
  byte_t arr3[] = {0x01, 0x03, 0x03};

  mem_t mem_a1(arr1, 3), mem_a2(arr2, 3), mem_a3(arr3, 3);

  EXPECT_TRUE(secure_equ(mem_a1, mem_a2));
  EXPECT_FALSE(secure_equ(mem_a1, mem_a3));
  EXPECT_TRUE(secure_equ(arr1, arr2, 3));
  EXPECT_FALSE(secure_equ(arr2, arr3, 3));
}

TEST(BaseTest, TestAES_CTR) {
  buf_t key = bn_t(0x00).to_bin(16);
  buf_t iv = bn_t(0x01).to_bin(16);
  buf_t data = bn_t(0x02).to_bin(32);

  buf_t enc = aes_ctr_t::encrypt(key, iv.data(), data);
  buf_t dec = aes_ctr_t::decrypt(key, iv.data(), enc);

  EXPECT_EQ(dec, data);
}

TEST(BaseTest, TestDRBG) {
  buf_t seed = bn_t(0xAB).to_bin(32);
  drbg_aes_ctr_t drbg(seed);

  // Initialize and generate some data
  drbg.init();
  buf_t random_data = drbg.gen(16);
  EXPECT_EQ(random_data.size(), 16);

  // Reseed
  buf_t more_seed = bn_t(0xCD).to_bin(32);
  drbg.seed(more_seed);
  buf_t second_data = drbg.gen(16);
  EXPECT_EQ(second_data.size(), 16);

  // RNG might differ, so we won't strictly compare random_data vs second_data
}

TEST(BaseTest, TestRandomShuffle) {
  buf128_t key{};
  std::vector<int> test_vec = {1, 2, 3, 4, 5};
  random_shuffle(key, test_vec, static_cast<int>(test_vec.size()));
  SUCCEED() << "Vector after shuffle: [" << test_vec[0] << ", " << test_vec[1] << ", " << test_vec[2] << ", "
            << test_vec[3] << ", " << test_vec[4] << "]";
  // We just confirm that the function runs
}

TEST(BaseTest, TestAES_GCM) {
  buf_t key = bn_t(0x00).to_bin(16);
  buf_t iv = bn_t(0x01).to_bin(12);
  buf_t auth = bn_t(0x02).to_bin(16);
  buf_t data = bn_t(0x03).to_bin(32);

  buf_t enc;
  aes_gcm_t::encrypt(key, iv, auth, /*tag_size=*/16, data, enc);

  buf_t dec;
  EXPECT_OK(aes_gcm_t::decrypt(key, iv, auth, /*tag_size=*/16, enc, dec));
  EXPECT_EQ(dec, data);
}

TEST(BaseTest, TestAES_GMAC) {
  buf_t key = bn_t(0xAA).to_bin(16);
  buf_t iv = bn_t(0xBB).to_bin(12);
  buf_t data = bn_t(0xCC).to_bin(64);
  int out_size = 16;

  buf_t tag = aes_gmac_t::calculate(key, iv, data, out_size);
  EXPECT_EQ(tag.size(), out_size);
}