set -e

cd /tmp
curl -L https://github.com/openssl/openssl/releases/download/openssl-3.2.0/openssl-3.2.0.tar.gz --output openssl-3.2.0.tar.gz
expectedHash='14c826f07c7e433706fb5c69fa9e25dab95684844b4c962a2cf1bf183eb4690e'
fileHash=$(sha256sum openssl-3.2.0.tar.gz | cut -d " " -f 1 )

if [ $expectedHash != $fileHash ]
then
  echo 'ERROR: SHA1 DOES NOT MATCH!'
  echo 'expected: ' $expectedHash
  echo 'file:     ' $fileHash
  exit 1
fi


tar -xzf openssl-3.2.0.tar.gz
cd openssl-3.2.0
sed -i -e 's/^static//' crypto/ec/curve25519.c

sudo mkdir -p /usr/local/opt/openssl@3.2.0/lib
sudo mkdir -p /usr/local/opt/openssl@3.2.0/include
sudo mkdir -p /usr/local/opt/openssl@3.2.0/include/openssl

sudo ./Configure -g3 -static -DOPENSSL_THREADS no-shared \
  no-afalgeng no-apps no-aria no-autoload-config no-bf no-camellia no-cast no-chacha no-cmac no-cms no-crypto-mdebug \
  no-comp no-cmp no-ct no-des no-dh no-dgram no-dsa no-dso no-dtls no-dynamic-engine no-ec2m no-egd no-engine no-external-tests \
  no-gost no-http no-idea no-mdc2 no-md2 no-md4 no-module no-nextprotoneg no-ocb no-ocsp no-psk no-padlockeng no-poly1305 \
  no-quic no-rc2 no-rc4 no-rc5 no-rfc3779 no-scrypt no-sctp no-seed no-siphash no-sm2 no-sm3 no-sm4 no-sock no-srtp no-srp \
  no-ssl-trace no-ssl3 no-stdio no-tests no-tls no-ts no-unit-test no-uplink no-whirlpool no-zlib \
  --prefix=/usr/local/opt/openssl@3.2.0 darwin64-x86_64-cc

make -j
make install_sw
make clean
