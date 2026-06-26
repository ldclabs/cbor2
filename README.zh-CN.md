# cbor2

适用于 Rust 的全功能 [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949) CBOR
实现：异步逐项 I/O、serde 往返、规范化/确定性编码、`Value`/`RawValue`、
CBOR simple values、COSE 风格整数 map 键、语义标签、诊断记法、`no_std`，
以及单独可用的良构性检查。

[![CI](https://github.com/ldclabs/cbor2/actions/workflows/ci.yml/badge.svg)](https://github.com/ldclabs/cbor2/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cbor2.svg)](https://crates.io/crates/cbor2)
[![docs.rs](https://docs.rs/cbor2/badge.svg)](https://docs.rs/cbor2)

[English](README.md) | 简体中文

`cbor2` 面向需要完整 CBOR 工具集（而非仅仅基础序列化器）的应用。它能直接
处理普通的 `serde::Serialize`/`Deserialize` 类型，在传输格式（wire shape）
重要时保留协议细节，并且从 `std` 服务一直适配到受限的 `no_std` 目标。

## 为什么选择 cbor2

| 需求           | 内置能力                                                                                                        |
| -------------- | --------------------------------------------------------------------------------------------------------------- |
| Serde 编解码   | `to_vec`、`to_writer`、可借用的 `from_slice`、`from_reader`，并直接支持派生的 serde 类型。                      |
| 稳定的协议字节 | RFC 8949 首选序列化（preferred serialization），外加确定性/规范化编码器和可选的 map 键排序。                    |
| 协议级 CBOR    | Simple values、语义标签、大整数（bignum）、整数 map 键、字段顺序数组，以及通过 `#[derive(cbor2::Cbor)]` 实现的 COSE 风格标签。 |
| 动态或未知数据 | `Value`、`cbor!` 宏，以及用于已校验透传字节的 `RawValue`。                                                      |
| 安全的输入处理 | 恰好一项的良构性检查、CBOR 序列迭代、递归深度限制以及受保护的分配大小。                                        |
| 异步边界       | `async_io` 读取或写入一个完整的 CBOR 项，而不假装 serde 本身是异步的。                                          |
| 调试与检查     | RFC 8949 诊断记法、美化诊断输出，以及配套的 `cbor` 命令行工具。                                                 |
| 嵌入式目标     | `no_std + alloc` 提供完整的基于堆的 API；无 alloc 时仍支持序列化、良构性检查和核心头部编解码器。                 |

以 MIT 许可发布。

## 与其他 CBOR crate 的对比

[`cbor2-bench`](cbor2-bench/README.md) 工作区从功能与速度两方面，把 cbor2 与
`ciborium 0.2`、`serde_cbor 0.11`、`cbor4ii 1.2`、`minicbor 2.2` 做了对比。它
是一个*独立*工作区，因此这些 crate 不会进入本库的依赖图、CI 或 MSRV。

### 功能对比

| 能力                                 | cbor2 | ciborium | serde_cbor | cbor4ii | minicbor |
| ------------------------------------ | :---: | :------: | :--------: | :-----: | :------: |
| serde 原生 `Serialize`/`Deserialize` |   ✅   |    ✅     |     ✅      |    ✅    |    ❌¹    |
| `no_std` + `alloc`                   |   ✅   |    ✅     |     ✅      |    ✅    |    ✅     |
| 零分配编码（定长缓冲）               |   ✅   |    ✅     |     ✅      |   ✅⁵    |    ✅     |
| 无 `alloc` 的类型化解码              |  ❌²   |    ❌     |     ❌      |   ❌²    |    ✅     |
| 从输入借用 `&str`/`&[u8]`            |   ✅   |    ❌     |     ✅      |    ✅    |    ✅     |
| 确定性 / 规范化编码³                 |   ✅   |    ❌     |     ❌      |    ❌    |    ❌     |
| 动态 `Value` 类型                    |   ✅   |    ✅     |     ✅      |    ✅    |    ❌     |
| 原始透传值（`RawValue`）             |   ✅   |    ❌     |     ❌      |   ✅⁶    |    ❌     |
| 语义标签（tags）                     |   ✅   |    ✅     |     ✅      |    ✅    |    ✅     |
| 结构体整数 map 键（COSE）            |   ✅   |    ❌     |     ❌      |    ❌    |    ✅     |
| 诊断记法（RFC 8949 §8）              |   ✅   |    ❌     |     ❌      |    ❌    |    ✅     |
| 异步逐项 I/O（futures / tokio）      |   ✅   |    ❌     |     ❌      |    ❌    |    ❌     |
| 不解码即可校验 / 算尺寸              |   ✅   |    ❌     |     ❌      |    ❌    |    ◑⁴    |

¹ minicbor 用自带的 `#[derive(Encode, Decode)]`；serde 支持在独立的 `minicbor-serde` crate 里。

² 没有任何基于 serde 的 CBOR crate 能在无堆下反序列化 —— 但 cbor2 的低层 [`core::Decoder`](https://docs.rs/cbor2/latest/cbor2/core/struct.Decoder.html)
（以及 cbor4ii 的低层 `Decode`）仍可零分配手动解码。

³ 即对 map 键排序，RFC 8949 §4.2.1；多数 crate 都会用最短形式编码数字（cbor4ii 的浮点固定为 64 位），但只有 cbor2 提供完整的规范化编码器。

⁴ minicbor 的 `Decoder::skip` 能校验结构，但没有精确尺寸计算原语。

⁵ cbor4ii 没有公开的 `no_std` 切片序列化器；它通过 `to_writer` 写入 `&mut [u8]` 来填充定长缓冲，这需要 `std`。

⁶ cbor4ii 的 `RawValue` 是核心层的借用类型，并未与 serde 集成。

`serde_cbor` 已停止维护；其余均在维护中。

### 性能基准

下表为 Apple M1 Pro 上每次操作的中位耗时，`no_std + alloc` 路径
（`to_vec` / `from_slice`），越小越好。完整的 `std` 与 `no_std + no_alloc`
表格、负载定义与方法学见 [`cbor2-bench`](cbor2-bench/README.md#results)。

| 操作 / 负载        | cbor2   | ciborium | serde_cbor | cbor4ii | minicbor |
| ------------------ | ------- | -------- | ---------- | ------- | -------- |
| `encode/int_array` | 2.79 µs | 6.59 µs  | 1.67 µs    | 2.92 µs | 3.29 µs  |
| `encode/log_batch` | 13.3 µs | 16.1 µs  | 9.54 µs    | 6.09 µs | 4.56 µs  |
| `encode/blob`      | 102 ns  | 131 ns   | 133 ns     | 127 ns  | 130 ns   |
| `decode/int_array` | 5.34 µs | 11.0 µs  | 3.24 µs    | 3.43 µs | 5.23 µs  |
| `decode/log_batch` | 38.5 µs | 66.3 µs  | 34.0 µs    | 36.8 µs | 21.8 µs  |
| `decode/blob`      | 97.5 ns | 224 ns   | 88.5 ns    | 90.1 ns | 91.1 ns  |

`int_array`（1024 × `u64`）与 `blob`（4 KiB 字节串）在五个 crate 间字节完全
一致，是严格 apples-to-apples；`log_batch`（128 条结构化记录）用各 crate 的
惯用编码（minicbor 的整数键数组小约 37%，cbor4ii 则把浮点固定为 64 位）。
cbor2 全场都有竞争力，并在 **`no_std + no_alloc`** 下独具优势 —— 在 serde
阵营里定长缓冲编码最快，且是唯一提供 `serialized_size`/`validate` 原语的。在
`std`/`alloc` 的结构化吞吐上 **cbor4ii 最为亮眼**（而 minicbor 的借用式解码
在结构化解码上领先）；cbor2 在编码上与它们按场景互有胜负，详见完整表格。在
`no_std + no_alloc` 下，cbor2 还提供零分配的*编码*（[`to_slice`]）、*校验*
（[`validate`]）和精确*尺寸计算*（[`serialized_size`]）。

```bash
cd cbor2-bench && cargo bench
```

[`to_slice`]: https://docs.rs/cbor2/latest/cbor2/fn.to_slice.html
[`validate`]: https://docs.rs/cbor2/latest/cbor2/fn.validate.html
[`serialized_size`]: https://docs.rs/cbor2/latest/cbor2/fn.serialized_size.html

## 快速开始

```toml
[dependencies]
cbor2 = "1"
```

要使用 `cbor` 命令行工具，请安装 `cbor2-cli`：

```bash
brew install ldclabs/tap/cbor2-cli   # Homebrew，安装 `cbor`
cargo install cbor2-cli              # Cargo，安装 `cbor`
```

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Photo {
    title: String,
    pixels: (u32, u32),
    tags: Vec<String>,
}

let photo = Photo {
    title: "Sunrise".into(),
    pixels: (1920, 1080),
    tags: vec!["morning".into(), "gradient".into()],
};

let bytes = cbor2::to_vec(&photo).unwrap();
let back: Photo = cbor2::from_slice(&bytes).unwrap();
assert_eq!(photo, back);
```

`to_writer` 和 `from_reader` 可与任意 `std::io::Write`/`Read` 配合使用，
`Deserializer::into_iter` 可解码一串连续拼接的项。`from_slice`/`from_reader`
读取开头的一个 CBOR 项；当缓冲区必须恰好包含一项时请使用 `validate`。

## 面向 AI agents

代码代理应先阅读 [`AGENTS.md`](AGENTS.md) 中压缩后的 API 选择规则，再参考
[`docs/agent-cookbook.md`](docs/agent-cookbook.md) 中可复制的 recipes 和迁移
陷阱。可运行示例 [`agent_patterns`](examples/agent_patterns.rs) 覆盖恰好一项
良构性检查、字节串、借用反序列化、原始值、CBOR 序列和规范化编码。

## 特性亮点

* **完整的 serde 集成** —— `#[derive(Serialize, Deserialize)]` 类型可直接
  编码和解码。
* **可借用的 `from_slice`** —— 确定长度的文本串和字节串可直接从输入缓冲区
  反序列化为 `&str` 和借用的 `serde_bytes` 值；分段的不定长字符串则回退为
  自有缓冲区。
* **RFC 8949 首选序列化** —— 整数和浮点数始终以其最小且无损的形式编码，
  包括半精度浮点数。
* **动态 `Value` 类型** —— CBOR 中与 `serde_json::Value` 对应的类型，配有
  `cbor!` 宏，可用类 JSON 语法构建值。
* **CBOR simple values** —— `Simple` 与 `Value::Simple` 可保留 serde
  内置 bool/null 形态之外的已注册或未分配 simple value，包括 SD-CWT 使用的
  `simple(59)` map 键。
* **标签支持** —— 通过 `tag` 模块中的包装类型捕获并输出语义标签
  （RFC 8949 §3.4）；`u128`/`i128` 会自动映射为 bignum 标签。
* **确定性编码** —— `to_canonical_vec`/`to_canonical_writer` 和
  `Value::canonicalize` 实现了核心的确定性编码要求（RFC 8949 §4.2.1）：
  按字节字典序排列 map 键、确定长度、首选序列化、规范化的 bignum 和 NaN。
  对于构建在更早的 RFC 7049 §3.9“Canonical CBOR”规则之上的协议（该规则保留为
  RFC 8949 §4.2.3，并被 ciborium 的 canonical 模块采用），`*_with` 变体可接受
  `KeyOrder::LengthFirst`。
* **COSE 风格整数 map 键、数组与标签** —— 启用 `derive` 特性后，
  `#[derive(cbor2::Cbor)]` 将结构体字段映射为整数键（`#[cbor(key = 1)]`），
  将具名结构体编码为字段顺序数组（`#[cbor(array)]`），并按 RFC 9052 的要求
  将容器包裹进 CBOR 标签（`#[cbor(tag = 18)]`），且与文本键之间没有歧义。
  标签会在编码时写出，并在解码时透明处理，因此同一个类型可同时接受带标签或不带
  标签的输入。字段名和类型名保持不变，因此同样的类型仍可序列化为普通 JSON ——
  `serde_json::to_string(&v)` 直接可用，使用原始字段名且没有标签。所声明的键、
  数组形态和标签在运行时仍可通过 `cbor2::Cbor` trait 检视。
* **原始值（Raw values）** —— `RawValue` 将一项保留为已校验、未解码的字节：
  序列化时将其原样拼接进流中，反序列化时按字节逐一捕获，适用于签名负载、
  透传项和延迟解码。`TryFrom` 可在 `RawValue` 与 `Value` 之间双向转换。
* **健壮的解码** —— 不定长项、分段字符串、重复 map 键、未知标签和 CBOR
  序列（RFC 8742）均可处理；递归有深度限制，伪造的长度无法触发巨量分配。
* **Concise Diagnostic Notation** —— `to_cdn` 将原始 CBOR 渲染为 IETF
  Concise Diagnostic Notation 草案（CDN，`draft-ietf-cbor-edn-literals`）
  正式化的人类可读文本；普通项与 RFC 8949 附录 A 示例一致，同时保留不定长
  标记。API 名称按方向区分：`to_cdn*` 将 CBOR 字节渲染为 CDN 文本，
  `cdn_to_vec`、`cdn_sequence_to_vec` 和 `from_cdn` 将 CDN 文本解析为 CBOR 字节
  或 serde 值；较早的 `diagnostic*` 名称仍作为兼容别名保留。CDN 输入支持注释、
  base 编码字节串、嵌入 CBOR 序列、encoding indicators、标签、simple values，
  以及强制的 `dt`、`ip`、`b1`、`t1` 扩展。`Value` 以相同记法实现 `Display`，
  并以缩进形式实现 `Debug`。对于 CWT claims 这类使用整数键的协议 map，
  `to_cdn_pretty_with_key_comments` 可在传输层整数键旁加入 CDN `// "iss"` 注释。
* **免分配辅助函数** —— `validate` 是针对恰好一项 CBOR 的良构性检查
  （RFC 8949 §5.3.1，包括文本的 UTF-8 校验），`serialized_size` 计算任意
  可序列化值的精确编码大小，`to_slice` 将编码写入调用方提供的缓冲区；这些
  均不分配堆内存。
* **异步逐项 I/O** —— `async_io` 模块在异步字节流上为完整的 CBOR 项划定
  边界，一旦项被缓冲，便复用常规的同步 serde API；面向不可信 stream 时可使用
  带大小上限的读取辅助函数。
* **底层头部编解码器** —— `core` 模块暴露 pull/push 式的 `Header` 接口，
  供需要精确控制传输格式的应用使用。
* **`no_std` 支持** —— `default-features = false, features = ["alloc"]` 保留
  完整 API，仅去掉 `std::io` 互操作和 `HashMap` 转换；不启用 `alloc` 时，
  crate 仍可序列化（`to_writer`/`to_slice`/`serialized_size`）、检查良构性，并使用
  `core` 头部编解码器。

## Crate 特性

| 特性      | 默认             | 作用                                                                                                                    |
| --------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `std`     | 是               | 为每个 `std::io::Read`/`Write` 实现 `cbor2::io` traits，加入 `async_io`，并加入 `HashMap` 转换。隐含 `alloc`。          |
| `alloc`   | 是（经由 `std`） | 一切需要堆的功能：`Value`、`to_vec`/`from_slice`/`from_reader`、`RawValue`、`diagnostic`、确定性编码器以及 `cbor!` 宏。 |
| `derive`  | 否               | `#[derive(cbor2::Cbor)]` 宏。                                                                                           |
| `futures` | 否               | 为 `futures_io::AsyncRead`/`AsyncWrite` 添加 `async_io::futures` 辅助函数。隐含 `std`。                                 |
| `tokio`   | 否               | 为 `tokio::io::AsyncRead`/`AsyncWrite` 添加 `async_io::tokio` 辅助函数。隐含 `std`。                                    |

完全不启用任何特性时，crate 是面向受限目标的 `#![no_std]` 核心：使用
`to_writer`/`to_slice`/`serialized_size` 的流式序列化、`validate`、`tag`
包装类型以及 `core` 头部编解码器。通过 serde 反序列化需要 `alloc`。读写器
实现了精简的 `cbor2::io` traits，已为字节切片（启用 `alloc` 时还包括
`Vec<u8>`）提供这些实现：

```toml
[dependencies]
cbor2 = { version = "1", default-features = false } # 或 features = ["alloc"]
```

```rust
// 可在 no_std + 无 alloc 的目标上运行：
let mut buffer = [0u8; 64];
let item = cbor2::to_slice(&("id", 42u8), &mut buffer).unwrap();
assert!(cbor2::validate(&item[..]).is_ok());
```

## 指南

### 字节串与 `serde_bytes`

一个常见的 serde 陷阱：裸 `Vec<u8>` 和 `&[u8]` 会序列化为整数数组，而不是
CBOR 字节串。对二进制负载请使用
[`serde_bytes`](https://docs.rs/serde_bytes/latest/serde_bytes/)。

```rust
let bytes = vec![1u8, 2, 3, 4];

// 裸 Vec<u8>：[1, 2, 3, 4]
assert_eq!(hex::encode(cbor2::to_vec(&bytes).unwrap()), "8401020304");

// serde_bytes：h'01020304'
let bytes = serde_bytes::ByteBuf::from(bytes);
assert_eq!(hex::encode(cbor2::to_vec(&bytes).unwrap()), "4401020304");
```

对派生结构体中的字段，请显式标注字节缓冲区：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Packet {
    #[serde(with = "serde_bytes")]
    payload: Vec<u8>,
}

let packet = Packet { payload: vec![0xde, 0xad, 0xbe, 0xef] };
assert_eq!(
    hex::encode(cbor2::to_vec(&packet).unwrap()),
    "a1677061796c6f616444deadbeef"
);
```

如果你用 `Value` 构建数据，请使用 `Value::Bytes(...)` 或针对字节切片/向量的
`From` 实现；它们已经表示一个 CBOR 字节串。

### 从切片借用反序列化

`from_slice` 是生命周期感知的：确定长度的文本串和字节串主体可以直接从输入
借用。这与 serde_json 的切片路径一致，适用于输入缓冲区生命周期已足够长的
签名负载或 COSE 结构。

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Packet<'a> {
    #[serde(borrow)]
    label: &'a str,
    #[serde(borrow, with = "serde_bytes")]
    payload: &'a [u8],
}

let bytes = hex::decode("a2656c6162656c626869677061796c6f616442dead").unwrap();
let packet: Packet<'_> = cbor2::from_slice(&bytes).unwrap();
assert_eq!(packet.label, "hi");
assert_eq!(packet.payload, &[0xde, 0xad]);
```

不定长字符串仍被接受，但由于其主体跨多个分段，无法被借用。

### 用 `#[derive(Cbor)]` 实现 COSE 风格整数 map 键、数组与标签

启用 `derive` 特性后，`#[derive(cbor2::Cbor)]` 会连同 CBOR 协议细节一起生成
serde 的 `Serialize`/`Deserialize` 实现：标注了 `#[cbor(key = ...)]` 的字段
使用整数 map 键，且容器会在编码时被包裹进 CBOR 标签（`#[cbor(tag = ...)]`）。
标签层在解码时透明处理，于是一个类型即可处理同时以带标签与不带标签两种形式传输
的协议，无需再额外定义一个「裸」结构体和 `From` 实现。具名结构体也可使用
`#[cbor(array)]`，将其编码为紧凑的字段顺序 CBOR 数组，同时为 JSON 和代码保留
Rust 字段名。字段名和类型名保持不变，因此同样的类型仍可序列化为普通 JSON。

```toml
[dependencies]
cbor2 = { version = "1", features = ["derive"] }
```

下例逐字节复现了
[RFC 9052 附录 C.4.1](https://datatracker.ietf.org/doc/html/rfc9052#appendix-C.4)
的 Simple Encrypted Message（52 字节）：

```rust
use cbor2::Cbor;

/// 受保护的头部参数（RFC 9052 §3.1）。它们以字节串形式承载自身的 CBOR 编码。
#[derive(Debug, PartialEq, Cbor)]
struct Protected {
    /// 10 = AES-CCM-16-64-128（RFC 9053 §4.2）
    #[cbor(key = 1)]
    alg: i8,
}

/// 未受保护的头部参数。
#[derive(Debug, PartialEq, Cbor)]
struct Unprotected {
    #[cbor(key = 5)]
    #[serde(with = "serde_bytes")]
    iv: Vec<u8>,
}

/// COSE_Encrypt0（RFC 9052 §5.2）：在
/// `[protected: bstr, unprotected: map, ciphertext: bstr]` 外包裹标签 16。
#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 16)]
struct CoseEncrypt0(
    #[serde(with = "serde_bytes")] Vec<u8>, // protected，已编码
    Unprotected,
    #[serde(with = "serde_bytes")] Vec<u8>, // ciphertext
);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 受保护头部是已编码的 map {1: 10}。
    let protected = cbor2::to_canonical_vec(&Protected { alg: 10 })?;
    assert_eq!(hex::encode(&protected), "a1010a");

    let msg = CoseEncrypt0(
        protected,
        Unprotected {
            iv: hex::decode("89f52f65a1c580933b5261a78c")?,
        },
        hex::decode("5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569")?,
    );

    // RFC 中那条 52 字节的消息，逐字节一致。
    let bytes = cbor2::to_canonical_vec(&msg)?;
    assert_eq!(bytes.len(), 52);
    assert_eq!(
        hex::encode(&bytes),
        "d08343a1010aa1054d89f52f65a1c580933b5261a78c581c\
         5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569"
    );

    println!("{}", cbor2::to_cdn(&bytes[..])?);
    // 16([h'a1010a', {5: h'89f52f65a1c580933b5261a78c'},
    //     h'5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569'])

    // 解码需要标签 16，并还原每一层。
    let back: CoseEncrypt0 = cbor2::from_slice(&bytes)?;
    assert_eq!(back, msg);
    let header: Protected = cbor2::from_slice(&back.0)?;
    assert_eq!(header, Protected { alg: 10 });

    // JSON 仍然自然——原始字段名，没有标签，没有整数键。
    let json = serde_json::to_string(&header)?;
    assert_eq!(json, r#"{"alg":10}"#);
    Ok(())
}
```

可运行的 [`examples/cose.rs`](examples/cose.rs) 将其扩展为
[`cose2`](https://github.com/ldclabs/cose2) 的真实传输类型 —— cose2 是一个
构建于 cbor2 之上的完整 RFC 9052 COSE 与 RFC 8392 CWT 库 —— 采用具名的
`#[cbor(array)]` 结构体、可选的（分离式）密文，以及透明的标签解码，让同一个
类型同时解码带标签与不带标签的消息：
`cargo run --features derive --example cose`。配套的
[`examples/cwt.rs`](examples/cwt.rs) 则是 cose2 的 CWT 声明集（RFC 8392）：
一个带注册整数声明键的加标签 *map*，配合自然的 JSON 名称、
`skip_serializing_if` 声明省略、以 COSE label 为键的 `#[serde(flatten)]`
扩展声明，以及同样的透明标签解码。它还使用
`to_cdn_pretty_with_key_comments(&bytes[..], Claims::KEYS)`，让诊断输出
保持真实的整数键传输形态，同时用代码注释展示对应的字符串键：

```text
61({
  1: "coap://as.example.com", // "iss"
  2: "erikw", // "sub"
  3: "coap://light.example.com", // "aud"
  4: 1444064944, // "exp"
  5: 1443944944, // "nbf"
  6: 1443944944, // "iat"
  7: h'0b71' // "cti"
})
```

可用 `cargo run --features derive --example cwt` 运行。

该派生宏还实现了 `cbor2::Cbor` trait，在运行时暴露所声明的协议细节 ——
`T::KEYS`、`T::TAG` 和 `T::ARRAY` 作为免分配常量，以及作为
`BTreeMap<String, i128>` 的 `value.keys()`：

```rust
use cbor2::Cbor; // 一次导入：派生宏与 trait

assert_eq!(Protected::KEYS, &[("alg", 1)]);
assert_eq!(CoseEncrypt0::TAG, Some(16));
assert!(!CoseEncrypt0::ARRAY);
```

对于传输形态是数组、但 Rust 形态应保留具名字段的 COSE 结构，添加
`#[cbor(array)]`：

```rust
use cbor2::Cbor;

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 18, array)]
struct Sign1 {
    #[serde(with = "serde_bytes")]
    protected: Vec<u8>,
    unprotected: u8,
    #[serde(with = "serde_bytes")]
    payload: Vec<u8>,
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
}

let msg = Sign1 {
    protected: vec![0xa0],
    unprotected: 0,
    payload: vec![],
    signature: vec![0xff],
};

assert_eq!(hex::encode(cbor2::to_vec(&msg).unwrap()), "d28441a0004041ff");
assert!(Sign1::ARRAY);
```

### 动态值

```rust
use cbor2::{cbor, Simple, Value};

let value = cbor!({
    "code": 415,
    "message": null,
    "extra": { "numbers": [8.2341e+4, 0.251425] },
    (Simple::new(59).unwrap()) => [Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef])],
}).unwrap();

let bytes = cbor2::to_vec(&value).unwrap();
let back: Value = cbor2::from_slice(&bytes).unwrap();
assert_eq!(value, back);

let simple: Simple = cbor2::from_slice(&[0xf8, 0x3b]).unwrap();
assert_eq!(simple, Simple::new(59).unwrap());
```

### 原始值

`RawValue` 延迟解码并保留一项的精确传输字节 —— 处理签名负载的合适工具：

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Signed {
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
    payload: cbor2::RawValue,
}

let bytes = cbor2::to_vec(&Signed {
    signature: vec![0xde, 0xad],
    payload: cbor2::RawValue::serialized(&("untouched", 42)).unwrap(),
}).unwrap();

let signed: Signed = cbor2::from_slice(&bytes).unwrap();
// 用 `signed.payload.as_bytes()` 校验 `signed.signature`，然后：
let (text, n): (String, u8) = signed.payload.deserialized().unwrap();
assert_eq!((text.as_str(), n), ("untouched", 42));
```

### 标签

```rust
use cbor2::tag::RequireExact;

// 标签 0：标准日期/时间字符串。
let datetime = RequireExact::<String, 0>("2013-03-21T20:04:00Z".into());
let bytes = cbor2::to_vec(&datetime).unwrap();
assert_eq!(bytes[0], 0xc0);
```

### CBOR 序列

```rust
let mut stream = Vec::new();
cbor2::to_writer(&"first", &mut stream).unwrap();
cbor2::to_writer(&2u64, &mut stream).unwrap();

let items: Vec<cbor2::Value> = cbor2::de::Deserializer::from_reader(&stream[..])
    .into_iter()
    .collect::<Result<_, _>>()
    .unwrap();

assert_eq!(items, vec![cbor2::Value::from("first"), cbor2::Value::from(2)]);
assert!(cbor2::validate(&stream[..]).is_err()); // 序列不是单一项
```

### 异步逐项 I/O

serde 本身是同步的，但异步传输通常需要项边界。`async_io` 模块将一个完整的
CBOR 项读入缓冲区，校验与 `validate` 相同的结构，然后让你对自己拥有的字节
调用 `from_slice`。

```rust
# async fn example<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<(), cbor2::de::Error> {
let item = cbor2::async_io::read_item(reader).await?;
let value: cbor2::Value = cbor2::from_slice(&item)?;
# Ok(())
# }
```

面向不可信 peer 时，如果外层传输没有已经强制消息大小上限，请使用
`read_item_with_limit` 或 `read_value_with_limit`：

```rust
# async fn bounded<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<cbor2::Value, cbor2::de::Error> {
let value: cbor2::Value = cbor2::async_io::read_value_with_limit(reader, 1 << 20).await?;
# Ok(value)
# }
```

使用 `async_io::write_value` 序列化并发送一个值，或在你已持有一个已校验的
单项字节缓冲区时使用 `async_io::write_item`。

启用 `futures` 或 `tokio` 特性后，请使用对应运行时的适配器，而不必自己编写
本地包装：

```rust
# #[cfg(feature = "futures")]
# async fn futures_example<R: futures_io::AsyncRead + Unpin + ?Sized>(reader: &mut R) -> Result<(), cbor2::de::Error> {
let item = cbor2::async_io::futures::read_item(reader).await?;
# let _: cbor2::Value = cbor2::from_slice(&item)?;
# Ok(())
# }
#
# #[cfg(feature = "tokio")]
# async fn tokio_example<R: tokio::io::AsyncRead + Unpin + ?Sized>(reader: &mut R) -> Result<(), cbor2::de::Error> {
let item = cbor2::async_io::tokio::read_item(reader).await?;
# let _: cbor2::Value = cbor2::from_slice(&item)?;
# Ok(())
# }
```

### 更多示例

可运行的示例位于 `examples/`：

```bash
cargo run --example basic
cargo run --example bytes
cargo run --example sequence
cargo run --example core_headers
cargo run --features derive --example cose
cargo run --features derive --example cwt
```

## 设计决策

本实现刻意与 ciborium 的传输行为保持一致，因此两个 crate 可逐字节互操作：

* 数字始终以其最小且无损的形式编码，符合确定性编码（RFC 8949 §4.2.1）的
  要求。Rust 中的整数宽度被视为内存中的细节，而非传输属性。
* 枚举编码为裸字符串（单元变体）或单条目 map `{variant: payload}`
  （其他所有情况）。
* `Value` 的 map 是 `Vec<(Value, Value)>`，保留传输顺序和任意键。
* 解码遵循健壮性原则：即便编码永不产生不定长、分段字符串、半宽浮点数和
  未知标签，解码时仍接受它们。

## 历史

本项目源自 [Andrew Gallant](https://github.com/BurntSushi) 于 2015 年创建的
`cbor` crate，它构建在 serde 之前的 `rustc-serialize` 框架之上，多年无人
维护。0.5 版本是基于 [serde](https://serde.rs) 的彻底重写，由
[LDC Labs](https://github.com/ldclabs) 维护并以 **`cbor2`** 之名发布 ——
crates.io 上的 `cbor` 名称保留给遗留的 0.4 版本 —— 而 1.0 使其稳定。0.4 的
API 无一保留。

此次重写沿用了 [ciborium](https://github.com/enarx/ciborium) 的设计 —— 在此向其作者们致谢。

## 命令行工具

本工作区在 [`cbor2-cli`](cbor2-cli/README.md) 中提供了 `cbor` 命令行工具。
裸 `cbor` 将任意 CBOR ——来自文件、stdin、十六进制字符串或 base64 字符串——
显示为诊断记法（RFC 8949 §8，已正式化为 CDN）；`decode` 转换为美化 JSON（或用 `--diag`
转换为美化诊断记法），`encode` 将 JSON 转换为 CBOR，`encode --diag` 将 CDN
文本转换为 CBOR，`encode --hex` 为 agent 和文档输出可复制的 CBOR 十六进制，
`validate` 校验完整 CBOR 输入：

```bash
brew install ldclabs/tap/cbor2-cli   # Homebrew
cargo install cbor2-cli              # Cargo
```

```bash
$ cbor bf61610161629f0203ffff
{_ "a": 1, "b": [_ 2, 3]}

$ echo '{"name": "example", "ok": true}' | cbor encode | cbor decode
{
  "name": "example",
  "ok": true
}

$ echo '{"name": "example", "ok": true}' | cbor encode --hex
a2646e616d65676578616d706c65626f6bf5

$ cbor validate a2646e616d65676578616d706c65626f6bf5
valid
```

## 测试

`cargo test` 会运行单元测试、单个集成测试二进制文件以及文档测试 —— 包括
RFC 8949 附录 A 的向量，以及针对 I/O 故障和畸形输入的故障注入测试。CI 会
构建并测试每一种特性组合，直至裸机 `no_std` 目标。用 `cargo llvm-cov`
测得的覆盖率为 100% 的函数和约 98% 的代码行；唯一永不执行的代码行是无法
发生的防御性分支，例如被 `RawValue` 有效性不变式排除掉的错误路径。

## 最低支持的 Rust 版本

Rust 1.85。

## 许可

以 MIT 许可发布。
