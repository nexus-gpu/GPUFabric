# GPUFabric OCR/P2P gpuf-s 生产上线与回滚计划 - 2026-07-10

## 目标

本计划用于 `ssh pro` 生产环境上线当前 OCR 多模态、P2P consumer-only、P2P 计量、网关直连计量相关改动。计划重点是先备份、再小范围发布、最后验证计量链路；回滚顺序遵循“先服务、后配置、数据库最后兜底”。

参考运维记录：

- `/home/jack/桌面/working/db/prod-db.md`
- `docs/banking-admin-api-deploy-rollback-20260705.md`
- `scripts/prod_schema_add_compute_map_token.sql`
- `scripts/prod_schema_add_compute_map_token_rollback.sql`

## 上线范围

本次建议发布：

- `gpuf-s` 主服务：OCR 多模态网关、P2P consumer-only session、P2P usage report/receipt 校验、直连网关计量。
- `inference_token_usage` 相关 schema：如生产库已存在则只校验，不重复变更；缺失时运行幂等新增脚本。
- 可选发布 `gpuf-p2p-proxy`：轻量 P2P 转发客户端，不作为生产算力设备上线。

本次默认不发布：

- `new-api` 容器：只验证 `gpuf-s -> Kafka -> new-api` 旧计量链路，不改镜像。
- `api_server`：除非同时要上线管理后台接口二进制，否则只记录健康状态和二进制 hash。
- PostgreSQL、Redis、Kafka 容器本身：只做备份和状态记录，不重启。

## 对旧版 gpuf-c 的影响

旧版 `gpuf-c` 的普通功能不受影响：

- 设备登录、心跳、在线状态、算力统计仍走 `CommandV1::Login/Heartbeat`。
- 普通文本 chat/completion 分享仍走 `CommandV1::ChatInferenceTask`。
- 低版本或移动端 embedding client 会被调度层跳过，不会强行下发 embedding 任务。

需要限制的新能力：

- OCR、多模态图片请求需要新版 `gpuf-c 1.0.4` 或已验证版本。
- P2P OCR 计量需要目标算力端回传 `P2PUsageReceipt`，因此目标端也应使用新版。
- 旧版 `gpuf-c` 不要配置或分享 `PaddleOCR-VL-1.6-GGUF` 这类 OCR 模型，避免被调度到不支持多模态协议的客户端。

对外 API 可能可见的非破坏性变化：

- `/v1/chat/completions` 网关直连响应会带 `p2p.enabled=false, transport=gateway, fallback=false`。
- streaming chunk 里会补充 `client_id` 与 `p2p` 元信息。
- OCR 图片请求的计量 endpoint 会记录为 `ocr.image`，普通多模态为 `multimodal.chat`，纯文本仍为 `chat.completion`。

## 生产环境路径

按现有运维记录，生产环境关键路径如下：

- 主机：`ssh pro`
- PostgreSQL 容器：`postgres`
- PostgreSQL compose：`/srv/gpunexus_com/deploy/postgres/docker-compose.yml`
- PostgreSQL conf：
  - `/mnt/gpunexus_com/postgres/conf/postgresql.conf`
  - `/mnt/gpunexus_com/postgres/conf/pg_hba.conf`
- PostgreSQL data：`/mnt/gpunexus_com/postgres/postgres_data`
- PostgreSQL logs：`/mnt/gpunexus_com/postgres/logs`
- `gpuf-s` 推荐当前生产目录：`/home/ubuntu/v1.0.4/gpuf-s`
- `gpuf-s` 配置：`/home/ubuntu/v1.0.4/gpuf-s/prod.env`
- `gpuf-s` 启动脚本：`/home/ubuntu/v1.0.4/gpuf-s/start_gpuf-s_tls.sh`
- `api_server` 旧记录路径：`/home/ubuntu/v1.0.3/api_server`
- `new-api` 部署目录：`/srv/gpunexus_com/deploy/new-api`
- Kafka 关键 topic：`request-message`, `client-heartbeats`

上线前必须在 `ssh pro` 再确认一次实际路径，不能只依赖旧记录。

## 上线前本地检查

在本地仓库执行：

