# gpuf-s API Server 中文文档

本文档是 `api_server.md` 的中文对应说明，重点覆盖前端银行后台
`/api/banking/admin/*` 接口。完整通用接口仍以英文主文档为准。

## 基础信息

- 默认地址：`http://127.0.0.1:18081`
- 受保护部署可使用：`http://<host>:18081`
- 请求和响应内容类型：`application/json`

银行后台接口统一返回：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

字段说明：

| 字段 | 类型 | 说明 |
|---|---|---|
| code | number | `0` 表示成功 |
| message | string | 成功时为 `ok` |
| data | object | 具体接口数据 |

## 推理网关说明

本文档中的管理 API 通常监听 gpuf-s API Server 端口，例如 `18081`。
OpenAI 兼容推理请求使用 gpuf-s 的独立推理网关端口，例如：

```text
http://<gpuf-s-host>:<inference_gateway_port>/v1/chat/completions
```

`POST /v1/chat/completions` 的 `messages[].content` 支持两种格式：

| 格式 | 说明 |
|---|---|
| string | 纯文本对话，保持旧 worker 协议兼容 |
| array | OpenAI 风格多模态内容数组，会走新版 chat task 协议 |

PaddleOCR-VL OCR 多模态请求示例：

```json
{
  "model": "PaddleOCR-VL-1.6-GGUF",
  "messages": [
    {
      "role": "user",
      "content": [
        {"type": "text", "text": "请识别图片中的文字。"},
        {
          "type": "image_url",
          "image_url": {
            "url": "data:image/png;base64,<base64-image>"
          }
        }
      ]
    }
  ],
  "max_tokens": 256,
  "temperature": 0.0,
  "stream": false
}
```

多模态数组说明：

- 支持的 part 类型：`text`、`image_url`、`image`、`media_marker`。
- 图片 URL 支持 `data:`、`http://`、`https://`。
- `file://` 默认禁用；只有服务该请求的 `gpuf-c` 进程设置
  `GPUF_ALLOW_FILE_IMAGE_URLS=1` 时才允许，建议只用于可信本地测试。
- OCR/视觉模型需要 `gpuf-c` 启动时传入匹配的 `--llama-mmproj-path`。
- 多模态数组请求需要更新后的非 Android `gpuf-c` worker；`cuda`、
  `vulkan`、`metal`、`cpu` 构建都包含 multimodal 支持。

完整 OpenAI 兼容推理接口见 `docs/gpuf-openai-compatible-api.md`。

## 数据来源和口径

当前银行后台看板主要读取以下表：

| 表 | 用途 |
|---|---|
| gpu_assets | 节点、状态、地区、用户、更新时间 |
| system_info | 算力、CPU/内存/磁盘使用率 |
| device_info | GPU 型号、GPU 数量、GPU 使用率 |
| inference_token_usage | Token 吞吐和 Token 总量 |

基础可见节点口径：

```sql
COALESCE(gpu_assets.valid_status, 'valid') IN ('valid', 'warning')
```

也就是说，看板接口只统计 API 可见的 `valid/warning` 节点。

## 节点状态分类

接口里有三个库存分类：

| 分类 | 含义 |
|---|---|
| all | 全部 API 可见节点 |
| online | 归一化状态为 `active` 或 `online` 的节点 |
| offline | API 可见节点中非 `active/online` 的节点 |

离线分类包含：

```text
offline
warning
maintenance
error
```

固定同一地区和同一时刻下：

```text
online + offline = all
```

这个等式适用于节点数量和算力。

## GET `/api/banking/admin/overview`

获取后台首页概览卡片、资源使用、集群构成，以及在线/离线/全部三分类数据。

### 查询参数

| 参数 | 类型 | 是否可选 | 说明 |
|---|---:|:---:|---|
| region | string | 是 | 按城市、区域、省份或算力区过滤 |
| from | ISO8601 string | 是 | Token 总量统计开始时间 |
| to | ISO8601 string | 是 | Token 总量统计结束时间，必须晚于 `from` |

注意：

- `overview` 不使用 `nodeStatus` 切换结果集。
- `overview` 一次返回 `all/online/offline` 三套库存维度数据。
- `from/to` 只影响 Token 总量卡片；节点数、算力、资源使用、集群构成仍是当前库存状态。

### 返回字段

| 字段 | 类型 | 说明 |
|---|---|---|
| summaryCards | SummaryCard[] | 前端卡片数组，包含节点数、算力、Token 卡片 |
| resourceUsage | ResourceUsage | 全部节点口径的资源使用 |
| clusterStack | ClusterStackItem[] | 全部节点口径的集群构成 |
| statusBreakdown | OverviewStatusBreakdown | `all/online/offline` 三分类结构化数据 |

### SummaryCard

