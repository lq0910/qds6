#!/bin/sh
# 功能：启动专用隔离验收服务，仅绑定回环端口；时间：2026-11-10；作者：lq。
set -eu
QDS6_DOCKER="${QDS6_DOCKER:-/Applications/Docker.app/Contents/Resources/bin/docker}"
case "${1:-start}" in
  start)
    "$QDS6_DOCKER" run -d --name qds6-fixture-pg -p 127.0.0.1:25432:5432 -e POSTGRES_USER=qds6 -e POSTGRES_PASSWORD=qds6-fixture-only -e POSTGRES_DB=qds6 postgres:17
    "$QDS6_DOCKER" run -d --name qds6-fixture-mysql -p 127.0.0.1:23306:3306 -e MYSQL_ROOT_PASSWORD=qds6-fixture-only -e MYSQL_USER=qds6 -e MYSQL_PASSWORD=qds6-fixture-only -e MYSQL_DATABASE=qds6 mysql:8.4
    "$QDS6_DOCKER" build -t qds6-fixture-ssh -f scripts/fixtures/Dockerfile.ssh .
    "$QDS6_DOCKER" run -d --name qds6-fixture-ssh -p 127.0.0.1:22222:22 qds6-fixture-ssh
    ;;
  stop)
    # 功能：只删除此脚本创建的专用临时容器，不碰其他容器或业务卷；时间：2026-11-10；作者：lq。
    "$QDS6_DOCKER" rm -f qds6-fixture-pg qds6-fixture-mysql qds6-fixture-ssh
    ;;
  *) exit 2 ;;
esac