```bash
cargo fmt --all --check
cargo test -p gpuf-s
cargo build -p gpuf-s --release
cargo build -p gpuf-p2p-proxy --release
sha256sum target/release/gpuf-s target/release/gpuf-p2p-proxy
```

如生产机 glibc 与本地构建不兼容，使用静态或目标环境构建策略：

```bash
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C target-feature=+crt-static" cargo build -p gpuf-s --release --target x86_64-unknown-linux-gnu
```

上线前记录：

- 当前 git branch、commit、dirty 状态。
- 产物 sha256。
- 当前测试环境已验证的 OCR gateway 请求、P2P 请求、stream 请求、Kafka 旧计量链路结果。

## 生产预检

在 `ssh pro` 上只读检查：

```bash
date
hostname
df -h
ps -ef | grep -E "gpuf-s|api_server|heartbeat_consumer" | grep -v grep
ss -ltnp | grep -E "17000|17001|8081|18081|5432|9092|29092"
docker ps --format "table {{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}"
```

检查 `gpuf-s` 当前路径与启动方式：

```bash
cd /home/ubuntu/v1.0.4/gpuf-s
pwd
ls -la
sha256sum gpuf-s prod.env start_gpuf-s_tls.sh
test -f gpuf-s.pid && cat gpuf-s.pid
test -f gpuf-s.logpath && cat gpuf-s.logpath
```

检查数据库 schema 是否已具备：

```bash
docker exec postgres psql -U <db_user> -d <db_name> -c "\d public.inference_token_usage"
docker exec postgres psql -U <db_user> -d <db_name> -c "\d public.gpu_assets"
```

如果 `inference_token_usage` 或 `gpu_assets.geo_*` 缺失，才运行 `scripts/prod_schema_add_compute_map_token.sql`。该脚本是幂等新增，但仍必须在完整备份后执行。

## 备份计划

统一时间戳：

```bash
TS="$(date +%Y%m%d_%H%M%S)"
```

### 1. gpuf-s 服务与配置备份

备份目录：

```text
/home/ubuntu/v1.0.4/gpuf-s/backups/ocr-p2p-${TS}/
```

备份内容：

- `gpuf-s` 当前二进制。
- `prod.env` 当前配置。
- `start_gpuf-s_tls.sh` 当前启动脚本。
- `gpuf-s.pid`, `gpuf-s.logpath` 如存在。
- 当前进程 argv、cwd、端口监听、二进制 sha256。
- TLS 证书只记录 hash 和路径；如需要复制证书，不在文档记录私钥内容。

示例：

```bash
cd /home/ubuntu/v1.0.4/gpuf-s
mkdir -p backups/ocr-p2p-${TS}
cp -a gpuf-s backups/ocr-p2p-${TS}/gpuf-s.before
cp -a prod.env backups/ocr-p2p-${TS}/prod.env.before
cp -a start_gpuf-s_tls.sh backups/ocr-p2p-${TS}/start_gpuf-s_tls.sh.before
test -f gpuf-s.pid && cp -a gpuf-s.pid backups/ocr-p2p-${TS}/gpuf-s.pid.before
test -f gpuf-s.logpath && cp -a gpuf-s.logpath backups/ocr-p2p-${TS}/gpuf-s.logpath.before
sha256sum gpuf-s prod.env start_gpuf-s_tls.sh > backups/ocr-p2p-${TS}/sha256.before.txt
ps -ef | grep "gpuf-s" | grep -v grep > backups/ocr-p2p-${TS}/process.before.txt
ss -ltnp | grep -E "17000|17001|8081" > backups/ocr-p2p-${TS}/ports.before.txt
```

### 2. 数据库备份

备份目录：

```text
/srv/gpunexus_com/deploy/postgres/backups/${TS}_gpuf_ocr_p2p/
```

备份内容：

- 生产库完整 custom dump。
- schema-only dump。
- 重点表数据快照：`gpu_assets`, `inference_token_usage`, `request_device_map`，以及 token/client 相关表按实际表名补充。
- PostgreSQL 配置文件备份。
- dump 文件 sha256。

示例：