| 字段 | 类型 | 说明 |
|---|---|---|
| key | string | 卡片唯一 key |
| label | string | 前端展示名称 |
| value | number | 原始值；算力为 TFLOPS |
| displayValue | string | 展示值；算力换算为 PF 展示 |
| unit | string | 单位 |
| caption | string | 可选说明 |

库存相关 `summaryCards` key：

| key | label | value 口径 | 单位 | 说明 |
|---|---|---:|---|---|
| onlineNodes | 在线节点 | 节点数 | 个 | 在线节点数量 |
| offlineNodes | 离线节点 | 节点数 | 个 | 离线/非在线节点数量 |
| allNodes | 全部节点 | 节点数 | 个 | 全部 API 可见节点数量 |
| totalCompute | 总算力 | TFLOPS | PF | 兼容旧前端，等于 `allCompute` |
| onlineCompute | 在线算力 | TFLOPS | PF | 在线节点算力 |
| offlineCompute | 离线算力 | TFLOPS | PF | 离线/非在线节点算力 |
| allCompute | 全部算力 | TFLOPS | PF | 全部 API 可见节点算力 |

Token 相关 `summaryCards` key：

| key | 说明 |
|---|---|
| realtimeTokenThroughput | 最近 10 秒窗口的 Token/s |
| todayTokenTotal | 今日 Token 总量，或 `from/to` 指定时间段 Token 总量 |

Token 数据不按 `online/offline/all` 拆分。原因是 Token 是历史调用时间序列，
而节点在线/离线是当前库存状态；直接混用会造成口径歧义。

### ResourceUsage

| 字段 | 类型 | 说明 |
|---|---|---|
| totalDevices | number | 当前分类下的节点数 |
| usedDevices | number | 当前分类下的使用中节点估算值 |
| usageRate | number | `usedDevices / totalDevices`，四舍五入百分比 |

`usedDevices` 是估算值：在线节点中，存在模型元数据或当前遥测负载大于等于
5% 的节点会被认为在使用中。

### ClusterStackItem

| 字段 | 类型 | 说明 |
|---|---|---|
| key | string | GPU 分组 key |
| label | string | 展示名称 |
| percent | number | 当前分类下的占比 |

当前分组：

| key | label |
|---|---|
| a100_h100 | GPU A100/H100 |
| a800_4090 | GPU A800/4090 |
| cpu_edge | CPU/边缘节点 |

### statusBreakdown

`statusBreakdown` 是前端推荐使用的新结构，避免从 `summaryCards` 中手动拼装三分类。

```json
{
  "statusBreakdown": {
    "all": {},
    "online": {},
    "offline": {}
  }
}
```

每个分类的结构相同：

| 字段 | 类型 | 说明 |
|---|---|---|
| nodes | number | 当前分类节点数 |
| compute | number | 当前分类原始算力，单位 TFLOPS |
| computeDisplayValue | string | 展示用算力值，单位 PF |
| computeUnit | string | 固定为 `PF` |
| resourceUsage | ResourceUsage | 当前分类资源使用 |
| clusterStack | ClusterStackItem[] | 当前分类集群构成 |

### overview 示例

下面是结构示例，数值会随线上节点状态和 Token 调用变化。

