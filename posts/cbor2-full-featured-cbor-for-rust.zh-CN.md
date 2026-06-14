# cbor2：适用于 Rust 的全功能 CBOR 库

大多数 Rust 的 CBOR 库都只是序列化器——它们将结构体转换为字节，然后再转换回来，仅此而已。但相比 JSON，[CBOR (RFC 8949)](https://www.rfc-editor.org/rfc/rfc8949) 具有更丰富的功能特性：语义标签（semantic tags）、大数（bignums）、确定性编码（deterministic encoding）、诊断表示法（diagnostic notation）以及不定长流（indefinite-length streams）。一旦你需要其中任何一种特性——例如 COSE、签名载荷（signed payloads）或规范哈希（canonical hash）——你通常就必须在序列化器的基础上手工实现它们。

**cbor2** 试图将这些完整的功能特性整合为一个原生支持 serde 的工具包，同时在速度上保持竞争力，并能够缩减支持无分配器（allocator）的 `no_std` 环境。

首先说明一下它的渊源，因为致敬前人的工作很重要：cbor2 源自 Andrew Gallant 最初的 `cbor` crate。0.5 版本是基于 serde 和 RFC 8949 从头重写的，而 1.0 版本则使其稳定下来。在这个领域还有其他优秀的 crate——`ciborium` 和 `minicbor` 表现扎实，`cbor4ii` 速度很快，而大多数人仍在使用的主流库 `serde_cbor` 自 2021 年起已不再维护。cbor2 并不打算取代它们中的任何一个，它的定位是服务于“我需要完整的协议支持，而不仅仅是一个序列化器”这一细分需求。

## 同一个结构体，两种传输格式

这是我最满意的部分。COSE（RFC 9052——WebAuthn、CWT 等背后的签名/加密格式）使用**整数**而非字符串来作为其 map 的键，并将消息封装在标签（tags）中。serde 的数据模型无法直接表达这两者。通常的做法是维护第二套类型，或者进行手动编码。

cbor2 可以让同一个结构体同时兼顾两者：

```rust
use cbor2::Cbor;

#[derive(Cbor)]
#[cbor(tag = 98)]
struct CoseSign {
    #[cbor(key = 1)]
    kty: u8,
    #[cbor(key = 3)]
    alg: i8,
    comment: String, // 无 key -> 在传输时保持为文本键
}
```

使用 cbor2 进行编码，你就可以得到 COSE 所期望的紧凑、以整数为键、带标签 98 的格式。而将*同一个值*传给 `serde_json::to_string`，你将得到普通的 `{"kty": ..., "alg": ..., "comment": ...}`——字段名保持原样，且没有标签。同一套类型，既能生成用于日志和调试端点的 JSON，又能生成用于网络传输的 COSE。

在底层，`#[derive(Cbor)]` 通过一个隐藏的 `#[serde(remote)]` 影子类型和容器名称标记来自动生成 serde 实现，因此真实类型的字段和类型名称绝不会被篡改。无需注册表，也无需链接时技巧——它在 `wasm32-unknown-unknown` 上同样可以正常工作。

## 当精确字节至关重要时

对于签名和内容寻址来说，生成*哪些*字节正是核心所在。cbor2 提供了确定性编码（RFC 8949 §4.2.1——排序的 map 键、最短形式的整数/浮点数、规范化的大数和 NaN）：

```rust
let canonical = cbor2::to_canonical_vec(&value)?;
```

至于另一个方向——对非自身生成的字节进行签名验证——则可以使用 `RawValue`，它能将单个项捕获为已验证但未解码的原始字节：

```rust
#[derive(Serialize, Deserialize)]
struct Signed {
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
    payload: cbor2::RawValue, // 按原样逐字节捕获
}
// 在信任内容之前，先对 signed.payload.as_bytes() 验证 signature，然后再进行解码
```

在信任内容之前，你可以基于精确的传输字节（甚至是采用非首选拼写形式的字节）进行验证。如果先将其转换为有类型的值然后再转换回来，会在无形中对数据进行重新编码，从而破坏签名。

此外，该库还提供了一个带有 `cbor!` 宏的动态 `Value`、RFC 8949 第 8 节定义的诊断表示法（以及一个可打印该表示法的 `cbor` 命令行工具），以及一个用于检查输入是否恰好为单个格式良好项的 `validate` 函数。

## 一路向下兼容至 `no_alloc`

cbor2 支持 `no_std`。在启用 `alloc` 的情况下，你可以获得完整的基于堆内存的 API；而在未启用的情况下，你仍然可以使用序列化、验证和精确大小计算功能——这些操作都不需要进行内存分配：

```rust
let mut buf = [0u8; 256];
let item = cbor2::to_slice(&value, &mut buf)?; // 编码到你的缓冲区中
let size = cbor2::serialized_size(&value)?;    // 精确长度，无需输出缓冲区
cbor2::validate(&item[..])?;                   // 格式是否良好？（包括 UTF-8）
```

这是一个需要坦白说明的局限性：**没有任何一个基于 serde 的 CBOR crate 可以在没有堆内存的情况下进行反序列化**——因为反序列化器需要一个临时缓冲区。cbor2 也不例外。只有不基于 serde 的 `minicbor` 才能在没有分配器的情况下解码出有类型的值。作为替代，cbor2 提供了一个低层级的 `core::Decoder` 拉取式（pull）API，如果你愿意手动控制它，就可以在零分配的情况下读取 CBOR：

```rust
use cbor2::core::{Decoder, Header};

let mut dec = Decoder::from(&bytes[..]);
let Header::Array(Some(n)) = dec.pull()? else { /* ... */ };
// 拉取头部；将字符串/字节体读取到你自己的 &mut [u8] 中
```

因此，无分配情况下的实际体验是：通过高层级 API 实现零分配的编码、验证和大小计算；通过 `core::Decoder` 进行手动解码，而不是通过 serde 进行有类型的反序列化。我希望提前让你知晓这一点，而不是等到项目进行到一半时才发现。

## 它的速度快吗？

挺快的，但我不会声称它是最快的，因为它确实不是，而且你很容易验证这一点。我构建了一个[独立的基准测试工作区](https://github.com/ldclabs/cbor2/tree/main/cbor2-bench#results)，在尽可能保持字节载荷一致的情况下，对比了 cbor2 与 ciborium、serde_cbor, cbor4ii 以及 minicbor 在三种部署模式（`std`、`no_std + alloc`、`no_std + no_alloc`）下的性能。这样，整数数组和字节字符串的对比就是完全公平的（apples-to-apples）。

坦率地总结：cbor2 的编码和解码性能处于第一梯队，并全面超越了 ciborium，但 `serde_cbor` 和 `cbor4ii` 在某些编码场景中略胜一筹，而 `minicbor` 得益于更紧凑的传输形式在结构化解码方面处于领先。cbor2 优势明显的地方在于 `no_std + no_alloc`——它是基于 serde 的众多 crate 中固定缓冲区编码速度最快的，并且还提供了其他库未配备的 `validate`/`serialized_size` 原语。（构建这套测试也发现了一些有趣的细节，比如 cbor4ii 的解码器会拒绝将 `f32` 缩窄后的浮点数反序列化到 `f64` 字段中——因此，每个 crate 在测试中都解码了各自生成的字节。）

完整的功能矩阵、场景细分表格以及测试方法，均可在[基准测试自述文件（README）](https://github.com/ldclabs/cbor2/tree/main/cbor2-bench#results)中找到。

## 它不能做什么

- 在没有 `alloc` 的情况下无法进行有类型的反序列化（这是上文提到的 serde 生态系统的客观现实）。
- 它是一个非常新的库，因此采用率较低，难免会存在不完善的地方。
- 如果你需要绝对最快的结构化解码，并且可以使用非 serde 的 derive 宏，那么 `minicbor` 是更好的选择——这完全合理。

## 尝试使用它

```toml
[dependencies]
cbor2 = "1"
```

- crates.io: <https://crates.io/crates/cbor2>
- docs: <https://docs.rs/cbor2>
- repo: <https://github.com/ldclabs/cbor2>

非常欢迎大家提交 Bug 报告、进行设计层面的探讨，或者提出“为什么不直接使用某某库”这类质疑——这也正是我发布它的原因。