```bash
mkdir -p /srv/gpunexus_com/deploy/postgres/backups/${TS}_gpuf_ocr_p2p
cp -a /mnt/gpunexus_com/postgres/conf/postgresql.conf /srv/gpunexus_com/deploy/postgres/backups/${TS}_gpuf_ocr_p2p/postgresql.conf.before
cp -a /mnt/gpunexus_com/postgres/conf/pg_hba.conf /srv/gpunexus_com/deploy/postgres/backups/${TS}_gpuf_ocr_p2p/pg_hba.conf.before
docker exec postgres pg_dump -U <db_user> -d <db_name> -Fc -f /tmp/gpuf_full_${TS}.dump
docker cp postgres:/tmp/gpuf_full_${TS}.dump /srv/gpunexus_com/deploy/postgres/backups/${TS}_gpuf_ocr_p2p/gpuf_full_${TS}.dump
docker exec postgres pg_dump -U <db_user> -d <db_name> --schema-only -f /tmp/gpuf_schema_${TS}.sql
docker cp postgres:/tmp/gpuf_schema_${TS}.sql /srv/gpunexus_com/deploy/postgres/backups/${TS}_gpuf_ocr_p2p/gpuf_schema_${TS}.sql
sha256sum /srv/gpunexus_com/deploy/postgres/backups/${TS}_gpuf_ocr_p2p/* > /srv/gpunexus_com/deploy/postgres/backups/${TS}_gpuf_ocr_p2p/sha256.txt
```

重点表快照示例：

```bash
docker exec postgres pg_dump -U <db_user> -d <db_name> -t public.gpu_assets -t public.inference_token_usage -t public.request_device_map -Fc -f /tmp/gpuf_key_tables_${TS}.dump
docker cp postgres:/tmp/gpuf_key_tables_${TS}.dump /srv/gpunexus_com/deploy/postgres/backups/${TS}_gpuf_ocr_p2p/gpuf_key_tables_${TS}.dump
```

### 3. new-api 状态备份

如果本次不发布 `new-api`，只记录状态，不重启：

```bash
cd /srv/gpunexus_com/deploy/new-api
mkdir -p backups/ocr-p2p-${TS}
cp -a docker-compose.ha.yml backups/ocr-p2p-${TS}/docker-compose.ha.yml.before
test -f .env && cp -a .env backups/ocr-p2p-${TS}/env.before
docker ps --format "table {{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}" > backups/ocr-p2p-${TS}/docker-ps.before.txt
docker images > backups/ocr-p2p-${TS}/docker-images.before.txt
```

只有在明确要发布 `new-api` 镜像时，才额外备份当前镜像 ID 或导出镜像 tar。

### 4. Kafka 状态记录

不修改 Kafka 数据，只记录 topic 与消费组 offset：

```bash
docker ps --format "table {{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}" | grep -i kafka
docker exec <kafka_container> kafka-topics.sh --bootstrap-server localhost:9092 --list
docker exec <kafka_container> kafka-consumer-groups.sh --bootstrap-server localhost:9092 --describe --group req-msg-consumer-group
```

如果容器内命令路径不同，以生产 Kafka 容器实际路径为准。

### 5. api_server 状态记录

如果本次不发布 `api_server`，只记录状态：

```bash
cd /home/ubuntu/v1.0.3/api_server
mkdir -p backups/ocr-p2p-${TS}
sha256sum api_server start_api_server.sh > backups/ocr-p2p-${TS}/sha256.before.txt
ps -ef | grep "api_server" | grep -v grep > backups/ocr-p2p-${TS}/process.before.txt
ss -ltnp | grep "18081" > backups/ocr-p2p-${TS}/ports.before.txt
```

## 部署顺序

1. 宣告短维护窗口，提醒 `gpuf-c` 可能重连。
2. 完成本地构建、测试、sha256 记录。
3. 在 `ssh pro` 完成服务、DB、配置、Kafka 状态备份。
4. 检查 DB schema；缺失时运行幂等新增 SQL。
5. 上传新 `gpuf-s` 到 release 目录：

```text
/home/ubuntu/v1.0.4/gpuf-s/releases/ocr-p2p-${TS}/gpuf-s
```

