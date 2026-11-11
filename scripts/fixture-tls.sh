#!/bin/sh
# 功能：专用容器 TLS 与 SSH 私钥验收准备；时间：2026-11-10；作者：lq。
set -eu
QDS6_DOCKER="${QDS6_DOCKER:-/Applications/Docker.app/Contents/Resources/bin/docker}"
QDS6_FIXTURE_DIR="$(pwd)/output/fixtures"
mkdir -p "$QDS6_FIXTURE_DIR"
chmod 700 "$QDS6_FIXTURE_DIR"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$QDS6_FIXTURE_DIR/ca.key" -out "$QDS6_FIXTURE_DIR/ca.crt" -days 7 -subj /CN=QDS6-fixture-CA -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign,cRLSign'
openssl req -new -newkey rsa:2048 -nodes -keyout "$QDS6_FIXTURE_DIR/server.key" -out "$QDS6_FIXTURE_DIR/server.csr" -subj /CN=localhost
cat > "$QDS6_FIXTURE_DIR/extensions.cnf" <<'EOF'
# 功能：TLS 主机名校验约束；时间：2026-11-10；作者：lq。
subjectAltName=DNS:localhost
basicConstraints=critical,CA:FALSE
extendedKeyUsage=serverAuth
keyUsage=critical,digitalSignature,keyEncipherment
EOF
openssl x509 -req -in "$QDS6_FIXTURE_DIR/server.csr" -CA "$QDS6_FIXTURE_DIR/ca.crt" -CAkey "$QDS6_FIXTURE_DIR/ca.key" -CAcreateserial -out "$QDS6_FIXTURE_DIR/server.crt" -days 7 -extfile "$QDS6_FIXTURE_DIR/extensions.cnf"
chmod 600 "$QDS6_FIXTURE_DIR"/*.key
for QDS6_FIXTURE_DB in pg mysql; do
    for QDS6_CERT_FILE in server.key server.crt ca.crt; do
        "$QDS6_DOCKER" cp "$QDS6_FIXTURE_DIR/$QDS6_CERT_FILE" "qds6-fixture-$QDS6_FIXTURE_DB:/tmp/qds6-$QDS6_CERT_FILE"
    done
done
"$QDS6_DOCKER" exec -u root qds6-fixture-pg sh -c 'cp /tmp/qds6-server.key /tmp/qds6-server.crt /tmp/qds6-ca.crt /var/lib/postgresql/data/; chown postgres:postgres /var/lib/postgresql/data/qds6-*; chmod 600 /var/lib/postgresql/data/qds6-server.key; printf "\nssl=on\nssl_cert_file=\047qds6-server.crt\047\nssl_key_file=\047qds6-server.key\047\n" >> /var/lib/postgresql/data/postgresql.conf'
"$QDS6_DOCKER" exec -u postgres qds6-fixture-pg pg_ctl reload -D /var/lib/postgresql/data
"$QDS6_DOCKER" exec -u root qds6-fixture-mysql sh -c 'cp /tmp/qds6-server.key /tmp/qds6-server.crt /tmp/qds6-ca.crt /var/lib/mysql/; chown mysql:mysql /var/lib/mysql/qds6-*; chmod 600 /var/lib/mysql/qds6-server.key; printf "\n[mysqld]\nssl-ca=/var/lib/mysql/qds6-ca.crt\nssl-cert=/var/lib/mysql/qds6-server.crt\nssl-key=/var/lib/mysql/qds6-server.key\n" >> /etc/my.cnf'
"$QDS6_DOCKER" restart qds6-fixture-mysql
if [ ! -f "$QDS6_FIXTURE_DIR/id_ed25519" ]; then ssh-keygen -t ed25519 -N '' -f "$QDS6_FIXTURE_DIR/id_ed25519" -q; fi
"$QDS6_DOCKER" cp "$QDS6_FIXTURE_DIR/id_ed25519.pub" qds6-fixture-ssh:/tmp/qds6-authorized-key
"$QDS6_DOCKER" exec -u root qds6-fixture-ssh sh -c 'mkdir -p /home/qds6/.ssh; cp /tmp/qds6-authorized-key /home/qds6/.ssh/authorized_keys; chown -R qds6:qds6 /home/qds6/.ssh; chmod 700 /home/qds6/.ssh; chmod 600 /home/qds6/.ssh/authorized_keys'
