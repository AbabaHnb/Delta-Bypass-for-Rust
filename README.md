# Delta Bypass for Rust

[English](README.en.md) · 简体中文

基于 Rust 开发的高性能 Platoboost 自动化绕过工具。支持自动化图像验证码（CAPTCHA）识别与关卡通行凭证提取，提供高效的命令行工具（CLI）与 HTTP REST API 服务两种运行模式。

---

## 项目简介

Delta Bypass for Rust 用于自动化处理 Platoboost 验证流程。输入目标验证链接或通行凭据后，系统将自动识别图像验证码并按序通过各阶关卡，最终返回生成的访问密钥（Key）。

本项目由 **Hasl_Team** 开发，为 Hasl_Team 原版 Python 自动化项目（[Delta-bypass 原项目仓库](https://github.com/AbabaHnb/Delta-bypass)）的 Rust 高性能重构版本。在保证图像识别点位结果与原版**完全一致（像素级精度对齐）**的前提下，显著提升了端到端处理速度与并发吞吐能力。

## 运行环境与依赖声明

* **无需安装语言运行时**：无需安装 Python、Node.js、.NET 或任何第三方解释器/运行时。
* **零外部动态库依赖**：无需安装 OpenSSL 等密码学库（加密模块采用纯 Rust 原生实现并静态编译进二进制中）。
* **网络与存储要求**：
  * 需要连接外网访问 `captcha.platorelay.com` 与 `auth.platorelay.com`。
  * 程序运行的工作目录需具备**写权限**，系统会自动创建并维护缓存文件 `.key_cache.json`（清理后会自动重建，不影响服务）。

---

## 编译与安装

系统要求：Rust 1.75 或更高版本。

### 通用源码编译

```bash
# 克隆仓库并编译 Release 产物
cargo build --release
```

编译产物路径：`target/release/delta-bypass` (Windows 下为 `delta-bypass.exe`)。编译完成后，仅需提取单一二进制文件即可独立部署。

### GitHub Actions 云端自动编译 (推荐)

项目已配置 GitHub Actions 自动化构建流程（[.github/workflows/build.yml](file:///.github/workflows/build.yml)）。
- **每次 Push 代码 / PR**：云端将自动针对 Windows (x64)、Linux (GNU) 以及 Linux (Musl 纯静态) 编译二进制文件，可在仓库的 **Actions** 标签页下直接下载 Artifacts 产物。
- **发布版本（Tag 触发）**：推送版本标签（如 `git tag v1.0.0 && git push origin v1.0.0`）时，GitHub 将自动创建 Release 并附带各平台编译好的可执行文件。

### Linux C 语言环境要求

若系统缺少标准 C 编译器，请先安装基础构建工具：

```bash
# Debian / Ubuntu
sudo apt install build-essential

# CentOS / RHEL / Rocky Linux
sudo dnf groupinstall "Development Tools"

# Alpine Linux
apk add build-base
```

### 极佳兼容性：Musl 纯静态编译 (Linux)

为彻底解决旧版 Linux 系统因 `glibc` 版本过低导致的 `GLIBC_2.xx not found` 错误，可生成无任何动态库依赖的纯静态二进制文件：

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt install musl-tools          # Debian/Ubuntu（其他发行版对应安装 musl-gcc）
cargo build --release --target x86_64-unknown-linux-musl
```

产物路径：`target/x86_64-unknown-linux-musl/release/delta-bypass`，可在任意 Linux 发行版（如 CentOS 7、Alpine、Arch）上直接运行。

---

## 性能对比

基于 8 核 CPU（Debian 操作系统）环境下的实测基准数据：

| 测试项目 | Python 原版 | Rust 重构版 | 说明与提升 |
|---|---|---|---|
| 单链端到端全流程耗时 | ~6.8 秒 | **~5.5 秒** | 综合处理速度提升约 20% |
| `coherence` 验证码识别 | 72 毫秒 | **28 毫秒** | 算法计算加速 2.57 倍 |
| `driftodd` 验证码识别 | 227 毫秒 | **38 毫秒** | 算法计算加速 5.97 倍 |
| 识别点位准确度 | 基准参照 | **24/24 完全一致** | 最大点位偏差 0.000 像素 |

> **说明**：在端到端 ~5.5 秒的耗时中，约 5.0 秒为上游服务端强制要求的关卡冷却时间（不可压缩网络等待），实际计算与本地处理耗时仅约 0.5 秒。

---

## 命令行工具（CLI）指南

### 常用运行示例

> **警告（针对命令行参数）**：目标 URL 包含 `&` 等特殊字符，请在 Windows CMD/PowerShell 及 Linux Shell 中统一使用**双引号**将 URL 包裹，防止参数被 Shell 截断。

```bash
# 绕过指定 Platoboost 链接
delta-bypass "https://auth.platorelay.com/a?d=<通行凭据>"

# 传入通行凭据字符串或包含凭据的文件路径
delta-bypass "<通行凭据>"
delta-bypass tickets.txt

# 批量生成 3 条测试链接并自动执行绕过
delta-bypass --generate 3

# 仅生成测试链接（不执行自动绕过）
delta-bypass --generate 5 --no-auto

# 启动 HTTP API 服务模式
delta-bypass --serve --port 2233
```

### Windows 命令行终端字符编码说明

旧版本 Windows 命令提示符（`cmd.exe`）默认编码非 UTF-8，可能导致日志显示出现乱码或方块。建议通过以下方式处理：
1. **临时切换 UTF-8 编码**：在 CMD 终端中运行 `chcp 65001` 后再启动程序。
2. **推荐终端环境**：使用 **Windows Terminal** 或 **PowerShell 7+**，其默认支持 UTF-8 编码。

### 命令行参数说明表

| 参数项 | 默认值 | 功能说明 |
|---|---|---|
| `<链接/凭据>` | — | 目标 URL、通行凭据字符串或存有凭据的文件路径 |
| `--generate N` / `-g` | 0 | 批量生成 N 条测试验证链接 |
| `--quiet` / `-q` | 关 | 静默模式，仅输出最终 key，隐藏中间过程日志 |
| `--max-rounds N` | 3 | 最大重试轮数（系统会根据服务端返回关卡数自动调整） |
| `--no-auto` | 关 | 搭配 `--generate` 使用，仅生成链接而不执行绕过 |
| `--serve` | 关 | 以 HTTP API 服务模式运行 |
| `--host` | 0.0.0.0 | 服务监听地址（若仅限本地调用建议设为 127.0.0.1） |
| `--port` / `-p` | 2233 | 服务监听端口 |
| `--prepared N` | 30 | 验证码预备池容量（设置为 0 则关闭预备池功能） |
| `--img <文件> --img-type <题型>` | — | 调试参数：针对本地图片测试识别（题型为 `driftodd` 或 `coherence`） |
| `--bench N` | 1 | 调试参数：针对单张图片重复计算 N 次并输出性能基准 |
| `--pool-stats` | 关 | 调试参数：仅实时监控预备池状态指标 |
| `--pool-watch-secs N` | 60 | 调试参数：预备池监控持续时长（单位：秒） |

---

## HTTP API 接口规范

### 启动服务

```bash
delta-bypass --serve --port 2233 --prepared 30
```

### 请求示例

```bash
curl -G http://127.0.0.1:2233/delta \
     --data-urlencode "url=https://auth.platorelay.com/a?d=<通行凭据>"
```

### 响应示例

```json
{
  "key": "FREE_xxxxxxxx",
  "cached": false,
  "error": null,
  "made_by": "Hasl_Team",
  "qq_group": "277707901",
  "times": "5.512340000000s"
}
```

### 响应字段 Schema

| 字段 | 类型 | 说明 |
|---|---|---|
| `key` | String \| null | 成功提取的通行密钥，获取失败时返回 `null` |
| `cached` | Boolean | 标识是否命中 24 小时内的历史结果缓存 |
| `error` | String \| null | 错误描述，操作成功时为 `null` |
| `times` | String | 实际绕过执行耗时（若命中缓存，则返回首次成功绕过时的记录耗时） |

### 常见 Error 字典

| 错误信息 | 诊断说明与处理建议 |
|---|---|
| `链接格式无效 / Malformed link` | 请求 URL 格式错误或无法解析出有效通行凭据 |
| `链接无效 / Invalid link: <服务端详情>` | 凭据本身已失效或过期（上游直接拒绝，不进行重试） |
| `绕过失败 / Bypass failed` | 连续执行两次绕过均未能提取 Key（已包含一次自动重试） |
| `内部执行异常 / Internal execution error` | 系统内部发生未预期异常 |
| `未获得结果 / No result returned` | 异步等待通道意外中断或超时 |

---

## 性能优化策略

系统通过以下三项核心设计最大化压缩处理延迟：

1. **预备池预解机制（缩短 ~1.1 秒）**  
   实测验证表明，上游验证码生成服务为全网公用机制，与特定验证链接无强绑定关系。系统在后台异步完成“拉取验证码 -> 下载 45KB 图像 -> 算法计算答案”的完整流程并生成凭证池。当用户请求到达时，仅需执行耗时约 180ms 的凭证兑换操作。所有凭证严格遵循“即时兑换、即时使用”原则，**绝不复用与长期缓存**。

2. **请求发送起点时间计算（缩短 ~0.2 秒）**  
   上游对 5 秒关卡冷却时间的校验基于服务端接收请求的时间戳。系统改为从客户端请求**发出时刻**开始计时，而非等待响应接收完毕，省去了一次网络往返延迟（RTT）。同时引入动态自适应余量调整机制（60ms ~ 1500ms），防范网络抖动导致的“请求过快”判定。

3. **HTTP 长连接复用（缩短 ~0.6 秒）**  
   建立预连接池以保持与上游服务器的通信长连接。在已建立的 TCP/TLS 连接上下载 45KB 图像仅需 ~25ms，相较于重新握手（~660ms）大幅降低耗时。

---

## 预备池（Prepared Pool）并发与速率控制

预备池机制本质上受限于**请求速率**而非服务器存储容量。

若维持 30 个有效期为 30 秒的预备题目，数学理论要求后台必须保持平均 **1 次/秒** 的补池速率（每题涉及 2 次 HTTP 请求，即恒定 2 QPS 的后台请求）。

针对上游严苛的限流策略，系统设计了严密的并发与退避控制机制：
* **全局单令牌桶派发**：所有后台补池线程统一排队获取操作许可。
* **并发限制**：限制全局同时处理的验证码题目上限为 2 道。
* **联合退避机制**：一旦触发上游限流或拒绝响应，全局所有补池线程立即休眠（初始 5 秒，按指数退避递增至最高 60 秒，恢复后逐步减半）。

实测预备池水位保持在 30/30，零触发限流。冷启动建立完整预备池约需 30 秒，且填充期间不影响正常在线服务。

> **调优建议**：若需扩大池容量，请优先调大 `src/config.rs` 中的 `POOL_MAX_AGE` 参数（上游服务端有效阈值为 60 秒），切勿单向增加目标池大小，否则速率限制器会将补池速率强制锁定在安全阀值之下。

---

## 识别算法精度对齐原理

Rust 重构版本做到了与原 Python 版本识别点位的像素级完全一致。为确保算法精度一致，以下核心逻辑严格保持原版行为：

1. **灰度转换浮点数除法**  
   必须严格执行 `/ 3.0` 浮点除法，不得替换为 `* (1.0 / 3.0)`。以 510/3 为例，精确值为 `170.0`，而 `510 * (1/3)` 的浮点运算结果为 `169.99998`。由于二值化阈值临界点恰好设定为 170，浮点精度微小偏差会导致像素二值化判定翻转，进而改变连通域形状及最终质心选点。

2. **GIF 逐帧画布合成**  
   GIF 动画为优化体积，后续帧通常仅保存增量变化区域。识别器必须构建全尺寸画布并按 dispose 规则（包含透明度处理与帧渲染规矩）逐帧覆盖合成。若简易读取各帧增量裁剪块，会导致首帧之后的图像严重黑屏破损。

3. **平局选点决策一致性**  
   Rust 标准库 `max_by` 在数值相同时默认选择后出现的元素，而 Python 默认选择首个出现的元素。这一差异会导致边缘点选择漂移。因此本项目自定义实现了 `pick_max` 与 `pick_min` 比较器。

4. **网格尺寸向下取整**  
   图像网格划分均使用整数整除运算，完全对齐 Python NumPy 的 `//` 算术行为。

---

## 生产环境部署与服务化指南

### 1. Windows 服务化部署

#### 方式 A：Windows 任务计划程序（无需额外工具）
1. 打开“任务计划程序”，点击右侧“创建任务”。
2. **常规**页：勾选“不管用户是否登录都要运行”及“使用最高权限运行”。
3. **触发器**页：新建触发器，选择“启动时”。
4. **操作**页：新建操作，“程序或脚本”填写 `delta-bypass.exe` 的绝对路径；“添加参数”填入 `--serve --host 127.0.0.1 --port 2233 --prepared 30`；**“起始于”务必填写 exe 所在的完整目录**（若未填写，工作目录会被重定向至 `C:\Windows\System32` 导致缓存写入失败）。
5. **设置**页：勾选“如果任务失败，按以下频率重新启动”。

#### 方式 B：NSSM 服务管理器（推荐：支持自动重启与日志重定向）
从 [nssm.cc](https://nssm.cc) 下载 NSSM 工具，以管理员身份运行 CMD 执行：

```cmd
nssm install DeltaBypass C:\delta-bypass\delta-bypass.exe
nssm set DeltaBypass AppParameters "--serve --host 127.0.0.1 --port 2233 --prepared 30"
nssm set DeltaBypass AppDirectory C:\delta-bypass
nssm set DeltaBypass AppStdout C:\delta-bypass\out.log
nssm set DeltaBypass AppStderr C:\delta-bypass\err.log
nssm start DeltaBypass
```

### 2. Linux Systemd 服务部署

可以使用项目自带的服务文件 `deploy/delta-bypass.service`：

```bash
# 1. 创建程序部署目录并复制可执行文件
sudo mkdir -p /opt/delta-bypass
sudo cp target/release/delta-bypass /opt/delta-bypass/
sudo chown -R www:www /opt/delta-bypass

# 2. 配置 Systemd 服务
sudo cp deploy/delta-bypass.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now delta-bypass

# 3. 查看服务状态与日志
systemctl status delta-bypass
journalctl -u delta-bypass -f
```

*若系统不存在服务运行用户 `www`，请先创建无登录权限的系统用户*：
```bash
sudo useradd -r -s /usr/sbin/nologin www
```

### 3. Docker 容器化部署

#### 推荐 Multi-Stage Dockerfile

```dockerfile
# 构建阶段
FROM rust:1-alpine AS builder
RUN apk add --no-cache build-base
WORKDIR /build
COPY . .
RUN cargo build --release

# 运行阶段
FROM alpine:latest
RUN apk add --no-cache ca-certificates && \
    adduser -D -H app
WORKDIR /app
COPY --from=builder /build/target/release/delta-bypass /app/
RUN chown -R app:app /app
USER app
EXPOSE 2233
CMD ["/app/delta-bypass", "--serve", "--host", "0.0.0.0", "--port", "2233", "--prepared", "30"]
```

#### 容器构建与运行命令

```bash
docker build -t delta-bypass .

# 启动容器并挂载数据卷保存 key 缓存
docker run -d --name delta-bypass \
  -p 127.0.0.1:2233:2233 \
  -v delta-keys:/app \
  --restart unless-stopped \
  delta-bypass
```

> **注意**：
> 1. 容器中必须安装 `ca-certificates`，否则无法建立 HTTPS 连接。
> 2. 容器内部 `--host` 需设为 `0.0.0.0`，外部安全限制通过 `-p 127.0.0.1:2233:2233` 绑定至本地回路。

### 4. Nginx 反向代理与安全加固

**安全警告**：本项目 HTTP API 接口默认未配置身份验证机制。任何能够连通该端口的主机均可调用绕过能力。**严禁直接将接口无保护暴露于公网环境。**

建议配置 Nginx 进行访问鉴权与限流：

```nginx
limit_req_zone $binary_remote_addr zone=delta:10m rate=10r/s;

location /delta {
    limit_req zone=delta burst=20 nodelay;
    auth_basic "Delta Bypass for Rust API Authorization";
    auth_basic_user_file /etc/nginx/.htpasswd;
    proxy_pass http://127.0.0.1:2233;
    proxy_read_timeout 120s;
}
```

---

## 项目代码结构

```
src/
├── main.rs              命令行入口（CLI 参数解析与任务调度）
├── lib.rs               核心库入口（模块导出声明）
├── config.rs            系统全局配置项与常量定义
├── api.rs               HTTP API 服务、Key 缓存与并发请求合并机制
├── chain.rs             绕过主流程（验证码获取 -> 提交 -> 关卡递进 -> 密钥提取）
├── pool.rs              验证码预备池异步管理机制
├── auth.rs              与认证服务端通信协议实现
├── crypto.rs            上游自定义加密算法实现
├── net.rs               HTTP 长连接与 Connection Pool 管理
├── useragent.rs         User-Agent 伪装与 Mobile 浏览器特征生成
├── link.rs              测试链接生成器
├── timing.rs            高精度耗时统计与性能诊断跟踪器
├── image/               图像处理与计算模块
│   ├── mod.rs           GIF 图像解码、灰度转换与圆拟合算法
│   ├── patches.rs       暗色像素连通域分割（Patching）
│   └── nearest.rs       近邻像素点快速搜索算法
└── solver/              验证码求解器模块
    ├── mod.rs           题型分发与求解器调度接口
    ├── driftodd.rs      反向旋转图像求解器
    ├── coherence.rs     静态区域/一致性图像求解器
    └── tracking.rs      图像形状运动轨迹追踪算法
```

---

## 核心配置参数

全局参数均定义于 `src/config.rs` 中，关键配置项说明如下：

| 配置常量 | 默认值 | 调整说明与限制 |
|---|---|---|
| `MIN_STEP_GAP` | 5 秒 | 上游关卡强制冷却硬性间隔，**严禁下调** |
| `GAP_MARGIN_START` | 250 毫秒 | 动态自适应延迟余量初始值 |
| `POOL_MAX_AGE` | 30 秒 | 预备池题目有效生存时间（必须低于服务端 60 秒上限） |
| `POOL_MIN_SLOT_INTERVAL` | 950 毫秒 | 预备池补池最小许可间隔，**严禁下调** |
| `POOL_MAX_INFLIGHT` | 2 | 预备池最大并发处理数，调大易触发上游限流 |
| `POLL_MAX_ATTEMPTS` | 10 | 密钥轮询最大尝试次数 |
| `MAX_ROUNDS_HARD_CAP` | 12 | 关卡循环硬上限（防止异常链接发生死循环） |

---

## 常见问题与排查指南

| 异常现象 | 根因分析 | 解决方案 |
|---|---|---|
| `链接无效 / Invalid link` | 凭据过期或已被上游拒绝 | 更换新的 Platoboost 测试链接（上游主动拒绝行为） |
| `绕过失败 / Bypass failed` | 流程中某一环节未能成功响应 | 移除 `--quiet` 参数重新运行，检查日志末尾 `终止于:` 确定故障具体步骤 |
| 大量日志提示“触发频率限制” | 关卡冷却间隔过短 | 检查 `MIN_STEP_GAP` 配置；若上游风控升级需提高冷却时间 |
| 预备池无法填满 | 触发上游频率限制 | 运行 `--pool-stats` 查看，若 `被拒` > 0 说明已被上游限流 |
| 高并发下连接频繁失败 | 系统句柄限制（Linux） | 执行 `ulimit -n 65535` 或在 Systemd 服务中配置 `LimitNOFILE=65535` |
| Windows CMD 中文显示乱码 | 终端默认编码非 UTF-8 | 运行 `chcp 65001` 或改用 Windows Terminal / PowerShell 7 |
| Linux 提示 `GLIBC_2.xx not found` | 编译镜像 glibc 版本高过目标系统 | 采用 musl 纯静态编译目标（`x86_64-unknown-linux-musl`） |
| Linux 提示 `Permission denied` | 二进制缺乏可执行权限 | 执行 `chmod +x delta-bypass` 赋予权限 |
| 无法找到 `.key_cache.json` | 启动工作目录非二进制所在路径 | 检查 Task Scheduler 中的“起始于”或 Systemd 中的 `WorkingDirectory` 设置 |
| Docker 容器内部报证书错误 | 镜像缺失 CA 根证书 | 在 Dockerfile 运行时阶段安装 `ca-certificates` |
| 端口号已被占用 | 目标端口被其他进程绑定 | 修改 `--port` 端口号，或使用 `netstat` / `ss` 命令定位并关闭冲突进程 |

---

## 开源协议

本项目基于 [MIT 协议](LICENSE) 开源。