```bash
curl "http://<host>:18081/api/banking/admin/overview"
```

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "summaryCards": [
      {
        "key": "onlineNodes",
        "label": "在线节点",
        "value": 3,
        "displayValue": "3",
        "unit": "个"
      },
      {
        "key": "totalCompute",
        "label": "总算力",
        "value": 100,
        "displayValue": "0.1",
        "unit": "PF"
      },
      {
        "key": "offlineNodes",
        "label": "离线节点",
        "value": 7,
        "displayValue": "7",
        "unit": "个"
      },
      {
        "key": "allNodes",
        "label": "全部节点",
        "value": 10,
        "displayValue": "10",
        "unit": "个"
      },
      {
        "key": "onlineCompute",
        "label": "在线算力",
        "value": 40,
        "displayValue": "0",
        "unit": "PF"
      },
      {
        "key": "offlineCompute",
        "label": "离线算力",
        "value": 60,
        "displayValue": "0.1",
        "unit": "PF"
      },
      {
        "key": "allCompute",
        "label": "全部算力",
        "value": 100,
        "displayValue": "0.1",
        "unit": "PF"
      },
      {
        "key": "realtimeTokenThroughput",
        "label": "实时Token吞吐",
        "value": 3.1,
        "displayValue": "3.1",
        "unit": "/s",
        "caption": "当前TPS"
      },
      {
        "key": "todayTokenTotal",
        "label": "今日Token总量",
        "value": 62,
        "displayValue": "62",
        "unit": "tokens",
        "caption": "今日已调用"
      }
    ],
    "resourceUsage": {
      "totalDevices": 10,
      "usedDevices": 2,
      "usageRate": 20
    },
    "clusterStack": [
      {
        "key": "a100_h100",
        "label": "GPU A100/H100",
        "percent": 30
      },
      {
        "key": "a800_4090",
        "label": "GPU A800/4090",
        "percent": 20
      },
      {
        "key": "cpu_edge",
        "label": "CPU/边缘节点",
        "percent": 50
      }
    ],
    "statusBreakdown": {
      "all": {
        "nodes": 10,
        "compute": 100,
        "computeDisplayValue": "0.1",
        "computeUnit": "PF",
        "resourceUsage": {
          "totalDevices": 10,
          "usedDevices": 2,
          "usageRate": 20
        },
        "clusterStack": [
          {
            "key": "a100_h100",
            "label": "GPU A100/H100",
            "percent": 30
          },
          {
            "key": "a800_4090",
            "label": "GPU A800/4090",
            "percent": 20
          },
          {
            "key": "cpu_edge",
            "label": "CPU/边缘节点",
            "percent": 50
          }
        ]
      },
      "online": {
        "nodes": 3,
        "compute": 40,
        "computeDisplayValue": "0",
        "computeUnit": "PF",
        "resourceUsage": {
          "totalDevices": 3,
          "usedDevices": 2,
          "usageRate": 67
        },
        "clusterStack": [
          {
            "key": "a100_h100",
            "label": "GPU A100/H100",
            "percent": 67
          },
          {
            "key": "a800_4090",
            "label": "GPU A800/4090",
            "percent": 33
          },
          {
            "key": "cpu_edge",
            "label": "CPU/边缘节点",
            "percent": 0
          }
        ]
      },
      "offline": {
        "nodes": 7,
        "compute": 60,
        "computeDisplayValue": "0.1",
        "computeUnit": "PF",
        "resourceUsage": {
          "totalDevices": 7,
          "usedDevices": 0,
          "usageRate": 0
        },
        "clusterStack": [
          {
            "key": "a100_h100",
            "label": "GPU A100/H100",
            "percent": 14
          },
          {
            "key": "a800_4090",
            "label": "GPU A800/4090",
            "percent": 14
          },
          {
            "key": "cpu_edge",
            "label": "CPU/边缘节点",
            "percent": 72
          }
        ]
      }
    }
  }
}
```

## GET `/api/banking/admin/network-map`

获取地图城市节点、区域、重点省份和 Top 城市。

### 查询参数

| 参数 | 类型 | 是否可选 | 默认值 | 说明 |
|---|---:|:---:|---|---|
| nodeStatus | string | 是 | all | `all`、`online`、`offline` |
| status | string | 是 | all | `nodeStatus` 缺省时的兼容别名 |

支持中文值：

| 中文值 | 等价值 |
|---|---|
| 全部 | all |
| 在线 | online |
| 离线 | offline |

错误处理：

- 非法 `nodeStatus/status` 会返回 HTTP `400 Bad Request`。
- 当前实现的 400 响应体为空，不会返回 `{ code, message, data }` envelope。

默认行为：

```bash
/api/banking/admin/network-map
```

等价于：

```bash
/api/banking/admin/network-map?nodeStatus=all
```

也就是说，不传 `nodeStatus` 时展示全部节点，包含在线和离线/非在线节点。

### nodeStatus 对返回值的影响

`nodeStatus` 会先过滤节点集合，再计算以下字段：

| 字段 | 是否受 nodeStatus 影响 | 说明 |
|---|---|---|
| cities | 是 | 只聚合过滤后的节点 |
| regions | 是 | 只包含过滤后仍有城市数据的区域 |
| highlightProvinces | 是 | 基于过滤后的省份算力/节点数计算 |
| topCities | 是 | 基于过滤后的城市集合重新排序取 Top 5 |
| links | 否 | v1 固定返回空数组 |

`topCities` 排序规则：

1. `tflops` 降序
2. `nodes` 降序
3. `cityId` 升序

因此：

```bash
/api/banking/admin/network-map?nodeStatus=offline
```

返回的 `topCities` 是离线/非在线节点集合里的 Top 5，不会混入在线节点。

### NetworkCity

| 字段 | 类型 | 说明 |
|---|---|---|
| id | string | 城市稳定 id |
| name | string | 城市名称 |
| province | string | 省份名称 |
| coord | [number, number] | `[lng, lat]` |
| nodes | number | 当前过滤集合中的城市节点数 |
| tflops | number | 当前过滤集合中的城市算力，单位 TFLOPS |
| gpuModel | string | Top GPU 型号，多个用 `/` 拼接 |
| tier | string | `mega/large/medium/small` |
| onlineNodes | number | 当前城市聚合结果里的在线节点数 |
| usedNodes | number | 当前城市聚合结果里的使用中节点估算值 |

注意：

- `cities[].onlineNodes` 字段保留字面含义。
- 当 `nodeStatus=offline` 时，`cities[].onlineNodes` 通常为 `0`。
- `cities[].nodes` 才是当前过滤结果集的节点数。

### network-map 示例

```bash
# 全部节点，默认行为
curl "http://<host>:18081/api/banking/admin/network-map"

