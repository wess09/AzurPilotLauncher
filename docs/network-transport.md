# 启动器 Rust 网络传输

`src/http_client.rs` 统一处理启动器 Rust 代码发起的互联网请求。全部请求（包括尚未解析 JSON 时获取 `stable.json`）使用以下顺序：

1. 优先 HTTP/3/QUIC，等待响应头最多 2 秒。
2. QUIC 不可用时使用 HTTPS/TCP，将 TLS ClientHello 拆为多个合法 TLS 记录。
3. 分片握手遇到连接错误或不兼容的 TLS 告警时，在总超时内使用普通 TLS 兼容回退。

不强制 HTTP/3，也不因 UDP 不通而禁止 TCP。旧清单域名已停服；更新检查默认只使用编译时内置的国内优化入口与官方回退入口，组件部署只使用官方入口，不再访问旧域名或依赖旧域名回退。

## TLS 分片针对的干扰

部分 DPI 设备根据明文 ClientHello 中的 SNI 匹配域名，再注入 TCP RST。将一个 ClientHello 放进多个 TLS 记录，会改变识别设备看到的记录布局，而支持标准握手重组的服务器仍能正常建立连接。此实现通过 rustls 的 max_fragment_size 将 ClientHello 的单条明文 TLS 记录限制为 64 字节（包括 5 字节 TLS 记录头）。这部分不修改 SNI、握手消息内容、证书验证或 mTLS 身份，不依赖报文级拦截。

这是一种针对识别过程的规避方法。它无法保证绕过已支持 TLS 记录重组的 DPI，也不能消除无条件 TCP 重置、IP 封锁或直接丢包。各网络设备的效果需要实测。分片限制也适用于后续出站 TLS 记录；启动器仅发送 GET/HEAD，主要额外开销在握手和请求头，下载数据的入站记录大小不受影响。

TLS 不兼容记忆和 QUIC 失败记忆均按 origin 保存 5 分钟，并限制条目数量。普通 TLS 回退复用相同的根证书、SNI、mTLS 身份和 ALPN。证书验证错误不会触发兼容回退，更不会关闭验证。已使用普通 TLS 的站点在记忆到期后会重新尝试分片。

## 下载、取消与重定向

每个重定向目标独立选择协议，保留 Range 等必要请求头；跨 origin 删除 Authorization、Cookie 和 Proxy-Authorization。禁止 HTTPS 降级为 HTTP。TLS 分片与普通 TLS 均支持 HTTP/2 和 HTTP/1.1。

TCP 重置、异常 EOF 和断流会清除 QUIC 失败记忆，重试时重新探测 QUIC，并轮换系统 DNS 返回的地址起点。地址轮换和随机退避是断连恢复措施；它们不阻止伪造 RST，也不提供 DNS 防污染能力。

只有 GET/HEAD 可以进入传输层，最多额外重试两次。清单响应体中断时完整重取，随后才解析 JSON；更新清单上限 1 MiB，组件清单上限 2 MiB。一次请求的重定向、协议回退及清单重读共享总超时。HTTP 429/5xx 留给既有下载层处理，不干扰 Retry-After、分片续传和镜像顺序。大型下载保持流式读取，继续进行 SHA-256、ZIP 校验及事务提交。

取消会丢弃异步请求 future；QUIC 探测期间取消不会继续建立 TCP 连接。响应体沿用有界缓冲及取消检查，网络线程不写文件。四种语言共用网络错误键。

Git、uv、npm、Python 后端、WebView 的网络栈以及本机 HTTP/WS 不受本模块控制。测试/开发模式允许本机 loopback HTTP fixture，发行版互联网请求使用 HTTPS。QUIC 使用直连 UDP；TCP 保留各调用方原有的系统代理/直连选择。

构建通过 .cargo/config.toml 的 reqwest_unstable 开关启用 reqwest 0.12 HTTP/3 API，不要求 nightly Rust。Windows、macOS、Linux 共用此实现。

## 验证

本地服务直接读取 socket 上的 TLS 记录：普通 ClientHello 命中模拟识别规则后收到真实 TCP RST；分片 ClientHello 可以完成标准 TLS、mTLS 并获取 JSON。另测不支持分片的服务端发送 TLS fatal 告警后，普通 TLS 回退及后续请求仍成功。同时验证 QUIC、UDP 静默丢弃、证书拒绝、重定向、取消和清单响应体中断。

官方入口连通性测试默认忽略，可手动执行；UDP 不可达时应通过 TCP 成功：

```sh
cargo test --locked official_manifest_prefers_quic_and_allows_tcp -- --ignored --nocapture
```

参考：[TLS 记录分片研究](https://upb-syssec.github.io/blog/2023/record-fragmentation/)、[zapret 的 TLSREC 实现与兼容性说明](https://github.com/bol-van/zapret/blob/master/docs/readme.en.md)、[QUIC（RFC 9000）](https://www.rfc-editor.org/rfc/rfc9000.html)。
