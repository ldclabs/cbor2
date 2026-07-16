# cbor2

适用于 Rust 的全功能 [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949) CBOR 实现：异步项 I/O、serde 往返编解码、规范/确定性编码、`Value`/`RawValue`、CBOR 简单值、COSE 风格的整数 Map 键、语义标签、诊断表示法、`no_std` 以及单独提供的格式完好性（well-formedness）检查。

[![CI](https://github.com/ldclabs/cbor2/actions/workflows/ci.yml/badge.svg)](https://github.com/ldclabs/cbor2/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cbor2.svg)](https://crates.io/crates/cbor2)
[![docs.rs](https://docs.rs/cbor2/badge.svg)](https://docs.rs/cbor2)

[English](README.md) | 简体中文

`cbor2` 适用于需要完整 CBOR 工具包（而不仅仅是基础序列化器）的应用程序。它支持标准的 `serde::Serialize`/`Deserialize` 类型，在传输格式至关重要时保留协议细节，并且能够从 `std` 服务平滑扩展到受限的 `no_std` 目标。

## 为什么选择 cbor2

| 需求             | 内置支持                                                                                                            |
| ---------------- | ------------------------------------------------------------------------------------------------------------------- |
| Serde 编码/解码  | `to_vec`、`to_writer`、借用 `from_slice`、`from_reader` 以及对派生的 serde 类型的直接支持。                         |
| 稳定的协议字节   | RFC 8949 首选序列化，以及确定性/规范编码器和可选择的 map 键排序。                                                   |
| 协议级 CBOR 支持 | 简单值、语义标签、大数（bignum）、整数 map 键、字段顺序数组以及通过 `#[derive(cbor2::Cbor)]` 实现的 COSE 风格标签。 |
| 动态或未知数据   | `Value`、`cbor!` 宏以及用于经过验证的透传字节的 `RawValue`。                                                        |
| 安全输入处理     | 仅限单项的格式完好性检查、CBOR 序列迭代、递归限制以及防御性的分配大小控制。                                         |
| 异步边界         | `async_io` 读取或写入一个完整的 CBOR 项，而无需假设 serde 本身是异步的。                                            |
| 调试与检查       | RFC 8949 诊断表示法、美化诊断输出以及配套的 `cbor` 命令行工具。                                                     |
| 嵌入式目标       | 提供支持完整堆分配 API 的 `no_std + alloc`，或针对序列化、格式完好性检查及核心标头编解码器的无分配支持。            |

采用 MIT 许可协议。

## 与其他 CBOR Crate 的对比

[`cbor2-bench`](cbor2-bench/README.md) 工作区在功能和速度上将 cbor2 与 `ciborium 0.2`、`serde_cbor 0.11`、`cbor4ii 1.2` 和 `minicbor 2.2` 进行了对比。这是一个*独立的*（detached）工作区，因此这些 crate 不会进入本库的依赖图、CI 或 MSRV 中。

### 功能特性对比

| 功能特性                                | cbor2 | ciborium | serde_cbor | cbor4ii | minicbor |
| --------------------------------------- | :---: | :------: | :--------: | :-----: | :------: |
| 原生 serde `Serialize`/`Deserialize`    |   ✅   |    ✅     |     ✅      |    ✅    |    ❌¹    |
| `no_std` + `alloc`                      |   ✅   |    ✅     |     ✅      |    ✅    |    ✅     |
| 零分配编码（固定缓冲区）                |   ✅   |    ✅     |     ✅      |   ✅⁵    |    ✅     |
| 无需 `alloc` 的类型化解码               |  ❌²   |    ❌     |     ❌      |   ❌²    |    ✅     |
| 从输入中借用 `&str`/`&[u8]`             |   ✅   |    ❌     |     ✅      |    ✅    |    ✅     |
| 确定性 / 规范编码³                      |   ✅   |    ❌     |     ❌      |    ❌    |    ❌     |
| 动态 `Value` 类型                       |   ✅   |    ✅     |     ✅      |    ✅    |    ❌     |
| 原始透传值 (`RawValue`)                 |   ✅   |    ❌     |     ❌      |   ✅⁶    |    ❌     |
| 语义标签                                |   ✅   |    ✅     |     ✅      |    ✅    |    ✅     |
| 结构体的整数 map 键 (COSE)              |   ✅   |    ❌     |     ❌      |    ❌    |    ✅     |
| 诊断表示法 (RFC 8949 §8)                |   ✅   |    ❌     |     ❌      |    ❌    |    ✅     |
| 异步项 I/O (futures / tokio)            |   ✅   |    ❌     |     ❌      |    ❌    |    ❌     |
| 在不解码的情况下进行验证 / 获取准确大小 |   ✅   |    ❌     |     ❌      |    ❌    |    ◑⁴    |

¹ minicbor 使用其自有的 `#[derive(Encode, Decode)]`；serde 支持位于一个单独的 `minicbor-serde` crate 中。

² 没有一个基于 serde 的 CBOR crate 可以在不使用堆的情况下进行反序列化 —— 但 cbor2 的底层 [`core::Decoder`](https://docs.rs/cbor2/latest/cbor2/core/struct.Decoder.html)（以及 cbor4ii 的底层 `Decode`）仍可在零分配的情况下手动解码。

³ 排序的 map 键，RFC 8949 §4.2.1；大多数 crate 会输出首选的最短格式数字（cbor4ii 将浮点数保持为 64 位），但只有 cbor2 附带了完整的规范编码器。

⁴ minicbor 的 `Decoder::skip` 可以验证结构，但没有获取精确大小的原语。

⁵ cbor4ii 没有公开的 `no_std` 切片（slice）序列化器；它通过 `&mut [u8]` 上的 `to_writer` 填充固定缓冲区，这需要 `std`。

⁶ cbor4ii 的 `RawValue` 是核心层（core-level）的借用类型，未与 serde 集成。

`serde_cbor` 已停止维护；其他库均处于维护状态。

### 基准测试

Apple M1 Pro 上每次操作的中间时间，采用 `no_std + alloc` 路径 (`to_vec` / `from_slice`)；越低越好。完整的 `std` 和 `no_std + no_alloc` 表格、负载定义和测试方法位于 [`cbor2-bench`](cbor2-bench/README.md#results)。

| 操作 / 负载        | cbor2   | ciborium | serde_cbor | cbor4ii | minicbor |
| ------------------ | ------- | -------- | ---------- | ------- | -------- |
| `encode/int_array` | 2.79 µs | 6.59 µs  | 1.67 µs    | 2.92 µs | 3.29 µs  |
| `encode/log_batch` | 13.3 µs | 16.1 µs  | 9.54 µs    | 6.09 µs | 4.56 µs  |
| `encode/blob`      | 102 ns  | 131 ns   | 133 ns     | 127 ns  | 130 ns   |
| `decode/int_array` | 5.34 µs | 11.0 µs  | 3.24 µs    | 3.43 µs | 5.23 µs  |
| `decode/log_batch` | 38.5 µs | 66.3 µs  | 34.0 µs    | 36.8 µs | 21.8 µs  |
| `decode/blob`      | 97.5 ns | 224 ns   | 88.5 ns    | 90.1 ns | 91.1 ns  |

`int_array`（1024 × `u64`）和 `blob`（4 KiB 字节字符串）在所有五个 crate 中生成的字节是完全相同的，因此这些行是严格的同等对比；`log_batch`（128 个结构化记录）使用了每个 crate 特有的推荐编码方式（minicbor 采用整数键数组，体积缩小约 37%；cbor4ii 将浮点数保持为 64 位）。
cbor2 在各项测试中都极具竞争力，**在 `no_std + no_alloc` 路径下表现尤为突出** —— 它拥有 serde 类 crate 中最快的固定缓冲区编码速度，也是唯一提供 `serialized_size` / `validate` 原语的 crate。在 `std`/`alloc` 的结构化吞吐量方面，**cbor4ii 的表现非常亮眼**（且 minicbor 的借用解码器在结构化解码中处于领先地位）；cbor2 在不同场景下与它们交替领先 —— 详情请参见完整表格。在 `no_std + no_alloc` 模式下，cbor2 还提供零分配的*编码*（[`to_slice`]）、*验证*（[`validate`]）和精确的*大小计算*（[`serialized_size`]）。

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

如需使用 `cbor` 命令行工具，请安装 `cbor2-cli`：

```bash
brew install ldclabs/tap/cbor2-cli   # Homebrew，安装 cbor
cargo install cbor2-cli              # Cargo，安装 cbor
```

Windows 用户可以从最新 GitHub Release 下载
[`Cbor2CliSetup-windows-x86_64.exe`](https://github.com/ldclabs/cbor2/releases/latest/download/Cbor2CliSetup-windows-x86_64.exe)。
安装程序会将 `cbor.exe` 放到 `%LOCALAPPDATA%\Programs\cbor2-cli`，并把该目录加入用户 `PATH`；安装后请打开新的终端再运行 `cbor`。

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

`to_writer` 和 `from_reader` 适用于任何 `std::io::Write`/`Read`，且 `Deserializer::into_iter` 可用于解码拼接在一起的数据项流。`from_slice`/`from_reader` 读取第一个主导（leading）的 CBOR 项；当需要确保缓冲区中恰好仅包含一个数据项时，请使用 `validate_slice`（对读取器则使用 `validate`）。

## 致 AI 智能体 (Agent)

代码智能体应首先查阅 [`AGENTS.md`](AGENTS.md) 以获取精简的 API 选择规则，然后参考 [`docs/agent-cookbook.md`](docs/agent-cookbook.md) 获取可复制的方案以及常见的迁移陷阱。可运行的 [`agent_patterns`](examples/agent_patterns.rs) 示例涵盖了单项格式完好性检查、字节字符串、借用反序列化、原始值、CBOR 序列和规范编码。

## 核心亮点

* **完整的 serde 集成** —— 支持直接对 `#[derive(Serialize, Deserialize)]` 类型进行编码和解码。
* **支持借用的 `from_slice`** —— 确定长度（definite-length）的文本和字节字符串体可以直接从输入缓冲区中反序列化；分段的无定长（indefinite）字符串则回退到有所有权的缓冲区。
* **RFC 8949 首选序列化** —— 整数和浮点数始终以最小的无损形式进行编码，包括半精度浮点数。
* **动态 `Value` 类型** —— CBOR 版本的 `serde_json::Value`，附带用于以类似 JSON 语法构建值的 `cbor!` 宏。
* **CBOR 简单值** —— `Simple` 和 `Value::Simple` 保留了 serde 内置的 bool/null 之外的已注册和未分配的简单值，包括 SD-CWT 的 `simple(59)` 等 map 键。
* **标签支持** —— 通过 `tag` 模块中的包装类型捕获并输出语义标签（RFC 8949 §3.4）；`u128`/`i128` 会自动映射到大数（bignum）标签。
* **确定性编码** —— `to_canonical_vec`/`to_canonical_writer` 和 `Value::canonicalize` 实现了核心确定性编码要求（RFC 8949 §4.2.1）：字节级字典序 map 键排序、确定长度、首选序列化、归一化大数和 NaN。对于基于较旧的 RFC 7049 §3.9 “规范 CBOR” 规则（在 RFC 8949 §4.2.3 中保留，并被 ciborium 的 canonical 模块采用）的协议，`*_with` 变体接受 `KeyOrder::LengthFirst` 参数。
* **COSE 风格的整数 map 键、数组和带有 `#[derive(Cbor)]` 的标签** —— 启用 `derive` 特性后，`#[derive(cbor2::Cbor)]` 会生成带有 CBOR 协议细节的 serde `Serialize`/`Deserialize` 实现：标注了 `#[cbor(key = ...)]` 的字段在编码时使用整数 map 键，且容器在编码时会被包装在 CBOR 标签（`#[cbor(tag = ...)]`）中。标签层在解码时是透明的，因此同一类型可以处理包含或不包含标签的协议，而无需定义第二个“裸”结构体并实现 `From`。命名的结构体还可以使用 `#[cbor(array)]` 编码为紧凑的字段顺序 CBOR 数组，同时在 JSON 和代码中保持 Rust 字段名。字段名和类型名保持不变，因此相同的类型仍然可以平滑地序列化为普通 JSON —— `serde_json::to_string(&v)` 可以直接工作，保持原始字段名且不带标签。声明的键、数组形状和标签在运行时仍可通过 `cbor2::Cbor` 特征（trait）进行检查。
* **原始值** —— `RawValue` 延迟解码并保留单个数据项的精确传输字节：序列化时将它们原封不动地拼接进流中，反序列化时则逐字节捕获，适用于签名负载、透传项和延迟解码。`TryFrom` 可在 `RawValue` 和 `Value` 之间进行双向转换。
* **鲁棒的解码** —— 妥善处理不定长数据项、分段字符串、重复 map 键、未知标签和 CBOR 序列（RFC 8742）；限制递归深度，并防止伪造的长度触发巨额内存分配。
* **简明诊断表示法 (Concise Diagnostic Notation)** —— `to_cdn` 可以将原始 CBOR 渲染为由 IETF 简明诊断表示法草案（CDN，`draft-ietf-cbor-edn-literals`）规范化的易读文本形式，在保留不定长标记的同时，与 RFC 8949 附录 A 的普通数据项示例相匹配。API 命名保持了显式的方向性：`to_cdn*` 将 CBOR 字节渲染为 CDN 文本，而 `cdn_to_vec`、`cdn_sequence_to_vec` 和 `from_cdn` 则将 CDN 文本解析为 CBOR 字节或 serde 值；较旧的 `diagnostic*` 名称作为兼容类别名予以保留。CDN 输入支持注释、基于编码的字节字符串、嵌入式 CBOR 序列、编码指示器、标签、简单值，以及 `dt`/`DT`、`ip`/`IP`、`b1`/`t1`、`ilbs`/`ilts`、`bytes`、`same` 和 `float` 等应用扩展；启用 `cdn` 特性后还会添加依赖外部 crate 的 `hash`、`cri` 和 `CRI`。`bytes<<"ä", h'2f'>>` 会生成 `h'c3a42f'`，而 `same<< float'47110815', 0x1.22102ap+15 >>` 会校验同一数据项的不同写法并输出第一个实参。`Value` 通过相同的表示法实现 `Display`，并将其缩进形式作为 `Debug` 实现。对于整数键协议 map，`to_cdn_pretty_with_key_comments` 可以在传输整数键旁边添加 CDN `// "iss"` 类似的注释。
* **无分配辅助函数** —— `validate` 与零拷贝的 `validate_slice` 是针对单个 CBOR 数据项的格式完好性检查（RFC 8949 §5.3.1，包括文本 UTF-8），`serialized_size` 计算任何可序列化值的精确编码大小，而 `to_slice` 则将数据编码到调用者提供的缓冲区中；这些操作均不分配堆内存。
* **异步项 I/O** —— `async_io` 模块在异步字节流上对完整的 CBOR 项进行分帧，随后在数据项缓冲完成后复用正常的同步 serde API。针对不受信任的流，提供了有界的读取辅助函数。
* **底层标头编解码器** —— `core` 模块为需要精确传输控制的应用暴露了拉取/推送式 `Header` 接口。
* **`no_std` 支持** —— `default-features = false, features = ["alloc"]` 保持了完整的 API（减去了 `std::io` 互操作和 `HashMap` 转换）；即使在没有 `alloc` 的情况下，此 crate 仍然可以进行序列化（`to_writer`/`to_slice`/`serialized_size`）、格式完好性检查，并支持 `core` 标头编解码器。

## Crate 特性

| 特性      | 默认启用         | 作用                                                                                                                    |
| --------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `std`     | 是               | 为每个 `std::io::Read`/`Write` 实现 `cbor2::io` 特征，添加 `async_io` 并添加 `HashMap` 转换。隐式启用 `alloc`。         |
| `alloc`   | 是（通过 `std`） | 所有需要堆的操作：`Value`、`to_vec`/`from_slice`/`from_reader`、`RawValue`、`diagnostic`、确定性编码器以及 `cbor!` 宏。 |
| `cdn`     | 否               | 添加需要外部 crate 的 CDN 输入扩展：`hash`、`cri` 和 `CRI`。隐式启用 `alloc`。                                          |
| `derive`  | 否               | `#[derive(cbor2::Cbor)]` 宏。                                                                                           |
| `futures` | 否               | 为 `futures_io::AsyncRead`/`AsyncWrite` 添加 `async_io::futures` 辅助函数。隐式启用 `std`。                             |
| `tokio`   | 否               | 为 `tokio::io::AsyncRead`/`AsyncWrite` 添加 `async_io::tokio` 辅助函数。隐式启用 `std`。                                |

在不启用任何特性的情况下，此 crate 是一个适用于受限目标的 `#![no_std]` 核心库：支持通过 `to_writer`/`to_slice`/`serialized_size` 进行流式序列化、格式完好性验证、`tag` 包装器以及 `core` 标头编解码器。通过 serde 反序列化需要 `alloc`。读取器和写入器实现了简易的 `cbor2::io` 特征，这些特征已为字节切片（以及在启用 `alloc` 时的 `Vec<u8>`）提供：

```toml
[dependencies]
cbor2 = { version = "1", default-features = false } # 或 features = ["alloc"]
```

```rust
// 适用于 no_std + 无 alloc 目标：
let mut buffer = [0u8; 64];
let item = cbor2::to_slice(&("id", 42u8), &mut buffer).unwrap();
assert!(cbor2::validate(&item[..]).is_ok());
```

## 指南

### 字节字符串与 `serde_bytes`

一个常见的 serde 陷阱：裸 `Vec<u8>` 和 `&[u8]` 会序列化为整数数组，而不是 CBOR 字节字符串。对于二进制负载，请使用 [`serde_bytes`](https://docs.rs/serde_bytes/latest/serde_bytes/)。

```rust
let bytes = vec![1u8, 2, 3, 4];

// 裸 Vec<u8>: [1, 2, 3, 4]
assert_eq!(hex::encode(cbor2::to_vec(&bytes).unwrap()), "8401020304");

// serde_bytes: h'01020304'
let bytes = serde_bytes::ByteBuf::from(bytes);
assert_eq!(hex::encode(cbor2::to_vec(&bytes).unwrap()), "4401020304");
```

对于派生结构体中的字段，请显式标注字节缓冲区：

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

如果使用 `Value` 构建数据，请使用 `Value::Bytes(...)` 或为字节切片/向量实现的 `From`；这些已经表示 CBOR 字节字符串。

### 从切片中进行借用反序列化

`from_slice` 具有生命周期感知能力：确定长度（definite-length）的文本和字节字符串体可以直接从输入中借用。这与 serde_json 的切片路径相匹配，适用于签名负载或输入缓冲区生命周期足够长的 COSE 结构。

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

仍接受不定长字符串，但由于它们的实体被拆分在多个分段中，因此无法直接借用。

### 使用 `#[derive(Cbor)]` 获得 COSE 风格的整数 map 键、数组和标签

启用 `derive` 特性后，`#[derive(cbor2::Cbor)]` 会生成带有 CBOR 协议细节的 serde `Serialize`/`Deserialize` 实现：标注了 `#[cbor(key = ...)]` 的字段使用整数 map 键，且容器在编码时会包装在 CBOR 标签（`#[cbor(tag = ...)]`）中。标签层在解码时是透明的，因此相同的类型可以处理包含或不包含标签的协议，而不需要定义第二个“裸”结构体并实现 `From`。命名的结构体还可以使用 `#[cbor(array)]` 编码为紧凑的字段顺序 CBOR 数组，同时保留 Rust 字段名以用于 JSON 和代码。字段名和类型名保持不变，因此相同的类型仍然可以序列化为普通 JSON。

```toml
[dependencies]
cbor2 = { version = "1", features = ["derive"] }
```

这逐字节复现了 [RFC 9052, Appendix C.4.1](https://datatracker.ietf.org/doc/html/rfc9052#appendix-C.4) 的简单加密消息（52 字节）：

```rust
use cbor2::Cbor;

/// 受保护的标头参数 (RFC 9052 §3.1)。它们作为包含其自身 CBOR 编码的字节字符串进行传输。
#[derive(Debug, PartialEq, Cbor)]
struct Protected {
    /// 10 = AES-CCM-16-64-128 (RFC 9053 §4.2)
    #[cbor(key = 1)]
    alg: i8,
}

/// 未受保护的标头参数。
#[derive(Debug, PartialEq, Cbor)]
struct Unprotected {
    #[cbor(key = 5)]
    #[serde(with = "serde_bytes")]
    iv: Vec<u8>,
}

/// COSE_Encrypt0 (RFC 9052 §5.2)：外层带有标签 16 的
/// `[protected: bstr, unprotected: map, ciphertext: bstr]`。
#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 16)]
struct CoseEncrypt0(
    #[serde(with = "serde_bytes")] Vec<u8>, // 受保护的标头，已编码
    Unprotected,
    #[serde(with = "serde_bytes")] Vec<u8>, // 密文
);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 受保护的标头是编码后的 map {1: 10}。
    let protected = cbor2::to_canonical_vec(&Protected { alg: 10 })?;
    assert_eq!(hex::encode(&protected), "a1010a");

    let msg = CoseEncrypt0(
        protected,
        Unprotected {
            iv: hex::decode("89f52f65a1c580933b5261a78c")?,
        },
        hex::decode("5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569")?,
    );

    // 逐字节对应的 RFC 52 字节消息。
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

    // 解码需要标签 16 并恢复每个层级。
    let back: CoseEncrypt0 = cbor2::from_slice(&bytes)?;
    assert_eq!(back, msg);
    let header: Protected = cbor2::from_slice(&back.0)?;
    assert_eq!(header, Protected { alg: 10 });

    // JSON 保持自然形式 —— 原始字段名、无标签、无整数键。
    let json = serde_json::to_string(&header)?;
    assert_eq!(json, r#"{"alg":10}"#);
    Ok(())
}
```

可运行的 [`examples/cose.rs`](examples/cose.rs) 将其扩展为 [`cose2`](https://github.com/ldclabs/cose2)（一个基于 cbor2 构建的完整 RFC 9052 COSE 和 RFC 8392 CWT 库）的实际传输类型 —— 包含一个命名的 `#[cbor(array)]` 结构体、一个可选（分离）的密文和透明的标签解码，从而使一个类型既能解码带标签的消息也能解码不带标签的消息：运行命令为 `cargo run --features derive --example cose`。

配套的 [`examples/cwt.rs`](examples/cwt.rs) 是 cose2 的 CWT 声明集（RFC 8392）：一个带有已注册整数声明键、自然 JSON 名称、`skip_serializing_if` 声明省略、COSE 标签键控的 `#[serde(flatten)]` 扩展声明以及相同透明标签解码的带标签 *map*。它还使用了 `to_cdn_pretty_with_key_comments(&bytes[..], Claims::KEYS)`，以便诊断输出保持与整数键传输格式一致，同时将匹配的字符串键作为代码注释显示：

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

使用 `cargo run --features derive --example cwt` 运行它。

派生宏还实现了 `cbor2::Cbor` 特征（trait），该特征在运行时公开了声明的协议细节 —— 无分配的常量 `T::KEYS`、`T::TAG` 和 `T::ARRAY`，以及作为 `BTreeMap<String, i128>` 的 `value.keys()`：

```rust
use cbor2::Cbor; // 统一导入：派生宏和特征

assert_eq!(Protected::KEYS, &[("alg", 1)]);
assert_eq!(CoseEncrypt0::TAG, Some(16));
assert!(!CoseEncrypt0::ARRAY);
```

对于其传输形状为数组但在 Rust 形式中应当保持命名字段的 COSE 结构，请添加 `#[cbor(array)]`：

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

`RawValue` 延迟解码并保留单个数据项的精确传输字节 —— 这是签名负载的正确处理方式：

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
// 在 signed.payload.as_bytes() 上验证 signed.signature，然后：
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
assert!(cbor2::validate(&stream[..]).is_err()); // 序列不是单个数据项
```

### 异步项 I/O

Serde 本身是同步的，但异步传输通常需要数据项边界。`async_io` 模块将一个完整的 CBOR 数据项读取到缓冲区中，验证与 `validate` 相同的结构，然后允许您在自己拥有的字节上调用 `from_slice`。

```rust
# async fn example<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<(), cbor2::de::Error> {
let item = cbor2::async_io::read_item(reader).await?;
let value: cbor2::Value = cbor2::from_slice(&item)?;
# Ok(())
# }
```

对于不受信任的对端，除非外部传输层已强制限制消息大小，否则请使用 `read_item_with_limit` 或 `read_value_with_limit`：

```rust
# async fn bounded<R: cbor2::async_io::AsyncRead + ?Sized>(reader: &mut R) -> Result<cbor2::Value, cbor2::de::Error> {
let value: cbor2::Value = cbor2::async_io::read_value_with_limit(reader, 1 << 20).await?;
# Ok(value)
# }
```

使用 `async_io::write_value` 序列化并发送值，或者在已有经过验证的单项字节缓冲区时使用 `async_io::write_item`。

启用 `futures` 或 `tokio` 特性后，可以使用特定于运行时的适配器，而无需编写本地包装器：

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

可运行的示例位于 `examples/` 中：

```bash
cargo run --example basic
cargo run --example bytes
cargo run --example sequence
cargo run --example core_headers
cargo run --features derive --example cose
cargo run --features derive --example cwt
```

## 设计决策

此实现特意与 ciborium 的传输行为保持一致，以便两个 crate 能够实现逐字节互操作：

* 数字始终以其最小的无损形式进行编码，正如确定性编码（RFC 8949 §4.2.1）所要求的那样。Rust 中的整数宽度被视为内存中的细节，而不是传输属性。
* 枚举（Enum）编码为裸字符串（单元变体）或单条目 map `{variant: payload}`（其他所有变体）。
* `Value` 的 map 是 `Vec<(Value, Value)>`，保留了传输顺序和任意键。
* 解码遵循鲁棒性原则：接受不定长、分段字符串、半精度浮点数和未知标签，即使编码过程永远不会产生它们。

## 历史背景

该项目源自 [Andrew Gallant](https://github.com/BurntSushi) 于 2015 年创建的 `cbor` crate，该 crate 基于 serde 之前的 `rustc-serialize` 框架构建，且多年未维护。0.5 版本是基于 [serde](https://serde.rs) 的全新重写版本，由 [LDC Labs](https://github.com/ldclabs) 维护，并作为 **`cbor2`** 发布 —— crates.io 上的 `cbor` 名称仍保留给遗留的 0.4 版本 —— 1.0 版本对其进行了稳定。0.4 版本的 API 均未保留。

此次重写遵循了 [ciborium](https://github.com/enarx/ciborium) 的设计（并且与其传输兼容）—— 非常感谢其作者们。

## 命令行工具

工作区在 [`cbor2-cli`](cbor2-cli/README.md) 中提供了一个 `cbor` 命令行工具。原生的 `cbor` 命令可将任何 CBOR（来自文件、标准输入、十六进制字符串或 base64 字符串）显示为诊断表示法（RFC 8949 §8，规范化为 CDN）；`decode` 默认显示美化诊断表示法，并使用 `--json` 转换为易读但有损的 JSON，`encode` 将 JSON 兼容值或 CDN 文本转换为 CBOR，`encode --json` 强制使用严格 JSON 输入，`decode`/`encode --diag`/`--cdn` 使用 CDN 表示法，`encode --hex` 打印可复制的 CBOR 十六进制以用于智能体和文档，而 `validate` 则用于验证完整的 CBOR 输入：

```bash
brew install ldclabs/tap/cbor2-cli   # Homebrew
cargo install cbor2-cli              # Cargo
```

Windows 用户可以下载最新的
[`Cbor2CliSetup-windows-x86_64.exe`](https://github.com/ldclabs/cbor2/releases/latest/download/Cbor2CliSetup-windows-x86_64.exe)
安装包；安装完成后打开新的终端并运行 `cbor --help`。

```bash
$ cbor bf61610161629f0203ffff
{_ "a": 1, "b": [_ 2, 3]}

$ echo '{"name": "example", "ok": true}' | cbor encode --json | cbor decode --json
{
  "name": "example",
  "ok": true
}

$ echo '{"name": "example", "ok": true}' | cbor encode --hex
a2646e616d65676578616d706c65626f6bf5

$ printf "bytes<<\"hi\", h'2f'>>" | cbor encode --diag --hex
4368692f

$ cbor validate a2646e616d65676578616d706c65626f6bf5
valid
```

## 测试

`cargo test` 会运行单元测试、单个集成测试二进制文件和文档测试 —— 包括 RFC 8949 附录 A 的测试向量，以及针对 I/O 失败和格式错误输入的错误注入测试。CI 在各种特性组合下进行构建和测试，乃至裸机 `no_std` 目标。使用 `cargo llvm-cov` 测得的代码覆盖率为 100% 的函数覆盖率和约 98% 的行覆盖率；唯一未执行的行是无法发生的防御性分支，例如 `RawValue` 有效性不变性规则排除了的错误路径。

## 最低支持的 Rust 版本

Rust 1.85。

## 许可协议

采用 MIT 许可协议。