6. 在生产机校验新二进制 hash 和 `--help`。
7. 停止当前 `gpuf-s`，只停止该进程，不动 Postgres、Redis、Kafka、new-api。
8. 原子替换 `gpuf-s` 二进制，配置保持原样。
9. 使用原 `start_gpuf-s_tls.sh` 启动。
10. 验证端口、日志、设备重连、推理、计量。
11. 如需 P2P proxy，只在消费端或测试机部署 `gpuf-p2p-proxy`，不要让它作为算力设备登录。

替换示例：

```bash
cd /home/ubuntu/v1.0.4/gpuf-s
install -m 0755 releases/ocr-p2p-${TS}/gpuf-s ./gpuf-s.new
sha256sum ./gpuf-s.new
old_pid="$(cat gpuf-s.pid 2>/dev/null || true)"
if [ -n "$old_pid" ] && kill -0 "$old_pid" 2>/dev/null; then kill -TERM "$old_pid"; sleep 3; fi
mv ./gpuf-s ./backups/ocr-p2p-${TS}/gpuf-s.replaced-at-deploy
mv ./gpuf-s.new ./gpuf-s
./start_gpuf-s_tls.sh
```

## 上线后验证

### 1. 服务健康

```bash
ps -ef | grep "gpuf-s" | grep -v grep
ss -ltnp | grep -E "17000|17001|8081"
tail -n 200 /home/ubuntu/v1.0.4/gpuf-s/*.log
```

关注日志：

- `New control connection`
- client login/heartbeat 是否恢复。
- 不应持续出现 DB、Kafka、TLS、TURN 配置错误。

### 2. 普通文本请求

验证旧功能：

- 普通文本 `/v1/chat/completions` 成功。
- 响应仍有标准 `id/object/created/model/choices/usage`。
- 新增 `p2p.enabled=false` 不影响调用方。

### 3. OCR 网关直连

使用新版 OCR client 作为目标设备：

- 请求 `/v1/chat/completions`。
- `x-target-client-id` 指向已验证 OCR client。
- 响应 `p2p.enabled=false, transport=gateway`。
- `usage.prompt_tokens/completion_tokens/total_tokens/final_tokens` 正常。
- `inference_token_usage.endpoint = 'ocr.image'` 有新增记录。

### 4. P2P OCR 或 P2P 文本

如部署了 `gpuf-p2p-proxy`：

- proxy 响应 `p2p.enabled=true` 且 transport 为 `udp` 或实际 P2P 类型。
- 目标 `gpuf-c` 日志有 P2P request 与 usage receipt。
- `gpuf-s` 日志出现 P2P usage report 与 receipt 匹配成功。
- DB 只在 report 与 receipt 匹配后入库，避免伪造统计。

### 5. 旧计量链路

验证 `gpuf-s -> Kafka -> new-api`：

- 使用计费 token 和唯一 `request-id` 发起一次安全的小请求。
- Kafka `request-message` 有对应记录。
- `req-msg-consumer-group` lag 为 0。
- `new-api` 的 `request_device_map` 有对应 `agent_request_id` 和 `client_id`。

注意：如果请求绕过 new-api relay，只能验证 `request_device_map` 映射链路，不一定产生 `device_response_quota`。

### 6. 管理后台统计

如果 `api_server` 未发布，只做冒烟：

- `/api/banking/admin/overview`
- `/api/banking/admin/network-map`

确认在线节点、离线节点、全部节点、算力与 topCities 未异常。

## 回滚触发条件

出现以下任一情况，优先回滚 `gpuf-s`：

- `gpuf-s` 无法启动或端口 `17000/17001/8081` 不监听。
- 大量旧 `gpuf-c` 无法重连。
- 普通文本推理失败。
- OCR/P2P 请求导致主服务持续 panic 或阻塞。
- DB 写入错误持续影响普通请求。
- Kafka 旧计量链路异常且无法快速定位。

## 回滚顺序

回滚顺序必须按影响面从小到大执行：

1. 停止可选 `gpuf-p2p-proxy` 或关闭外部 P2P 入口。
2. 回滚 `gpuf-s` 二进制、配置、启动脚本。
3. 如果本次发布了 `api_server`，再回滚 `api_server`。
4. 如果本次发布了 `new-api`，再回滚 `new-api` compose/image。
5. Kafka 通常不做数据回滚；只检查消费组 lag，必要时重启消费者或 new-api worker。
6. 数据库只作为最后兜底；优先保留新增字段和新增表，不做破坏性回滚。