# 在线节点地图和 Top 5
curl "http://<host>:18081/api/banking/admin/network-map?nodeStatus=online"

# 离线/非在线节点地图和 Top 5
curl "http://<host>:18081/api/banking/admin/network-map?nodeStatus=offline"
```

## GET `/api/banking/admin/compute-nodes`

获取算力节点分页列表，用于节点表格。

### 查询参数

| 参数 | 类型 | 是否可选 | 默认值 | 说明 |
|---|---:|:---:|---|---|
| status | string | 是 | 无 | `active/online/warning/maintenance/offline/error` |
| device | string | 是 | 无 | `linux/windows/mac`；无法识别的系统归为 `unknown` |
| region | string | 是 | 无 | 匹配城市 id、城市名、地区或节点池文本 |
| keyword | string | 是 | 无 | 在 id/name/owner/region/GPU 中模糊搜索 |
| page | number | 是 | 1 | 页码 |
| pageSize | number | 是 | 20 | 每页数量，最大 200 |

### 返回字段

| 字段 | 类型 | 说明 |
|---|---|---|
| items | ComputeNodeItem[] | 节点列表 |
| pagination | Pagination | 分页信息 |
| stats | ComputeNodeStats | 过滤前后统计 |

### ComputeNodeItem

| 字段 | 类型 | 说明 |
|---|---|---|
| id | string | `client_id` 的 hex 字符串 |
| name | string | `client_name` 或后端生成的备用名称 |
| owner | string | `user_id` 或后端生成的节点池名称 |
| region | string | 城市名或原始 geo 区域 |
| regionId | string/null | 城市 id |
| device | string | 归一化系统类型 |
| status | string | 归一化节点状态 |
| gpu | string | 例如 `2 x H100` |
| gpuModel | string/null | Top GPU 型号 |
| gpuCount | number/null | GPU 设备数量 |
| load | number | CPU/内存/磁盘/GPU 使用率中的最大值，0-100 |
| tokensPerSecond | number | 最近 10 秒该节点 Token/s |
| lastSeenAt | RFC3339 string | 最近资产/系统/设备更新时间 |
| lastSeenText | string/null | 后端格式化的相对时间 |

### compute-nodes 示例

```bash
curl "http://<host>:18081/api/banking/admin/compute-nodes?page=1&pageSize=20&device=linux"
```

## GET `/api/banking/admin/token-throughput`

获取 Token 吞吐折线图数据。

### 查询参数

| 参数 | 类型 | 是否可选 | 默认值 | 说明 |
|---|---:|:---:|---|---|
| windowSeconds | number | 是 | 180 | 聚合窗口，最大 86400 |
| intervalSeconds | number | 是 | 3 | 桶间隔，最大 300；后端可能自动调大以控制点数 |
| region | string | 是 | 无 | 按服务节点 `geo_city` 或 `geo_region` 过滤 |

### 返回字段

| 字段 | 类型 | 说明 |
|---|---|---|
| windowSeconds | number | 实际使用的窗口秒数 |
| intervalSeconds | number | 实际使用的桶间隔秒数 |
| latest | TokenThroughputPoint | 最新桶 |
| peaks | object | 当前窗口 input/output 峰值 TPS |
| totals | object | 当前窗口原始 token 汇总 |
| points | TokenThroughputPoint[] | 连续时间桶；空桶补 0 |

### TokenThroughputPoint

| 字段 | 类型 | 说明 |
|---|---|---|
| timestamp | RFC3339 string | 桶时间 |
| input | number | input tokens/s |
| output | number | output tokens/s |
| inputTokens | number | 桶内 input token 原始数量 |
| outputTokens | number | 桶内 output token 原始数量 |
| totalTokens | number | 桶内 token 原始总量 |

### token-throughput 示例

```bash
curl "http://<host>:18081/api/banking/admin/token-throughput?windowSeconds=180&intervalSeconds=30"
```

## 前端接入建议

- 首页卡片继续可以按 `summaryCards[].key` 读取。
- 新页面如果要做在线/离线/全部切换，优先使用 `statusBreakdown`。
- 地图页如果要切换在线/离线/全部，调用 `network-map?nodeStatus=<value>`。
- 地图页不传 `nodeStatus` 时就是全部节点。
- Token 卡片不要和当前在线/离线状态做强行对账。