## gpuf-s 回滚步骤

```bash
cd /home/ubuntu/v1.0.4/gpuf-s
old_pid="$(cat gpuf-s.pid 2>/dev/null || true)"
if [ -n "$old_pid" ] && kill -0 "$old_pid" 2>/dev/null; then kill -TERM "$old_pid"; sleep 3; fi
cp -a backups/ocr-p2p-${TS}/gpuf-s.before ./gpuf-s
cp -a backups/ocr-p2p-${TS}/prod.env.before ./prod.env
cp -a backups/ocr-p2p-${TS}/start_gpuf-s_tls.sh.before ./start_gpuf-s_tls.sh
chmod 0755 ./gpuf-s ./start_gpuf-s_tls.sh
./start_gpuf-s_tls.sh
ps -ef | grep "gpuf-s" | grep -v grep
ss -ltnp | grep -E "17000|17001|8081"
```

回滚后验证：

- 旧客户端能重连。
- 普通文本推理成功。
- Kafka 旧计量链路恢复。
- 管理后台在线节点逐步恢复。

## api_server 回滚

只有本次实际发布 `api_server` 时才执行。参考已有文档：

```bash
ssh pro "/home/ubuntu/v1.0.3/api_server/releases/<release>/rollback_api_server.sh"
```

如果没有生成脚本，则手工恢复备份二进制并按原 argv 启动。

## new-api 回滚

只有本次实际发布 `new-api` 时才执行：

```bash
cd /srv/gpunexus_com/deploy/new-api
cp -a backups/ocr-p2p-${TS}/docker-compose.ha.yml.before ./docker-compose.ha.yml
test -f backups/ocr-p2p-${TS}/env.before && cp -a backups/ocr-p2p-${TS}/env.before ./.env
docker compose -f docker-compose.ha.yml up -d
docker ps --format "table {{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}"
```

如镜像也升级过，必须先确认备份中的旧 image ID，再回滚到旧 tag。

## 数据库回滚策略

默认不回滚 DB schema。

原因：

- `prod_schema_add_compute_map_token.sql` 是新增 nullable 字段、新增 `inference_token_usage` 表和索引。
- 应用回滚后，旧服务可以忽略新增字段和新增表。
- 直接执行 rollback SQL 会删除 `inference_token_usage` 与 `gpu_assets.geo_*` 字段，属于破坏性操作。

优先处理方式：

- 如果只是测试数据污染，按明确 `request_id` 或时间窗口删除对应测试记录。
- 如果只是 geo mock 数据异常，按备份快照恢复指定 client 的 geo 字段。
- 如果应用已回滚且业务恢复，DB schema 保留。

最后兜底：

- 只有确认新增 schema 导致生产不可用，并经明确批准，才执行 `scripts/prod_schema_add_compute_map_token_rollback.sql`。
- 完整库恢复 `pg_restore` 只在灾难场景使用，因为会覆盖备份点之后的数据。

DB 破坏性回滚前必须确认：

- 已停止会写入相关表的新服务。
- 已导出当前故障现场 dump。
- 已明确可接受备份点之后的数据损失。
- 已得到人工确认。

## 上线检查清单

- [ ] 本地 `cargo fmt --all --check` 通过。
- [ ] 本地 `cargo test -p gpuf-s` 通过。
- [ ] 产物 sha256 已记录。
- [ ] `ssh pro` 当前 gpuf-s 二进制、conf、启动脚本已备份。
- [ ] PostgreSQL full dump、schema dump、重点表 dump 已完成并记录 sha256。
- [ ] new-api 状态、Kafka offset、api_server 状态已记录。
- [ ] DB schema 已确认或幂等新增脚本已执行。
- [ ] 新 `gpuf-s` 已上传到 release 目录并校验 hash。
- [ ] `gpuf-s` 已启动，端口 `17000/17001/8081` 正常。
- [ ] 旧版 `gpuf-c` 普通上线、心跳、文本推理正常。
- [ ] 新版 `gpuf-c 1.0.4` OCR 请求正常。
- [ ] P2P 请求和 P2P 计量匹配正常。
- [ ] 网关直连 `inference_token_usage` 正常写入。
- [ ] `gpuf-s -> Kafka -> new-api` 旧计量链路正常。
- [ ] 管理后台 overview/network-map 数据正常。
- [ ] 回滚脚本或手工回滚命令已在备份目录中确认可用。

## 上线结论模板

```text
上线时间：
发布 commit：
发布产物 sha256：
生产备份目录：
DB 备份目录：
是否变更 DB schema：
是否发布 gpuf-p2p-proxy：
是否发布 api_server：
是否发布 new-api：
验证结果：
遗留风险：
回滚点：
```

## 执行记录 - 2026-07-10

本次已按计划发布 `gpuf-s` 到 `ssh pro`。

脱敏说明：

- 本文不记录真实 Bearer token、数据库连接串、数据库密码、TURN secret、TLS 私钥内容或生产 `.env/prod.env` 内容。
- 验证 request-id 使用占位符记录；如需排查，以生产备份目录和当时执行终端记录为准。
- sha256、PID、端口、备份路径和服务路径用于回滚审计，不属于凭证。

实际发布内容：

- 发布服务：`/home/ubuntu/v1.0.4/gpuf-s/gpuf-s`
- 发布产物：`target/x86_64-unknown-linux-gnu/release/gpuf-s`
- 构建配置：`gpuf-s/.cargo/config.toml`
- 构建形态：`x86_64-unknown-linux-gnu` static-pie，`ldd` 显示 statically linked
- 新二进制 sha256：`d716f4da9d7be739f80acab26ae3112049f2f2bb93471387e5cc37b2d88a8dee`
- 旧二进制 sha256：`1054175f223979fd2b201687a367ff122997c48e8683c6ceebd88f95e3476228`
- 新 PID：`834480`
- 新日志：`/home/ubuntu/v1.0.4/gpuf-s/gpuf-s-tls-20260710-100036.log`

备份目录：

- `gpuf-s`：`/home/ubuntu/v1.0.4/gpuf-s/backups/ocr-p2p-20260710_095810`
- DB：`/srv/gpunexus_com/deploy/postgres/backups/20260710_095810_gpuf_ocr_p2p`
- `new-api` 状态：`/srv/gpunexus_com/deploy/new-api/backups/ocr-p2p-20260710_095810`
- `api_server` 状态：`/home/ubuntu/v1.0.3/api_server/backups/ocr-p2p-20260710_095810`

DB 处理结果：

- `inference_token_usage` 已存在，未执行新增 SQL。
- `gpu_assets.public_ip` 与 `gpu_assets.geo_*` 字段已存在，未执行新增 SQL。
- 已完成 full dump、schema dump、重点表 dump。

验证结果：

- `gpuf-s` 启动成功，端口 `17000/17001/8081` 正常监听。
- 管理后台 `overview` 返回 `code=0`，上线后观察到在线节点 `13`，全部节点 `1594`。
- 管理后台 `network-map?nodeStatus=all` 返回 `code=0`。
- 普通 chat 请求成功，响应包含标准 `id/object/model/client_id/choices/usage` 与 `p2p.enabled=false, transport=gateway`。
- `inference_token_usage` 新增生产验证记录，`chat.completion` 计量正常。
- metered token 请求成功，Kafka `request-message` 消费组 lag 为 `0`。
- `request_device_map` 已写入验证 request-id：`<redacted-production-request-id>`。
- `new-api` 未发布，仅记录状态。
- `api_server` 未发布，仅记录状态。

注意事项：

- 生产当前在线模型列表只有 `gpuf-android`，没有 OCR 模型在线，因此本次未在 `pro` 上做真实 OCR/P2P OCR 推理验证。
- 日志里仍可见 `InvalidContentType` TLS 握手失败和少量 connection reset；旧日志中也存在同类噪声，当前不判断为本次发布引入。
- 普通 chat 验证时出现过模型精确匹配失败后的 generic device fallback，但请求成功返回；后续可单独清理在线 client 的模型上报/模型名一致性。
